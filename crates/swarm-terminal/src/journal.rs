use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JournalLimits {
    pub max_bytes: usize,
    pub max_frames: usize,
}

impl JournalLimits {
    #[must_use]
    pub const fn new(max_bytes: usize, max_frames: usize) -> Self {
        Self {
            max_bytes,
            max_frames,
        }
    }
}

impl Default for JournalLimits {
    fn default() -> Self {
        Self::new(8 * 1024 * 1024, 16_384)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SequencedFrame {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum JournalResume {
    Deltas { frames: Vec<SequencedFrame> },
    SnapshotRequired { latest_sequence: u64 },
}

#[derive(Debug)]
pub struct BoundedJournal {
    limits: JournalLimits,
    frames: VecDeque<SequencedFrame>,
    retained_bytes: usize,
    next_sequence: u64,
}

impl BoundedJournal {
    #[must_use]
    pub fn new(limits: JournalLimits) -> Self {
        Self {
            limits,
            frames: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 1,
        }
    }

    pub fn push(&mut self, bytes: impl Into<Vec<u8>>) -> u64 {
        let bytes = bytes.into();
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if self.limits.max_bytes == 0
            || self.limits.max_frames == 0
            || bytes.len() > self.limits.max_bytes
        {
            self.clear_retained();
            return sequence;
        }
        self.retained_bytes += bytes.len();
        self.frames.push_back(SequencedFrame { sequence, bytes });
        self.enforce_limits();
        sequence
    }

    /// Advances the canonical sequence while invalidating every retained
    /// byte-only cursor. Callers use this when terminal state changes without
    /// corresponding PTY output, such as a committed resize.
    pub(crate) fn snapshot_boundary(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.clear_retained();
        sequence
    }

    #[must_use]
    pub(crate) fn resume_after(&self, sequence: u64) -> JournalResume {
        let latest_sequence = self.latest_sequence();
        if sequence >= latest_sequence {
            return JournalResume::Deltas { frames: Vec::new() };
        }
        let first_retained = self
            .frames
            .front()
            .map_or(self.next_sequence, |frame| frame.sequence);
        if sequence.saturating_add(1) < first_retained {
            return JournalResume::SnapshotRequired { latest_sequence };
        }
        JournalResume::Deltas {
            frames: self
                .frames
                .iter()
                .filter(|frame| frame.sequence > sequence)
                .cloned()
                .collect(),
        }
    }

    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    #[must_use]
    pub fn retained_frames(&self) -> usize {
        self.frames.len()
    }
    #[must_use]
    pub const fn latest_sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    fn enforce_limits(&mut self) {
        while self.retained_bytes > self.limits.max_bytes
            || self.frames.len() > self.limits.max_frames
        {
            if let Some(frame) = self.frames.pop_front() {
                self.retained_bytes -= frame.bytes.len();
            } else {
                break;
            }
        }
    }

    fn clear_retained(&mut self) {
        self.frames.clear();
        self.retained_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_old_frames_at_byte_limit() {
        let mut journal = BoundedJournal::new(JournalLimits::new(5, 10));
        journal.push(b"abc".to_vec());
        let latest = journal.push(b"def".to_vec());
        assert_eq!(journal.retained_bytes(), 3);
        assert_eq!(journal.retained_frames(), 1);
        assert_eq!(
            journal.resume_after(0),
            JournalResume::SnapshotRequired {
                latest_sequence: latest
            }
        );
    }

    #[test]
    fn resumes_with_exact_missing_deltas() {
        let mut journal = BoundedJournal::new(JournalLimits::new(32, 10));
        let first = journal.push(b"one".to_vec());
        journal.push(b"two".to_vec());
        journal.push(b"three".to_vec());
        let JournalResume::Deltas { frames } = journal.resume_after(first) else {
            panic!("expected retained deltas");
        };
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].bytes, b"two");
        assert_eq!(frames[1].bytes, b"three");
    }

    #[test]
    fn oversized_frame_does_not_break_the_bound() {
        let mut journal = BoundedJournal::new(JournalLimits::new(4, 10));
        let sequence = journal.push(vec![0; 5]);
        assert_eq!(journal.retained_bytes(), 0);
        assert_eq!(journal.retained_frames(), 0);
        assert_eq!(
            journal.resume_after(0),
            JournalResume::SnapshotRequired {
                latest_sequence: sequence
            }
        );
    }

    #[test]
    fn sustained_output_never_exceeds_configured_memory() {
        let mut journal = BoundedJournal::new(JournalLimits::new(8 * 1024, 32));
        for _ in 0..100_000 {
            journal.push(vec![0; 1024]);
            assert!(journal.retained_bytes() <= 8 * 1024);
            assert!(journal.retained_frames() <= 32);
        }
        assert_eq!(journal.retained_bytes(), 8 * 1024);
        assert_eq!(journal.retained_frames(), 8);
    }

    #[test]
    fn snapshot_boundary_invalidates_byte_only_cursors() {
        let mut journal = BoundedJournal::new(JournalLimits::new(32, 10));
        let cursor = journal.push(b"before-resize".to_vec());

        let boundary = journal.snapshot_boundary();

        assert_eq!(boundary, cursor + 1);
        assert_eq!(journal.retained_bytes(), 0);
        assert_eq!(journal.retained_frames(), 0);
        assert_eq!(
            journal.resume_after(cursor),
            JournalResume::SnapshotRequired {
                latest_sequence: boundary
            }
        );
    }
}
