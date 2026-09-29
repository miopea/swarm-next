//! This Hive's own recent log, kept in memory so a member can share it with its
//! Keeper (ADR 0112).
//!
//! ⚠️ BOUNDED AND OWNED, LIKE EVERY BUFFER HERE. A thousand lines, oldest out
//! first, one message cut at a kilobyte; what falls out before it is shared is
//! counted and said, not hidden. It lives for the life of the process: the
//! journal on the machine is the durable copy, this is the one that can travel.

use std::{
    collections::VecDeque,
    fmt::{self, Write as _},
    sync::{Arc, Mutex},
};

use swarm_domain::{
    DiagnosticBatch, DiagnosticEntry, DiagnosticLevel, MAX_DIAGNOSTIC_BATCH_ENTRIES,
    MAX_DIAGNOSTIC_MESSAGE_BYTES, MAX_DIAGNOSTIC_TARGET_BYTES, clip_diagnostic_text,
};
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, filter::filter_fn, layer::Context, registry::LookupSpan};

/// Where a Keeper writes the lines its members sent, so its own journal can be
/// read with theirs. Never kept in this Hive's own buffer: a Keeper's copy of a
/// member's line is not a line this Hive wrote.
pub(crate) const MEMBER_LOG_TARGET: &str = "swarm_api::member_log";

/// Lines kept. At the steady rate of a quiet Hive this is hours.
const CAPACITY: usize = 1000;

/// Modules whose DEBUG lines are kept as well as their INFO and above: the
/// Apiary connection's own lifecycle. On 2026-09-29 these were what explained
/// a watch that never started and a takeover whose keys never landed, and at
/// INFO they are silent.
const DEBUG_TARGETS: [&str; 3] = [
    "swarm_api::federation_events",
    "swarm_api::watch_producer",
    "swarm_api::takeover_producer",
];

/// Whether a line belongs in the shared log: Swarm's own, at INFO and above,
/// or DEBUG from the Apiary connection.
fn captured(metadata: &Metadata<'_>) -> bool {
    let target = metadata.target();
    if !target.starts_with("swarm") || target == MEMBER_LOG_TARGET {
        return false;
    }
    let level = *metadata.level();
    level <= Level::INFO || (level == Level::DEBUG && DEBUG_TARGETS.contains(&target))
}

pub struct DiagnosticLog {
    /// Tells one run of this process from the next, since sequences restart.
    boot_id: String,
    ring: Mutex<Ring>,
}

#[derive(Default)]
struct Ring {
    last_sequence: u64,
    entries: VecDeque<DiagnosticEntry>,
    /// The last line the Keeper accepted.
    shipped_through: u64,
}

impl DiagnosticLog {
    #[must_use]
    pub fn new() -> Self {
        Self {
            boot_id: format!("{}-{}", std::process::id(), now_ms()),
            ring: Mutex::new(Ring::default()),
        }
    }

    fn record(&self, level: DiagnosticLevel, target: &str, message: &str) {
        // A poisoned buffer loses a log line rather than the process.
        let Ok(mut ring) = self.ring.lock() else {
            return;
        };
        ring.last_sequence += 1;
        let sequence = ring.last_sequence;
        if ring.entries.len() == CAPACITY {
            ring.entries.pop_front();
        }
        ring.entries.push_back(DiagnosticEntry {
            sequence,
            at_ms: now_ms(),
            level,
            target: clip_diagnostic_text(target, MAX_DIAGNOSTIC_TARGET_BYTES),
            message: clip_diagnostic_text(message, MAX_DIAGNOSTIC_MESSAGE_BYTES),
        });
    }

    /// The lines after the last batch the Keeper accepted, oldest first, or
    /// nothing when there is nothing new.
    pub(crate) fn pending_batch(&self) -> Option<DiagnosticBatch> {
        let ring = self.ring.lock().ok()?;
        let after = ring.shipped_through;
        let entries: Vec<DiagnosticEntry> = ring
            .entries
            .iter()
            .filter(|entry| entry.sequence > after)
            .take(MAX_DIAGNOSTIC_BATCH_ENTRIES)
            .cloned()
            .collect();
        let first = entries.first()?.sequence;
        Some(DiagnosticBatch {
            boot_id: self.boot_id.clone(),
            dropped: first.saturating_sub(after + 1),
            entries,
        })
    }

    /// Records that the Keeper accepted everything up to `sequence`.
    pub(crate) fn mark_shipped(&self, sequence: u64) {
        if let Ok(mut ring) = self.ring.lock() {
            ring.shipped_through = ring.shipped_through.max(sequence);
        }
    }

    /// The most recent lines, oldest first, for this Hive's own view.
    pub(crate) fn recent(&self, limit: usize) -> Vec<DiagnosticEntry> {
        let Ok(ring) = self.ring.lock() else {
            return Vec::new();
        };
        let skip = ring.entries.len().saturating_sub(limit);
        ring.entries.iter().skip(skip).cloned().collect()
    }
}

impl Default for DiagnosticLog {
    fn default() -> Self {
        Self::new()
    }
}

/// The tracing layer that feeds a [`DiagnosticLog`].
///
/// ⚠️ FILTERED PER LAYER, NOT BY THE PROCESS FILTER. The journal's filter is
/// the operator's `RUST_LOG`; this one is fixed, so what a Keeper can read does
/// not depend on how a member happened to be started. And it names exactly what
/// it wants, so a library's TRACE never builds an event only to be discarded.
pub fn diagnostic_layer<S>(log: Arc<DiagnosticLog>) -> impl Layer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    DiagnosticLayer { log }.with_filter(filter_fn(captured))
}

struct DiagnosticLayer {
    log: Arc<DiagnosticLog>,
}

impl<S: Subscriber> Layer<S> for DiagnosticLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut line = Line::default();
        event.record(&mut line);
        self.log
            .record(level(*metadata.level()), metadata.target(), &line.finish());
    }
}

fn level(level: Level) -> DiagnosticLevel {
    match level {
        Level::ERROR => DiagnosticLevel::Error,
        Level::WARN => DiagnosticLevel::Warn,
        Level::INFO => DiagnosticLevel::Info,
        _ => DiagnosticLevel::Debug,
    }
}

/// The message, then its fields as `name=value`, the way the journal shows it.
#[derive(Default)]
struct Line {
    message: String,
    fields: String,
}

impl Line {
    fn finish(self) -> String {
        self.message + &self.fields
    }
}

impl Visit for Line {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;

    fn with_log(test: impl FnOnce()) -> Arc<DiagnosticLog> {
        let log = Arc::new(DiagnosticLog::new());
        let subscriber = tracing_subscriber::registry().with(diagnostic_layer(Arc::clone(&log)));
        tracing::subscriber::with_default(subscriber, test);
        log
    }

    #[test]
    fn swarm_lines_are_kept_and_everything_else_is_not() {
        let log = with_log(|| {
            tracing::warn!(target: "swarm_api", step = "project catalog", "synchronisation stopped");
            tracing::info!(target: "swarm_api", "Swarm API listening");
            tracing::debug!(target: "swarm_api", "federation pass starting");
            tracing::debug!(target: "swarm_api::takeover_producer", lease = %"lease-1", "takeover relay connected to Keeper");
            tracing::warn!(target: "hyper", "a library's own complaint");
            tracing::warn!(target: MEMBER_LOG_TARGET, "a line a member sent this Keeper");
        });
        let kept: Vec<(DiagnosticLevel, String)> = log
            .recent(10)
            .into_iter()
            .map(|entry| (entry.level, entry.message))
            .collect();
        assert_eq!(
            kept,
            vec![
                (
                    DiagnosticLevel::Warn,
                    "synchronisation stopped step=project catalog".to_owned()
                ),
                (DiagnosticLevel::Info, "Swarm API listening".to_owned()),
                (
                    DiagnosticLevel::Debug,
                    "takeover relay connected to Keeper lease=lease-1".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn the_buffer_is_bounded_and_says_what_it_lost() {
        let log = DiagnosticLog::new();
        for index in 0..CAPACITY + 50 {
            log.record(DiagnosticLevel::Info, "swarm_api", &format!("line {index}"));
        }
        assert_eq!(log.recent(usize::MAX).len(), CAPACITY);

        let batch = log.pending_batch().expect("lines to send");
        assert_eq!(batch.entries.len(), MAX_DIAGNOSTIC_BATCH_ENTRIES);
        assert_eq!(batch.entries[0].sequence, 51);
        assert_eq!(
            batch.dropped, 50,
            "the fifty evicted before sending are named"
        );
        assert_eq!(batch.validate(), Ok(()));

        log.mark_shipped(batch.entries.last().unwrap().sequence);
        let next = log.pending_batch().expect("the rest");
        assert_eq!(next.entries[0].sequence, 251);
        assert_eq!(next.dropped, 0);
    }

    #[test]
    fn nothing_new_sends_nothing_and_long_lines_are_cut() {
        let log = DiagnosticLog::new();
        assert!(log.pending_batch().is_none());
        log.record(
            DiagnosticLevel::Warn,
            "swarm_api",
            &"é".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES),
        );
        let batch = log.pending_batch().unwrap();
        assert!(batch.entries[0].message.len() <= MAX_DIAGNOSTIC_MESSAGE_BYTES);
        log.mark_shipped(batch.entries[0].sequence);
        assert!(log.pending_batch().is_none());
    }
}
