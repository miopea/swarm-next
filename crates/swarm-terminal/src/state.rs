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
        let visible = screen.state_formatted();
        let mut bytes = formatted_scrollback(
            screen,
            MAX_CANONICAL_SNAPSHOT_BYTES.saturating_sub(visible.len()),
        );
        if screen.alternate_screen() {
            bytes.extend_from_slice(b"\x1b[?1049h");
        }
        bytes.extend(visible);
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CellStyle {
    foreground: vt100::Color,
    background: vt100::Color,
    modes: u8,
}

impl CellStyle {
    fn from_cell(cell: &vt100::Cell) -> Self {
        let modes = u8::from(cell.bold())
            | (u8::from(cell.dim()) << 1)
            | (u8::from(cell.italic()) << 2)
            | (u8::from(cell.underline()) << 3)
            | (u8::from(cell.inverse()) << 4);
        Self {
            foreground: cell.fgcolor(),
            background: cell.bgcolor(),
            modes,
        }
    }

    fn is_default(self) -> bool {
        self == Self {
            foreground: vt100::Color::Default,
            background: vt100::Color::Default,
            modes: 0,
        }
    }

    fn write_sgr(self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(b"\x1b[0");
        if self.modes & 1 != 0 {
            bytes.extend_from_slice(b";1");
        }
        if self.modes & (1 << 1) != 0 {
            bytes.extend_from_slice(b";2");
        }
        if self.modes & (1 << 2) != 0 {
            bytes.extend_from_slice(b";3");
        }
        if self.modes & (1 << 3) != 0 {
            bytes.extend_from_slice(b";4");
        }
        if self.modes & (1 << 4) != 0 {
            bytes.extend_from_slice(b";7");
        }
        write_color_sgr(bytes, self.foreground, true);
        write_color_sgr(bytes, self.background, false);
        bytes.push(b'm');
    }
}

fn write_color_sgr(bytes: &mut Vec<u8>, color: vt100::Color, foreground: bool) {
    match color {
        vt100::Color::Default => {}
        vt100::Color::Idx(index @ 0..=7) => {
            bytes.extend_from_slice(
                format!(";{}", if foreground { 30 + index } else { 40 + index }).as_bytes(),
            );
        }
        vt100::Color::Idx(index @ 8..=15) => {
            bytes.extend_from_slice(
                format!(";{}", if foreground { 82 + index } else { 92 + index }).as_bytes(),
            );
        }
        vt100::Color::Idx(index) => {
            bytes.extend_from_slice(
                format!(";{};5;{index}", if foreground { 38 } else { 48 }).as_bytes(),
            );
        }
        vt100::Color::Rgb(red, green, blue) => {
            bytes.extend_from_slice(
                format!(
                    ";{};2;{red};{green};{blue}",
                    if foreground { 38 } else { 48 }
                )
                .as_bytes(),
            );
        }
    }
}

/// Renders a canonical snapshot back to the plain text a person sees.
///
/// A snapshot is a terminal stream, so text in it is not searchable as text.
/// Providers position with cursor moves rather than spaces — Claude draws its
/// collapsed paste chip as `[Pasted`, cursor forward, `text`, cursor forward,
/// `#1]` — so a phrase that is plainly on screen may exist nowhere in the
/// bytes. Anything matching against what the operator can see has to replay the
/// stream first, which is what a terminal is for.
///
/// Scrollback is included, so text that has scrolled above the visible rows is
/// still found.
#[must_use]
pub fn snapshot_plain_text(bytes: &[u8], rows: u16, columns: u16) -> String {
    let mut parser = vt100::Parser::new(rows.max(1), columns.max(1), CANONICAL_SCROLLBACK_ROWS);
    parser.process(bytes);
    let mut screen = parser.screen().clone();
    screen.set_scrollback(usize::MAX);
    let retained = screen.scrollback();
    let mut rendered = String::new();
    for offset in (1..=retained).rev() {
        screen.set_scrollback(offset);
        rendered.push_str(&screen.contents());
        rendered.push('\n');
    }
    screen.set_scrollback(0);
    rendered.push_str(&screen.contents());
    rendered
}

fn formatted_scrollback(screen: &vt100::Screen, max_bytes: usize) -> Vec<u8> {
    let mut history = screen.clone();
    history.set_scrollback(usize::MAX);
    let retained_rows = history.scrollback();
    let (visible_rows, columns) = history.size();
    let structural_bytes = 7usize
        .saturating_add(usize::from(visible_rows.saturating_sub(1)).saturating_mul(2))
        .saturating_add(20);
    if retained_rows == 0 || max_bytes <= structural_bytes {
        return Vec::new();
    }

    let row_budget = max_bytes - structural_bytes;
    let mut rows = Vec::with_capacity(retained_rows);
    let mut retained_bytes = 0usize;
    for offset in (1..=retained_rows).rev() {
        history.set_scrollback(offset);
        let mut row = formatted_history_row(&history, columns);
        row.extend_from_slice(b"\x1b[0m\r\n");
        retained_bytes = retained_bytes.saturating_add(row.len());
        rows.push(row);
        while retained_bytes > row_budget {
            let removed = rows.remove(0);
            retained_bytes = retained_bytes.saturating_sub(removed.len());
            if rows.is_empty() {
                break;
            }
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }

    let mut bytes = Vec::with_capacity(retained_bytes.saturating_add(32));
    bytes.extend_from_slice(b"\x1b[?7l");
    for row in rows {
        bytes.extend(row);
    }
    for _ in 0..visible_rows.saturating_sub(1) {
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"\x1b[?7h\x1b[0m\x1b[2J\x1b[H");
    bytes
}

fn formatted_history_row(screen: &vt100::Screen, columns: u16) -> Vec<u8> {
    let last_meaningful = (0..columns).rev().find(|column| {
        screen
            .cell(0, *column)
            .is_some_and(|cell| cell.has_contents() || !CellStyle::from_cell(cell).is_default())
    });
    let Some(last_meaningful) = last_meaningful else {
        return Vec::new();
    };

    let mut bytes = Vec::new();
    let mut previous_style = CellStyle {
        foreground: vt100::Color::Default,
        background: vt100::Color::Default,
        modes: 0,
    };
    for column in 0..=last_meaningful {
        let Some(cell) = screen.cell(0, column) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let style = CellStyle::from_cell(cell);
        if style != previous_style {
            style.write_sgr(&mut bytes);
            previous_style = style;
        }
        if cell.has_contents() {
            bytes.extend_from_slice(cell.contents().as_bytes());
        } else {
            bytes.push(b' ');
        }
    }
    bytes
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
    fn fresh_attachment_preserves_bounded_colored_scrollback() {
        let mut state =
            CanonicalTerminalState::new(JournalLimits::new(4096, 64), TerminalSize::new(3, 20));
        state.push(
            b"first\r\n\x1b[38;2;91;143;211msecond\x1b[m\r\nthird\r\nfourth\r\nfifth".to_vec(),
        );

        let snapshot = state.snapshot();
        let mut restored =
            vt100::Parser::new(snapshot.rows, snapshot.columns, CANONICAL_SCROLLBACK_ROWS);
        restored.process(&snapshot.bytes);
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );

        let mut restored_history = restored.screen().clone();
        restored_history.set_scrollback(usize::MAX);
        assert!(restored_history.scrollback() >= 2);
        assert!(restored_history.contents().contains("first"));
        assert_eq!(
            restored_history.cell(1, 0).unwrap().fgcolor(),
            vt100::Color::Rgb(91, 143, 211),
        );
    }

    #[test]
    fn scrollback_snapshot_stays_within_the_canonical_memory_bound() {
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(8 * 1024 * 1024, 16_384),
            TerminalSize::new(4, 200),
        );
        for row in 0..2_000 {
            state.push(
                format!(
                    "\x1b[38;2;91;143;211m{row:04} {}\x1b[m\r\n",
                    "x".repeat(190)
                )
                .into_bytes(),
            );
        }

        let snapshot = state.snapshot();
        assert!(snapshot.bytes.len() <= MAX_CANONICAL_SNAPSHOT_BYTES);
        let mut restored =
            vt100::Parser::new(snapshot.rows, snapshot.columns, CANONICAL_SCROLLBACK_ROWS);
        restored.process(&snapshot.bytes);
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
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
    fn wrapped_question_repaints_converge_after_snapshot_recovery() {
        // Synthetic relative-cursor repaint, not a captured provider trace.
        // Unlike a clear-screen fixture, the next question must erase the old
        // physical lines, including wrapped descriptions and a short viewport.
        for columns in [36_u16, 100] {
            for rows in [4_u16, 24] {
                let mut state = CanonicalTerminalState::new(
                    JournalLimits::new(16 * 1024, 128),
                    TerminalSize::new(rows, columns),
                );
                let mut prior_lines = 0;
                for (index, marker) in ["APPLE", "CLOVER", "HONEY"].iter().enumerate() {
                    let snapshot = state.snapshot();
                    let mut restored = vt100::Parser::new(rows, columns, CANONICAL_SCROLLBACK_ROWS);
                    restored.process(&snapshot.bytes);
                    let mut repaint = if prior_lines > 0 {
                        format!("\r\x1b[{}A\x1b[J", prior_lines - 1)
                    } else {
                        String::new()
                    };
                    let lines = [
                        format!("Question {}: {marker}", index + 1),
                        "A long explanation that wraps across several physical rows on a narrow phone terminal while remaining one logical line.".to_owned(),
                        "  1. Continue the existing task".to_owned(),
                        "  2. Leave the worker waiting".to_owned(),
                        format!("Choose {marker}:"),
                    ];
                    prior_lines = lines
                        .iter()
                        .map(|line| line.len().div_ceil(usize::from(columns)))
                        .sum::<usize>();
                    repaint.push_str(&lines.join("\r\n"));
                    // Deliberately split CSI and text across transport frames.
                    for chunk in repaint.as_bytes().chunks(7) {
                        state.push(chunk.to_vec());
                        restored.process(chunk);
                    }
                    assert_eq!(
                        restored.screen().contents(),
                        state.parser.screen().contents(),
                        "question {} at {columns}x{rows}",
                        index + 1,
                    );
                    assert_eq!(
                        restored.screen().cursor_position(),
                        state.parser.screen().cursor_position()
                    );
                    assert!(restored.screen().contents().contains(marker));
                }
            }
        }
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
