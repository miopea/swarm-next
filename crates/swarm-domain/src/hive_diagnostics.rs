//! A member Hive's recent diagnostic log, as it shares it with its Keeper.
//!
//! ADR 0112. Always shared, by operator decision on 2026-09-29: a member that
//! stopped reaching its Keeper, or refused a takeover's keystrokes, used to be
//! diagnosable only by its own operator reading its own machine, which is the
//! leg work the Keeper exists to take off them. What travels is Swarm's own log
//! lines — never terminal output, never typed input — bounded on both sides.

use serde::{Deserialize, Serialize};

/// The most lines one batch may carry.
pub const MAX_DIAGNOSTIC_BATCH_ENTRIES: usize = 200;
/// The longest message kept, in bytes; longer ones are cut at a character.
pub const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 1024;
/// The longest module path kept, in bytes.
pub const MAX_DIAGNOSTIC_TARGET_BYTES: usize = 128;
/// The longest identifier of one process's run, in bytes.
pub const MAX_DIAGNOSTIC_BOOT_ID_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// One log line. `sequence` counts up within one run of the process and starts
/// again when it restarts, which is what `boot_id` tells apart.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticEntry {
    pub sequence: u64,
    /// Milliseconds since the Unix epoch, on the member's clock.
    pub at_ms: i64,
    pub level: DiagnosticLevel,
    pub target: String,
    pub message: String,
}

/// What a member sends: the lines since the last batch the Keeper accepted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticBatch {
    pub boot_id: String,
    pub entries: Vec<DiagnosticEntry>,
    /// Lines that fell out of the member's buffer before they could be sent.
    /// Said rather than hidden, so a gap reads as a gap.
    pub dropped: u64,
}

impl DiagnosticBatch {
    /// Whether this batch is within the bounds both sides agree on.
    ///
    /// # Errors
    /// Names the first bound the batch breaks.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.boot_id.is_empty() || self.boot_id.len() > MAX_DIAGNOSTIC_BOOT_ID_BYTES {
            return Err("boot id is empty or too long");
        }
        if self.entries.len() > MAX_DIAGNOSTIC_BATCH_ENTRIES {
            return Err("too many lines in one batch");
        }
        if self.entries.iter().any(|entry| {
            entry.message.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES
                || entry.target.len() > MAX_DIAGNOSTIC_TARGET_BYTES
        }) {
            return Err("a line is too long");
        }
        if self
            .entries
            .windows(2)
            .any(|pair| pair[0].sequence >= pair[1].sequence)
        {
            return Err("lines are not in order");
        }
        Ok(())
    }
}

/// Cuts `text` to at most `limit` bytes without splitting a character.
#[must_use]
pub fn clip_diagnostic_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sequence: u64) -> DiagnosticEntry {
        DiagnosticEntry {
            sequence,
            at_ms: 1,
            level: DiagnosticLevel::Warn,
            target: "swarm_api".to_owned(),
            message: "the Keeper is temporarily unavailable".to_owned(),
        }
    }

    #[test]
    fn a_batch_within_bounds_is_accepted_and_each_bound_is_named() {
        let mut batch = DiagnosticBatch {
            boot_id: "boot".to_owned(),
            entries: vec![entry(1), entry(2)],
            dropped: 0,
        };
        assert_eq!(batch.validate(), Ok(()));

        batch.entries = vec![entry(2), entry(1)];
        assert_eq!(batch.validate(), Err("lines are not in order"));

        batch.entries = (1..=201).map(entry).collect();
        assert_eq!(batch.validate(), Err("too many lines in one batch"));

        batch.entries = vec![DiagnosticEntry {
            message: "x".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES + 1),
            ..entry(1)
        }];
        assert_eq!(batch.validate(), Err("a line is too long"));

        batch.entries = vec![entry(1)];
        batch.boot_id = String::new();
        assert_eq!(batch.validate(), Err("boot id is empty or too long"));
    }

    #[test]
    fn clipping_never_splits_a_character() {
        assert_eq!(clip_diagnostic_text("héllo", 2), "h");
        assert_eq!(clip_diagnostic_text("héllo", 3), "hé");
        assert_eq!(clip_diagnostic_text("short", 64), "short");
    }
}
