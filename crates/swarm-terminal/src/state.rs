use serde::{Deserialize, Serialize};

use crate::journal::{BoundedJournal, JournalResume};
use crate::{JournalLimits, SequencedFrame, TerminalSize};

pub const CANONICAL_SCROLLBACK_ROWS: usize = 1_000;
pub const CANONICAL_COMPACTION_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_CANONICAL_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
const TRUNCATED_STATE_NOTICE: &[u8] =
    b"\r\n[Swarm reset an oversized terminal view to preserve memory safety.]\r\n";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    pub sequence: u64,
    pub rows: u16,
    pub columns: u16,
    pub truncated: bool,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resume {
    Deltas { frames: Vec<SequencedFrame> },
    Snapshot { snapshot: TerminalSnapshot },
}

pub struct CanonicalTerminalState {
    journal: BoundedJournal,
    parser: vt100::Parser,
    bytes_since_compaction: usize,
    state_truncated: bool,
}

impl CanonicalTerminalState {
    #[must_use]
    pub fn new(limits: JournalLimits, size: TerminalSize) -> Self {
        Self {
            journal: BoundedJournal::new(limits),
            parser: vt100::Parser::new(size.rows, size.columns, CANONICAL_SCROLLBACK_ROWS),
            bytes_since_compaction: 0,
            state_truncated: false,
        }
    }

    pub fn push(&mut self, bytes: impl Into<Vec<u8>>) -> u64 {
        let bytes = bytes.into();
        let sequence = self.journal.push(bytes.clone());
        self.parser.process(&bytes);
        self.bytes_since_compaction = self.bytes_since_compaction.saturating_add(bytes.len());
        if self.bytes_since_compaction >= CANONICAL_COMPACTION_INPUT_BYTES {
            self.compact();
        }
        sequence
    }

    #[must_use]
    pub fn resume_after(&self, sequence: Option<u64>) -> Resume {
        let Some(sequence) = sequence else {
            return Resume::Snapshot {
                snapshot: self.snapshot(),
            };
        };
        match self.journal.resume_after(sequence) {
            JournalResume::SnapshotRequired { .. } => Resume::Snapshot {
                snapshot: self.snapshot(),
            },
            JournalResume::Deltas { frames } => Resume::Deltas { frames },
        }
    }

    #[must_use]
    pub fn size(&self) -> TerminalSize {
        let (rows, columns) = self.parser.screen().size();
        TerminalSize::new(rows, columns)
    }

    /// Commits dimensions and creates a sequenced snapshot boundary.
    ///
    /// Byte deltas cannot describe a resize to an already attached renderer,
    /// so every client cursor before this boundary must receive a canonical
    /// snapshot. Identical dimensions are a no-op to prevent resize echoes
    /// from creating synchronization loops.
    pub fn resize(&mut self, size: TerminalSize) -> bool {
        if self.size() == size {
            return false;
        }
        self.parser.screen_mut().set_size(size.rows, size.columns);
        self.journal.snapshot_boundary();
        true
    }

    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        let screen = self.parser.screen();
        let (rows, columns) = screen.size();
        let mut bytes = Vec::new();
        if screen.alternate_screen() {
            bytes.extend_from_slice(b"\x1b[?1049h");
        }
        bytes.extend(screen.state_formatted());
        TerminalSnapshot {
            sequence: self.journal.latest_sequence(),
            rows,
            columns,
            truncated: self.state_truncated,
            bytes,
        }
    }

    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.journal.retained_bytes()
    }

    #[must_use]
    pub fn retained_frames(&self) -> usize {
        self.journal.retained_frames()
    }

    fn compact(&mut self) {
        self.compact_to_limit(MAX_CANONICAL_SNAPSHOT_BYTES);
    }

    fn compact_to_limit(&mut self, max_snapshot_bytes: usize) {
        let snapshot = self.snapshot();
        let mut parser =
            vt100::Parser::new(snapshot.rows, snapshot.columns, CANONICAL_SCROLLBACK_ROWS);
        if snapshot.bytes.len() <= max_snapshot_bytes {
            parser.process(&snapshot.bytes);
        } else {
            parser.process(TRUNCATED_STATE_NOTICE);
            self.state_truncated = true;
        }
        self.parser = parser;
        self.bytes_since_compaction = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_attachment_receives_canonical_state_at_an_exact_sequence() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(128, 8), TerminalSize::new(4, 20));
        state.push(b"first\r\n".to_vec());
        let sequence = state.push(b"\x1b[31mred\x1b[m".to_vec());

        let Resume::Snapshot { snapshot } = state.resume_after(None) else {
            panic!("fresh attachment must receive a snapshot");
        };
        assert_eq!(snapshot.sequence, sequence);
        assert_eq!((snapshot.rows, snapshot.columns), (4, 20));
        assert!(!snapshot.truncated);

        let mut restored = vt100::Parser::new(4, 20, 0);
        restored.process(&snapshot.bytes);
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );
        assert_eq!(
            restored.screen().cell(1, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
    }

    #[test]
    fn canonical_snapshots_preserve_indexed_and_truecolor_attributes() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(512, 16), TerminalSize::new(4, 40));
        state.push(b"\x1b[31mred \x1b[38;5;45mindexed \x1b[38;2;91;143;211mrgb".to_vec());
        let snapshot = state.snapshot();
        let mut restored = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
        restored.process(&snapshot.bytes);

        assert_eq!(
            restored.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        assert_eq!(
            restored.screen().cell(0, 4).unwrap().fgcolor(),
            vt100::Color::Idx(45)
        );
        assert_eq!(
            restored.screen().cell(0, 12).unwrap().fgcolor(),
            vt100::Color::Rgb(91, 143, 211),
        );
    }

    #[test]
    fn evicted_cursor_receives_snapshot_while_covered_cursor_receives_deltas() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(6, 2), TerminalSize::new(4, 20));
        state.push(b"one".to_vec());
        let second = state.push(b"two".to_vec());
        let latest = state.push(b"three".to_vec());

        assert!(matches!(
            state.resume_after(Some(1)),
            Resume::Snapshot { .. }
        ));
        let Resume::Deltas { frames } = state.resume_after(Some(second)) else {
            panic!("covered cursor must receive deltas");
        };
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sequence, latest);
    }

    #[test]
    fn resize_is_part_of_the_canonical_snapshot() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(128, 8), TerminalSize::new(24, 80));
        let cursor = state.push(b"before-resize".to_vec());

        assert!(state.resize(TerminalSize::new(40, 120)));
        let Resume::Snapshot { snapshot } = state.resume_after(Some(cursor)) else {
            panic!("a pre-resize cursor must receive canonical dimensions");
        };
        assert_eq!(snapshot.sequence, cursor + 1);
        assert_eq!((snapshot.rows, snapshot.columns), (40, 120));

        assert!(!state.resize(TerminalSize::new(40, 120)));
        assert!(matches!(
            state.resume_after(Some(snapshot.sequence)),
            Resume::Deltas { frames } if frames.is_empty()
        ));
    }

    #[test]
    fn snapshot_plus_deltas_converges_with_uninterrupted_parsing() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(256, 16), TerminalSize::new(6, 40));
        state.push(b"first\r\n\x1b[34mblue".to_vec());
        let snapshot = state.snapshot();
        state.push(b"\x1b[m\r\nlast".to_vec());
        let Resume::Deltas { frames } = state.resume_after(Some(snapshot.sequence)) else {
            panic!("snapshot cursor must remain covered");
        };

        let mut restored = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
        restored.process(&snapshot.bytes);
        for frame in frames {
            restored.process(&frame.bytes);
        }
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );
        assert_eq!(
            restored.screen().state_formatted(),
            state.parser.screen().state_formatted()
        );
    }

    #[test]
    fn alternate_screen_is_reconstructed() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(256, 16), TerminalSize::new(6, 40));
        state.push(b"main\x1b[?1049halternate".to_vec());
        let snapshot = state.snapshot();
        let mut restored = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
        restored.process(&snapshot.bytes);

        assert!(state.parser.screen().alternate_screen());
        assert!(restored.screen().alternate_screen());
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );
    }

    #[test]
    fn pathological_cell_growth_compacts_to_a_bounded_visible_state() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(8 * 1024, 32), TerminalSize::new(4, 20));
        let combining_mark = "\u{0301}".repeat(CANONICAL_COMPACTION_INPUT_BYTES);
        state.push(format!("a{combining_mark}").into_bytes());

        let snapshot = state.snapshot();
        assert!(snapshot.bytes.len() <= MAX_CANONICAL_SNAPSHOT_BYTES);
        assert_eq!(state.bytes_since_compaction, 0);
    }

    #[test]
    fn oversized_snapshot_fallback_is_visible_and_bounded() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(128, 8), TerminalSize::new(4, 20));
        state.push(b"content".to_vec());
        state.compact_to_limit(0);

        let snapshot = state.snapshot();
        assert!(snapshot.truncated);
        assert!(String::from_utf8_lossy(&snapshot.bytes).contains("preserve memory safety"));
    }
}
