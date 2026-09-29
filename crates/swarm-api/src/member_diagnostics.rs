//! What each member Hive has shared of its log with this Keeper (ADR 0112).
//!
//! In memory and bounded: a thousand lines per Hive, for at most as many Hives
//! as an Apiary directory holds. Every line is also written to this Keeper's
//! own journal as it arrives, which is the durable copy — so a Keeper restart
//! empties this view without losing what was said.

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use serde::Serialize;
use swarm_domain::{DiagnosticBatch, DiagnosticEntry, HiveId, MAX_APIARY_DIRECTORY_ENTRIES};

/// Lines kept per Hive.
const PER_HIVE: usize = 1000;

#[derive(Default)]
pub(crate) struct MemberDiagnostics {
    hives: Mutex<HashMap<HiveId, HiveLog>>,
}

#[derive(Default)]
struct HiveLog {
    boot_id: String,
    last_sequence: u64,
    entries: VecDeque<DiagnosticEntry>,
    dropped: u64,
    received_at: i64,
}

/// One Hive's shared log as the Keeper's roster reads it.
#[derive(Debug, Default, Serialize)]
pub(crate) struct MemberLogView {
    /// Oldest first.
    pub(crate) entries: Vec<DiagnosticEntry>,
    /// Lines the member said it lost before it could send them.
    pub(crate) dropped: u64,
    /// When this Keeper last received anything from it; absent if never.
    pub(crate) received_at: Option<i64>,
}

impl MemberDiagnostics {
    /// Keeps the lines not already held and returns them, so the caller can
    /// write exactly those to the journal. A new `boot_id` means the member
    /// restarted and its sequences began again.
    pub(crate) fn accept(
        &self,
        hive: HiveId,
        batch: &DiagnosticBatch,
        now: i64,
    ) -> Vec<DiagnosticEntry> {
        let Ok(mut hives) = self.hives.lock() else {
            return Vec::new();
        };
        if !hives.contains_key(&hive) && hives.len() >= MAX_APIARY_DIRECTORY_ENTRIES {
            return Vec::new();
        }
        let log = hives.entry(hive).or_default();
        if log.boot_id != batch.boot_id {
            log.boot_id.clone_from(&batch.boot_id);
            log.last_sequence = 0;
        }
        log.dropped = log.dropped.saturating_add(batch.dropped);
        log.received_at = now;
        let fresh: Vec<DiagnosticEntry> = batch
            .entries
            .iter()
            .filter(|entry| entry.sequence > log.last_sequence)
            .cloned()
            .collect();
        for entry in &fresh {
            if log.entries.len() == PER_HIVE {
                log.entries.pop_front();
            }
            log.entries.push_back(entry.clone());
        }
        if let Some(last) = fresh.last() {
            log.last_sequence = last.sequence;
        }
        fresh
    }

    pub(crate) fn view(&self, hive: HiveId) -> MemberLogView {
        let Ok(hives) = self.hives.lock() else {
            return MemberLogView::default();
        };
        hives
            .get(&hive)
            .map_or_else(MemberLogView::default, |log| MemberLogView {
                entries: log.entries.iter().cloned().collect(),
                dropped: log.dropped,
                received_at: Some(log.received_at),
            })
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::DiagnosticLevel;

    use super::*;

    fn batch(boot: &str, sequences: std::ops::RangeInclusive<u64>) -> DiagnosticBatch {
        DiagnosticBatch {
            boot_id: boot.to_owned(),
            entries: sequences
                .map(|sequence| DiagnosticEntry {
                    sequence,
                    at_ms: 1,
                    level: DiagnosticLevel::Warn,
                    target: "swarm_api".to_owned(),
                    message: format!("line {sequence}"),
                })
                .collect(),
            dropped: 0,
        }
    }

    #[test]
    fn a_resent_batch_adds_nothing_and_a_restart_starts_counting_again() {
        let store = MemberDiagnostics::default();
        let hive = HiveId::new();
        assert_eq!(store.accept(hive, &batch("boot-1", 1..=3), 10).len(), 3);
        // The response was lost and the member sent it again, plus one more.
        assert_eq!(store.accept(hive, &batch("boot-1", 1..=4), 11).len(), 1);
        // The member restarted: sequence 1 again, and it is new.
        assert_eq!(store.accept(hive, &batch("boot-2", 1..=2), 12).len(), 2);

        let view = store.view(hive);
        assert_eq!(view.entries.len(), 6);
        assert_eq!(view.received_at, Some(12));
        assert_eq!(store.view(HiveId::new()).received_at, None);
    }

    #[test]
    fn each_hive_keeps_a_bounded_log() {
        let store = MemberDiagnostics::default();
        let hive = HiveId::new();
        for start in (1..=1200_u64).step_by(200) {
            store.accept(hive, &batch("boot", start..=start + 199), 1);
        }
        let view = store.view(hive);
        assert_eq!(view.entries.len(), PER_HIVE);
        assert_eq!(
            view.entries[0].sequence, 201,
            "the oldest lines leave first"
        );
    }
}
