use serde::{Deserialize, Serialize};
use swarm_domain::ProviderKind;

use crate::TerminalSnapshot;

/// Provider activity derived from the canonical visible terminal surface.
///
/// This is deliberately separate from operator attention. The API combines this
/// provider-owned evidence with durable engagement and decision state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderActivity {
    Active,
    Resting,
    AwaitingOperator,
    Unknown,
}

#[must_use]
pub fn classify_provider_activity(
    provider: ProviderKind,
    snapshot: &TerminalSnapshot,
) -> ProviderActivity {
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    classify_visible_text(provider, &parser.screen().contents())
}

fn classify_visible_text(provider: ProviderKind, visible: &str) -> ProviderActivity {
    // A provider this build cannot launch has no screen worth reading, and
    // guessing from another provider's glyphs would be worse than admitting it.
    // Unknown is the honest answer and it already exists for exactly this.
    if provider == ProviderKind::Unsupported {
        return ProviderActivity::Unknown;
    }
    let normalized = visible.to_lowercase();

    // ⚠️ THE LIVE MENU IS CHECKED BEFORE active_signal, and the order is the
    // whole fix. Operator, 2026-09-12: "Rcg network was on a askuser prompt and
    // says buzzing. Still state detection problems on Mobile."
    //
    // A worker sitting on an AskUserQuestion read as ACTIVE, because a footer
    // still carrying an interrupt hint matched first and returned before the
    // menu was ever looked at. Buzzing is what the roster shows for Active, so
    // a worker waiting on the operator advertised itself as busy.
    //
    // ⚠️ AND WHY THIS ONE IS ANCHORED AT THE START OF THE LINE. A narrow
    // terminal truncates the END. Measured across this Hive's own journals, the
    // menu footer really does arrive cut:
    //     "Enter to select · Tab/Arrow keys to navigate · Esc to cancel"  26
    //     "Enter to select · Tab/Arrow keys to navigate · "                3
    //     "Enter to select · ↑/↓ to navigate · n to add "                  3
    //     "Enter to select ·↑/↓ to navigate · ctrl+g to "                  2
    // Every existing Claude branch keyed on "Esc to cancel", which is the part
    // truncation removes first, so on a phone they matched nothing. "Enter to
    // select" is at the start and survives all four.
    if provider == ProviderKind::ClaudeCode && claude_menu_is_open(visible) {
        return ProviderActivity::AwaitingOperator;
    }

    if active_signal(&normalized) {
        return ProviderActivity::Active;
    }
    if provider == ProviderKind::ClaudeCode
        && normalized.contains("esc to cancel")
        && (normalized.contains("enter to confirm")
            || visible
                .lines()
                .any(|line| is_choice_cursor('❯', line.trim())))
    {
        return ProviderActivity::AwaitingOperator;
    }

    let recent = visible
        .lines()
        .rev()
        .filter_map(|line| {
            let line = line.trim();
            (!line.is_empty()).then_some(line)
        })
        .take(10)
        .collect::<Vec<_>>();

    // A long Claude AskUser menu can scroll its selected first option off
    // screen. Require its current navigation footer and both numbered escape
    // options, rather than inventing a selected cursor or treating it as idle.
    // A returned composer below historical menu text does not satisfy this.
    if provider == ProviderKind::ClaudeCode
        && recent.first().is_some_and(|line| {
            line.starts_with("Enter to select")
                && line.contains("to navigate")
                && line.ends_with("Esc to cancel")
        })
        && recent
            .iter()
            .any(|line| is_numbered_choice(line) && line.ends_with("Type something."))
        && recent
            .iter()
            .any(|line| is_numbered_choice(line) && line.ends_with("Chat about this"))
    {
        return ProviderActivity::AwaitingOperator;
    }

    // A MENU, NOT A FOOTER. Codex renders at least three different prompts —
    // a command approval, an edit approval and an update notice — and their
    // wording does not agree: two end "Press enter to confirm or esc to
    // cancel" and the third ends "Press enter to continue". Keying on that
    // sentence would have matched the first two and silently missed the third.
    //
    // What every one of them shares is the SHAPE: a cursored numbered option
    // with siblings. A resting composer has the same leading glyph and no
    // numbered options at all, which is what keeps this from swallowing it.
    if let Some(cursor) = choice_cursor(provider)
        && recent.iter().any(|line| is_choice_cursor(cursor, line))
        && recent
            .iter()
            .filter(|line| is_numbered_choice(line))
            .count()
            >= 2
    {
        return ProviderActivity::AwaitingOperator;
    }

    // ⚠️ THE SPINNER LINE, BECAUSE THE FOOTER HINT CAN BE TRUNCATED AWAY. With
    // auto mode on, Claude's footer grows to "⏵⏵ auto mode on (shift+tab to
    // cycle) · esc to interrupt", and on an ordinary terminal width the right
    // edge cuts it to "· esc …" — before the "to" that `active_signal` needs.
    // Every worker running in auto mode therefore read as RESTING while it was
    // working. Operator, 2026-09-22, photographing the roster: "you're obviously
    // working and so is Scout. Yet what we're seeing is no state updates or
    // reflecting reality." Read off this Hive's own live screen at the moment
    // the roster said Resting: "✽ Baking… (8m 38s · ↓ 23.5k tokens)" above
    // "⏵⏵ auto mode on (shift+tab to cycle) · esc …".
    //
    // The spinner line is anchored at the LEFT, so the truncation that ate the
    // footer cannot remove the part that identifies it. Checked AFTER the menus,
    // so an open question still reads as waiting on the operator, and BEFORE
    // the idle prompt, because Claude keeps its input box on screen while it
    // works.
    //
    // ⚠️ AND THE FOOTER, BECAUSE THE SPINNER IS NOT ALWAYS ON SCREEN. The live
    // screen that proved the first version of this fix insufficient (a 48-column
    // phone, 2026-09-22) had no spinner in view at all: a menu left over from a
    // `/model` filled the top and the operator's queued message filled the
    // bottom. The one busy signal still showing was the footer, "⏵⏵ auto mode on
    // (shift+tab to cycle) · esc …". Measured across two days of this Hive's
    // terminal history, the segment after "cycle) ·" reads "esc to interrupt"
    // 14,687 times, "esc…" 8,050 and "esc …" 4,051 when working, and "← for
    // agents" or a truncation of it when not. No idle form begins with "esc".
    if provider == ProviderKind::ClaudeCode
        && (below_input_box(&recent).any(claude_footer_says_interruptible)
            || claude_spinner_on_screen(visible))
    {
        return ProviderActivity::Active;
    }

    if recent.iter().any(|line| idle_prompt(provider, line))
        || recent.iter().any(|line| idle_footer(provider, line))
    {
        // A resting prompt outranks a background shell. The turn has ended;
        // something it started has not.
        return ProviderActivity::Resting;
    }

    if background_work_signal(provider, &normalized) {
        return ProviderActivity::Active;
    }

    ProviderActivity::Unknown
}

/// Whether something the worker started is still running after its turn ended.
///
/// The classifier deliberately lets a resting prompt outrank a background
/// shell — the turn HAS ended, and treating the worker as busy stalled the
/// whole Hive. But that collapses two situations an operator needs to tell
/// apart: a worker with nothing to do, and one that finished while a `gh run
/// watch` it started is still going. Both read "Resting", and the operator
/// reported exactly that confusion.
///
/// Reported separately rather than as another activity, so nothing that routes
/// work on activity has to learn a new case.
#[must_use]
pub fn background_work_running(provider: ProviderKind, snapshot: &TerminalSnapshot) -> bool {
    let mut parser = vt100::Parser::new(snapshot.rows, snapshot.columns, 0);
    parser.process(&snapshot.bytes);
    background_work_signal(provider, &parser.screen().contents().to_lowercase())
}

/// The provider is working on the operator's turn and must not be interrupted.
/// Whether a Claude choice menu is currently open and waiting.
///
/// The footer must be the BOTTOM-MOST content, which is what separates a live
/// menu from an answered one still scrolled up the screen: once answered,
/// Claude draws its composer below and this line is no longer last.
fn claude_menu_is_open(visible: &str) -> bool {
    let mut lines = visible
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let footer_is_last = lines
        .next()
        .is_some_and(|line| line.starts_with("Enter to select") && line.contains("to navigate"));
    // ⚠️ A FOOTER ALONE IS NOT A MENU, and an existing test says so — the
    // options have to be there too. Numbered options are start-anchored like
    // the footer, so they survive the same truncation; the selected option and
    // its cursor may be scrolled off a short screen, which is why one numbered
    // sibling is enough and a cursor is not required.
    footer_is_last && lines.take(12).any(is_numbered_choice)
}

/// ⚠️ A PREFIX, because a narrow terminal truncates the END of the line.
///
/// Measured across this Hive's journals: "esc to interrupt" 4486, but also
/// "esc to interrup…" 104, "esc to interru…" 35, "esc to inter" 18,
/// "esc to interr…" 16, "esc to inte" 13, "esc to int" 8, "esc to inter…" 6,
/// "esc to int…" 5, "esc to interrup" 2. The old list matched the full phrase
/// and exactly one truncation, so 202 observations of a WORKING provider read
/// as not working — the same phone-width truncation, failing the other way.
fn active_signal(normalized: &str) -> bool {
    normalized.contains("esc to int")
        || normalized.contains("esc to sto")
        || normalized.contains("esc to …")
}

/// The rows beneath the input box's lower border, bottom first.
///
/// ⚠️ THIS IS WHERE THE FOOTER LIVES, AND WHY IT IS FOUND STRUCTURALLY. The
/// first version read only the bottom line, and a live 48-column screen had
/// "0% until auto-compact" UNDER the footer — so a working worker at phone width
/// would have been read from the wrong line. Everything Claude prints into the
/// transcript, including a quoted footer, sits ABOVE the input box; what is
/// below its last border is Claude's own chrome. Stopping at that border is what
/// keeps a worker that discusses the busy footer from being called busy.
fn below_input_box<'a>(recent: &'a [&'a str]) -> impl Iterator<Item = &'a str> {
    recent
        .iter()
        .copied()
        .take_while(|line| !line.starts_with('─'))
}

/// Whether a line of Claude's footer carries the interrupt hint, however cut.
fn claude_footer_says_interruptible(line: &str) -> bool {
    let Some((_, after)) = line.split_once("cycle)") else {
        return false;
    };
    let Some(hint) = after.trim_start().strip_prefix('·') else {
        return false;
    };
    hint.trim_start().to_lowercase().starts_with("esc")
}

/// Whether a spinner line is anywhere in the lower part of the screen.
///
/// ⚠️ AT COLUMN ZERO ONLY. Claude draws its spinner flush left; everything it
/// prints into the transcript — its own prose, quoted text, tool output — is
/// indented under a bullet or a gutter. Trimming leading space before looking
/// would let a transcript that QUOTES a spinner line read as one.
fn claude_spinner_on_screen(visible: &str) -> bool {
    visible
        .lines()
        .rev()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .take(24)
        .any(claude_spinner_is_running)
}

/// Claude's own working indicator: a spinner glyph, a verb in progress, then
/// the parenthesised elapsed time — "✽ Baking… (8m 38s · ↓ 23.5k tokens)".
///
/// ⚠️ THE ELLIPSIS AND THE PARENTHESIS ARE BOTH REQUIRED, and each rules out a
/// real line that is not work. A finished turn leaves "✻ Cogitated for 1s" or
/// "✻ Brewed for 3s · done 11:33 AM" — a past-tense verb with no ellipsis. Tool
/// progress inside the transcript looks like "⎿  Waiting…" — the right verb
/// shape, but under a different glyph and with no elapsed time after it. A
/// match on either would pin a finished worker Active, which the roster shows
/// as Buzzing and which defers every delivery to it.
///
/// The glyph set is Claude's spinner frames. A line that merely begins with
/// ordinary punctuation is not one.
fn claude_spinner_is_running(line: &str) -> bool {
    const SPINNER_FRAMES: [char; 7] = ['·', '✢', '✳', '✶', '✻', '✽', '*'];
    let mut characters = line.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !SPINNER_FRAMES.contains(&first) {
        return false;
    }
    let rest = characters.as_str().trim_start();
    let Some((verb, after)) = rest.split_once('…') else {
        return false;
    };
    !verb.is_empty() && verb.chars().all(char::is_alphabetic) && after.trim_start().starts_with('(')
}

/// Something the provider started is still running in the background.
///
/// Not the same as the provider being busy, and treating it as the same was
/// costly: a `gh run watch` left behind by a finished turn kept Queen reading
/// as active for as long as the run took. She showed as buzzing beside an idle
/// terminal, and every delivery to her was deferred the whole time.
///
/// It still counts as active when nothing says the prompt is resting, because
/// then there is no evidence the turn has ended.
fn background_work_signal(provider: ProviderKind, normalized: &str) -> bool {
    match provider {
        ProviderKind::ClaudeCode => {
            normalized.contains(" monitor still running")
                || normalized.contains(" monitors still running")
                || normalized.contains(" shell still running")
                || normalized.contains(" shells still running")
                || normalized.contains("running dynamic workflow")
                || normalized.contains(" background dynamic workflow")
                || normalized.contains(" remote dynamic workflow")
        }
        // Codex prints no background-work banner, and an unrecognised provider
        // emits nothing this build knows how to read. Unsupported is also
        // unreachable here via the early return in classify_visible_text, but
        // the arm is required for exhaustiveness.
        //
        // THE ALPHA THREE ARE UNCLASSIFIED ON PURPOSE, NOT UNFINISHED. None of
        // gemini, grok or opencode is installed on this machine, so every glyph
        // written here would be a guess at a TUI nobody has watched redraw. The
        // file already argues the case in classify_visible_text: guessing from
        // another provider's glyphs is worse than admitting ignorance.
        //
        // Falling through costs less than being wrong. With all three of these
        // functions false, classify_visible_text returns Unknown -- and Unknown
        // is handled correctly end to end: delivery treats it as deliverable and
        // the attention flags surface it, both settled deliberately in c53b5ab.
        // A WRONG arm has no such safety net: it pins a worker Active forever
        // and defers every delivery to it, invisibly.
        //
        // The generic active_signal above is provider-independent and still
        // catches the busy case for these three, because "esc to interrupt" and
        // "esc to stop" are near-universal among agent TUIs. So what is lost is
        // the resting/idle half, not the whole classification. That gap IS what
        // the alpha label in the UI is pointing at.
        //
        // To finish one: install its CLI, capture a real snapshot at rest and
        // mid-turn, and add the arms with a test built from that capture rather
        // than from documentation.
        ProviderKind::Codex
        | ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => false,
    }
}

fn idle_prompt(provider: ProviderKind, line: &str) -> bool {
    match provider {
        ProviderKind::ClaudeCode => line == "❯" || line.starts_with("❯ "),
        ProviderKind::Codex => line == "›" || line.starts_with("› "),
        // Unclassified rather than guessed -- see background_work_signal.
        ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => false,
    }
}

fn idle_footer(provider: ProviderKind, line: &str) -> bool {
    let line = line.to_lowercase();
    match provider {
        ProviderKind::ClaudeCode => {
            line.contains("? for shortcuts")
                || line.contains("← for agents")
                || line.contains("auto mode on")
                || line.contains("manual mode on")
        }
        ProviderKind::Codex => line.contains("? for shortcuts"),
        // Unclassified rather than guessed -- see background_work_signal.
        // "? for shortcuts" is tempting because two providers print it, and
        // that is exactly the reasoning that would pin a third one wrong.
        ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => false,
    }
}

/// The glyph a provider draws against the SELECTED item of a choice menu.
///
/// Returned per provider rather than matched globally because it is also the
/// composer prompt for at least one of them: Codex draws `›` both in front of
/// the input line and in front of the highlighted option. Only the second is a
/// question, and telling them apart is what [`is_choice_cursor`] is for.
///
/// `None` for the alpha providers because nobody has watched one draw a menu.
/// Their CLIs are not installed on this machine, so a glyph here would be a
/// guess wearing the same shape as a measurement.
fn choice_cursor(provider: ProviderKind) -> Option<char> {
    match provider {
        ProviderKind::ClaudeCode => Some('❯'),
        ProviderKind::Codex => Some('›'),
        ProviderKind::Gemini
        | ProviderKind::Grok
        | ProviderKind::OpenCode
        | ProviderKind::Unsupported => None,
    }
}

fn is_choice_cursor(cursor: char, line: &str) -> bool {
    line.strip_prefix(cursor)
        .is_some_and(|tail| is_numbered_choice(tail.trim_start()))
}

fn is_numbered_choice(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && line[digits..].starts_with('.')
}

#[cfg(test)]
mod tests {

    /// Operator, 2026-09-12: "Rcg network was on a askuser prompt and says
    /// buzzing. Still state detection problems on Mobile."
    ///
    /// A worker WAITING ON THE OPERATOR advertised itself as working, because a
    /// footer still carrying an interrupt hint matched `active_signal` and
    /// returned before the menu was ever considered. Buzzing is the roster's
    /// rendering of Active, so the one state that should shout for attention
    /// looked like the one that wants none.
    #[test]
    fn an_open_menu_beats_a_leftover_interrupt_hint() {
        let menu = concat!(
            "Do you want to proceed?\r\n",
            "❯ 1. Yes\r\n",
            "  2. No\r\n\r\n",
            "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(menu)),
            ProviderActivity::AwaitingOperator,
        );
        // The reported screen: the same menu with an interrupt hint still on it.
        let with_hint = format!("esc to interrupt\r\n{menu}");
        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(&with_hint)),
            ProviderActivity::AwaitingOperator,
            "a live menu is what the worker is doing; the stale hint is not"
        );
    }

    /// ⚠️ THE MOBILE HALF, and the reason the operator sees this on a phone and
    /// not on a desktop. A narrow terminal truncates the END of a line, so every
    /// check keyed on "Esc to cancel" — the part that goes first — matched
    /// nothing. These four footers are REAL, taken from this Hive's journals.
    #[test]
    fn a_truncated_menu_footer_is_still_a_menu() {
        for footer in [
            "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
            "Enter to select · Tab/Arrow keys to navigate · ",
            "Enter to select · ↑/↓ to navigate · n to add ",
            "Enter to select ·↑/↓ to navigate · ctrl+g to ",
        ] {
            let screen = format!("Pick one:\r\n❯ 1. First\r\n  2. Second\r\n\r\n{footer}");
            assert_eq!(
                classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(&screen)),
                ProviderActivity::AwaitingOperator,
                "a phone-width footer is the same menu: {footer:?}"
            );
        }
    }

    /// The same truncation, failing the other way: a provider that IS working
    /// read as not working, because `active_signal` knew one truncated spelling
    /// out of ten. All of these are real observations from this Hive.
    #[test]
    fn a_truncated_interrupt_hint_still_means_working() {
        for hint in [
            "esc to interrupt",
            "esc to interrup…",
            "esc to interru…",
            "esc to interr…",
            "esc to inter…",
            "esc to int…",
            "esc to inter",
            "esc to inte",
            "esc to int",
        ] {
            assert_eq!(
                classify_provider_activity(
                    ProviderKind::ClaudeCode,
                    &snapshot(&format!("Working on it\r\n{hint}"))
                ),
                ProviderActivity::Active,
                "a working provider must not read as idle because its footer was cut: {hint:?}"
            );
        }
    }
    use super::*;

    /// An alpha provider reads Unknown, and Unknown is the safe answer.
    ///
    /// This is the assertion that keeps the "do not guess glyphs" decision
    /// honest. The danger of leaving the arms unwritten would be a worker pinned
    /// ACTIVE forever, because that defers every delivery to it invisibly. The
    /// classifier falls through to Unknown instead, which delivery and the
    /// attention flags both handle.
    #[test]
    fn an_alpha_provider_reads_unknown_rather_than_busy_forever() {
        // Another provider's resting prompt on screen. If any alpha arm
        // borrowed a glyph from Claude or Codex, this would read Resting.
        for provider in [
            ProviderKind::Gemini,
            ProviderKind::Grok,
            ProviderKind::OpenCode,
        ] {
            assert_eq!(
                classify_visible_text(provider, "❯\n› \n? for shortcuts"),
                ProviderActivity::Unknown,
                "{provider} must not borrow another provider's glyphs"
            );
            assert_eq!(
                classify_visible_text(provider, "some output nobody can classify"),
                ProviderActivity::Unknown,
                "{provider} is unclassified, which is Unknown rather than Active"
            );
        }
    }

    /// The generic busy signal still works for an alpha provider.
    ///
    /// So what the missing arms cost is the RESTING half, not the whole
    /// classification — "esc to interrupt" is near-universal among agent TUIs
    /// and is matched before any provider-specific branch.
    #[test]
    fn an_alpha_provider_is_still_seen_working_by_the_generic_signal() {
        for provider in [
            ProviderKind::Gemini,
            ProviderKind::Grok,
            ProviderKind::OpenCode,
        ] {
            assert_eq!(
                classify_visible_text(provider, "thinking... esc to interrupt"),
                ProviderActivity::Active,
                "{provider} mid-turn must not read as idle"
            );
        }
    }

    fn snapshot(text: &str) -> TerminalSnapshot {
        let mut state = crate::CanonicalTerminalState::new(
            crate::JournalLimits::new(64, 64 * 1024),
            crate::TerminalSize::new(24, 100),
        );
        state.push(text.as_bytes().to_vec());
        state.snapshot()
    }

    /// ⚠️ THE REPORTED SCREEN, read off this Hive at the moment the roster
    /// showed a working worker as Resting (2026-09-22). Auto mode lengthens the
    /// footer, the terminal cuts "esc to interrupt" to "esc …", and the only
    /// busy signal Swarm read was gone.
    #[test]
    fn an_auto_mode_worker_whose_interrupt_hint_was_truncated_is_still_working() {
        let screen = "● Bash(cargo test --workspace)\n  ⎿  Waiting…\n\n✽ Baking… (8m 38s · ↓ 23.5k tokens)\n\n────────────────────\n❯ \n────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle) · esc …";
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, screen),
            ProviderActivity::Active
        );
    }

    /// ⚠️ THE SCREEN THAT PROVED THE SPINNER ALONE WAS NOT ENOUGH, rendered at
    /// its real size (48 x 36, a phone owning the geometry) at a moment the
    /// roster said Resting while this worker was running a command. No spinner
    /// is in view: a leftover `/model` menu fills the top and the operator's
    /// queued message fills the bottom. Only the footer still says "working".
    #[test]
    fn a_working_screen_with_no_spinner_in_view_is_read_from_its_footer() {
        let screen = [
            "     4. Sonnet                   Sonnet 5 ·",
            "                                 Efficient for",
            "  ◐ Medium effort (default) ←/→ to adjust",
            "  Enter to set as default · s to use this",
            "  session only · Esc to cancel",
            "",
            "❯ /task Check the performance of switching",
            "  between workers and mobile. It's back to",
            "  taking a couple of seconds for the socket to",
            "  reconnect, which is weird since I'm not",
            "  minimizing the app.",
            "  ctrl+x ctrl+s to send now",
            "──────────────────── Task system gaps analysis ─",
            "❯\u{a0}Press up to edit queued messages",
            "────────────────────────────────────────────────",
            "  ⏵⏵ auto mode on (shift+tab to cycle) · esc …",
        ]
        .join("\n");
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, &screen),
            ProviderActivity::Active
        );
        // And with the status line a 48-column screen shows BENEATH the footer,
        // which a bottom-line-only reading got wrong.
        let screen = format!("{screen}\n                         0% until auto-compact");
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, &screen),
            ProviderActivity::Active
        );
    }

    /// ⚠️ A WORKER THAT TALKS ABOUT THE BUSY FOOTER IS NOT BUSY. This fix's own
    /// transcript quotes it verbatim; reading any row but the bottom one would
    /// call a resting worker working the moment it scrolled past its notes.
    #[test]
    fn a_transcript_quoting_the_busy_footer_does_not_make_an_idle_worker_busy() {
        let screen = "● The footer read:\n    ⏵⏵ auto mode on (shift+tab to cycle) · esc …\n  and that was the bug.\n\n────────────\n❯ \n────────────\n  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents";
        assert_ne!(
            classify_visible_text(ProviderKind::ClaudeCode, screen),
            ProviderActivity::Active
        );
    }

    /// Nor does quoting a spinner line: Claude draws the real one at column zero
    /// and indents everything it prints into the transcript.
    #[test]
    fn a_transcript_quoting_a_spinner_does_not_make_an_idle_worker_busy() {
        let screen = "● It showed:\n  ✽ Baking… (8m 38s · ↓ 23.5k tokens)\n\n✻ Cogitated for 2s\n\n❯ \n  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents";
        assert_ne!(
            classify_visible_text(ProviderKind::ClaudeCode, screen),
            ProviderActivity::Active
        );
    }

    /// Every busy footer form measured across two days of this Hive's history.
    #[test]
    fn every_measured_busy_footer_reads_as_working_and_no_idle_one_does() {
        for busy in [
            "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt · ← for agents",
            "  ⏵⏵ auto mode on (shift+tab to cycle) · esc…",
            "  ⏵⏵ auto mode on (shift+tab to cycle) · esc …",
        ] {
            assert!(
                super::claude_footer_says_interruptible(busy.trim()),
                "{busy}"
            );
        }
        for idle in [
            "  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents",
            "  ⏵⏵ auto mode on (shift+tab to cycle) · ← fo…",
            "  ⏵⏵ auto mode on (shift+tab to cycle) · 1 feedback",
            "  ⏵⏵ auto mode on (shift+tab to cycle)",
        ] {
            assert!(
                !super::claude_footer_says_interruptible(idle.trim()),
                "{idle}"
            );
        }
    }

    /// Hooks running before a tool are work too, and Claude says so on the
    /// spinner line rather than in the footer.
    #[test]
    fn a_spinner_reporting_hooks_is_working() {
        let screen = "✽ Baking… (running PreToolUse hooks… 0/2 · 8m 38s)\n\n❯ \n  ⏵⏵ auto mode on (shift+tab to cycle) · esc …";
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, screen),
            ProviderActivity::Active
        );
    }

    /// The finished turn from the operator's own screenshot. A past-tense verb
    /// with no ellipsis is a summary, not a spinner, and reading it as work
    /// would pin a resting worker Buzzing and defer every delivery to it.
    #[test]
    fn a_finished_turn_summary_is_not_a_spinner() {
        let screen = "● Got it — I'm here. What do you need?\n\n✻ Cogitated for 1s · done 1:38 PM\n\n────────────────────\n❯ \n────────────────────\n  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents";
        assert_ne!(
            classify_visible_text(ProviderKind::ClaudeCode, screen),
            ProviderActivity::Active
        );
    }

    /// Tool progress has the verb shape but not the glyph or the elapsed time.
    #[test]
    fn transcript_tool_progress_is_not_a_spinner() {
        assert!(!super::claude_spinner_is_running("⎿  Waiting…"));
        assert!(!super::claude_spinner_is_running(
            "✻ Brewed for 3s · done 11:33 AM"
        ));
        assert!(!super::claude_spinner_is_running("· Waiting on review"));
        assert!(super::claude_spinner_is_running(
            "✶ Mustering… (1s · ↓ 2 tokens)"
        ));
        assert!(super::claude_spinner_is_running(
            "· Testing… (2s · thinking)"
        ));
    }

    #[test]
    fn claude_completion_above_a_returned_prompt_is_resting() {
        let activity = classify_provider_activity(
            ProviderKind::ClaudeCode,
            &snapshot(
                "✻ Sautéed for 58s\r\n\r\n❯ \r\nmanual mode on · ? for shortcuts · ← for agents",
            ),
        );
        assert_eq!(activity, ProviderActivity::Resting);
    }

    /// Work that can be interrupted outranks a prompt; work left running behind
    /// one does not.
    ///
    /// This reverses half of a deliberate earlier rule, which held that a
    /// background monitor kept the provider active even with a prompt on
    /// screen. Measured cost of that rule on 2026-08-23: Queen finished a turn,
    /// left a `gh run watch` running, and read as busy for as long as the run
    /// took — showing as buzzing beside an idle terminal while every delivery to
    /// her was deferred and then surfaced in "Needs you" as work waiting behind
    /// a prompt that was not there.
    ///
    /// Writing to her in that state is safe: the composer is empty, the prompt
    /// is resting, and the background shell keeps running regardless.
    #[test]
    fn interruptible_work_outranks_a_prompt_and_a_background_monitor_does_not() {
        assert_eq!(
            classify_provider_activity(
                ProviderKind::ClaudeCode,
                &snapshot("✻ Verifying… · esc to interrupt\r\n❯ ")
            ),
            ProviderActivity::Active
        );

        assert_eq!(
            classify_provider_activity(
                ProviderKind::ClaudeCode,
                &snapshot("✻ Sautéed for 2m · 1 monitor still running\r\nauto mode on\r\n❯ ")
            ),
            ProviderActivity::Resting,
            "the turn has ended; something it started has not"
        );
    }

    #[test]
    fn claude_choice_menu_is_awaiting_operator() {
        let activity = classify_provider_activity(
            ProviderKind::ClaudeCode,
            &snapshot(
                "Choose an approach:\r\n❯ 1. Continue\r\n  2. Change course\r\n  3. Cancel\r\nEsc to cancel",
            ),
        );
        assert_eq!(activity, ProviderActivity::AwaitingOperator);
    }

    #[test]
    fn cropped_claude_ask_user_menu_keeps_its_question_identity() {
        // Fictional wording; real observed 80x24 shape. The selected first
        // option and its cursor are above the visible viewport.
        let menu = concat!(
            "continued explanation of the first option\r\n",
            "  2. Use the existing fixture\r\n",
            "     Check the fictional identifier.\r\n",
            "  3. Hold the test\r\n",
            "  4. Type something.\r\n",
            "────────────────────────\r\n",
            "  5. Chat about this\r\n\r\n",
            "Enter to select · ↑/↓ to navigate · Esc to cancel",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(menu)),
            ProviderActivity::AwaitingOperator,
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot(menu)),
            ProviderActivity::Unknown,
        );
        assert_eq!(
            classify_provider_activity(
                ProviderKind::ClaudeCode,
                &snapshot(&format!("{menu}\r\n❯ \r\nauto mode on")),
            ),
            ProviderActivity::Resting,
            "a historical menu above a returned composer is not a current question",
        );
        assert_eq!(
            classify_provider_activity(
                ProviderKind::ClaudeCode,
                &snapshot("Enter to select · ↑/↓ to navigate · Esc to cancel"),
            ),
            ProviderActivity::Unknown,
            "a footer alone is insufficient evidence of AskUser",
        );
    }

    #[test]
    fn wrapped_claude_confirmation_footer_is_awaiting_operator() {
        let activity = classify_provider_activity(
            ProviderKind::ClaudeCode,
            &snapshot(
                "Quick safety check:\r\n❯\r\n1.\r\nYes, I trust this folder\r\n2.\r\nNo, exit\r\nEnter to confirm · Esc to cancel",
            ),
        );
        assert_eq!(activity, ProviderActivity::AwaitingOperator);
    }

    /// `AwaitingOperator` is Claude-only, and every other provider reads `Unknown`.
    ///
    /// This is not a complaint about the classifier — Unknown is the honest
    /// answer, and guessing another provider's glyphs would be worse. It is
    /// pinned because of what READS the answer: any predicate that treats
    /// "not mid-turn" as "safe to stop" turns this honest Unknown into a
    /// confident "idle", and would stop a worker sitting at a permission
    /// prompt. Measured, so a later change to the arms shows up here rather
    /// than inside a safety check.
    /// Every one of these is transcribed from a live `codex` session captured
    /// in a PTY on 2026-09-01 and replayed through this classifier.
    ///
    /// The three prompts do NOT share a footer — two say "Press enter to
    /// confirm or esc to cancel" and the update notice says "Press enter to
    /// continue". That is why the discriminator is the menu shape and not the
    /// sentence: keying on the sentence would have passed the two approvals
    /// and silently missed the third.
    #[test]
    fn a_real_codex_command_approval_awaits_the_operator() {
        let screen = concat!(
            "• Running mkdir -p /tmp/probe-dir-two\r\n",
            "  Would you like to run the following command?\r\n",
            "  Environment: local\r\n",
            "  $ mkdir -p /tmp/probe-dir-two\r\n",
            "› 1. Yes, proceed (y)\r\n",
            "  2. Yes, and don't ask again for commands that start with `mkdir` (p)\r\n",
            "  3. No, and tell Codex what to do differently (esc)\r\n",
            "  Press enter to confirm or esc to cancel",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot(screen)),
            ProviderActivity::AwaitingOperator
        );
    }

    #[test]
    fn a_real_codex_edit_approval_awaits_the_operator() {
        let screen = concat!(
            "• Added /tmp/probe-note.txt (+1 -0)\r\n",
            "    1 +hello\r\n",
            "  Would you like to make the following edits?\r\n",
            "› 1. Yes, proceed (y)\r\n",
            "  2. Yes, and don't ask again for these files (a)\r\n",
            "  3. No, and tell Codex what to do differently (esc)\r\n",
            "  Press enter to confirm or esc to cancel",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot(screen)),
            ProviderActivity::AwaitingOperator
        );
    }

    /// The prompt that proves the footer is not the discriminator.
    #[test]
    fn a_real_codex_update_notice_awaits_the_operator() {
        let screen = concat!(
            "  ✨ Update available! 0.147.0 -> 0.152.0\r\n",
            "  Release notes: https://github.com/openai/codex/releases/latest\r\n",
            "› 1. Update now (runs `npm install -g @openai/codex`)\r\n",
            "  2. Skip\r\n",
            "  3. Skip until next version\r\n",
            "  Press enter to continue",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot(screen)),
            ProviderActivity::AwaitingOperator,
            "no confirm/cancel footer here — the menu shape is what identifies it"
        );
    }

    /// THE NEGATIVE, AND IT IS THE ONE THAT MATTERS.
    ///
    /// Delivery submits to a worker only while it reads Resting, so a
    /// discriminator that made any Codex screen non-Resting would stop work
    /// reaching Codex workers entirely and silently — presenting as "nothing
    /// is being assigned" rather than as a bug. A real composer carries the
    /// same leading `›` as the menu cursor and must still rest.
    #[test]
    fn a_real_codex_composer_still_rests() {
        let screen = concat!(
            "  Tip: Use /skills to list available skills or ask Codex to use one.\r\n",
            "• You have 1 usage limit reset available. Run /usage to use one.\r\n",
            "› Summarize recent commits\r\n",
            "  gpt-5.6-sol medium · ~/projects/personal/swarm-next",
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot(screen)),
            ProviderActivity::Resting,
            "the composer glyph is the menu glyph; only the numbered options differ"
        );
    }

    /// A menu is only recognised through the glyph its own provider draws.
    ///
    /// Claude's `❯` on a Codex screen means nothing, and vice versa, so the
    /// same menu is Unknown to everyone who does not own that cursor. The
    /// alpha providers own none, deliberately: their CLIs are not installed
    /// here, nobody has watched one draw a menu, and Unknown is the honest
    /// answer until somebody has.
    ///
    /// It is pinned because of what READS the answer. Unknown is uncertain and
    /// consumers can route on it; the danger is a confident wrong answer, which
    /// is exactly what Codex used to give here.
    #[test]
    fn a_menu_is_unknown_to_a_provider_that_does_not_draw_that_cursor() {
        let claude_menu =
            "Allow this command?\r\n❯ 1. Yes\r\n2. No\r\nEnter to confirm · Esc to cancel";

        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(claude_menu)),
            ProviderActivity::AwaitingOperator
        );

        for provider in [
            ProviderKind::Codex,
            ProviderKind::Gemini,
            ProviderKind::Grok,
            ProviderKind::OpenCode,
        ] {
            assert_eq!(
                classify_provider_activity(provider, &snapshot(claude_menu)),
                ProviderActivity::Unknown,
                "{provider:?} does not draw ❯, so this menu is not its to read"
            );
        }

        let codex_menu = "Would you like to run this?\r\n› 1. Yes\r\n2. No\r\nPress enter";
        assert_eq!(
            classify_provider_activity(ProviderKind::Gemini, &snapshot(codex_menu)),
            ProviderActivity::Unknown,
            "and an alpha provider stays Unknown on a Codex-shaped menu too"
        );
    }

    #[test]
    fn codex_prompt_is_resting_and_unrecognized_output_is_unknown() {
        assert_eq!(
            classify_provider_activity(ProviderKind::Codex, &snapshot("› ")),
            ProviderActivity::Resting
        );
        assert_eq!(
            classify_provider_activity(ProviderKind::ClaudeCode, &snapshot("starting provider")),
            ProviderActivity::Unknown
        );
    }

    /// The operator: "the queen says she is buzzing but her terminal is idle."
    ///
    /// Her turn had ended — resting prompt, footer showing, composer empty —
    /// and a `gh run watch` she started was still going. "1 shell still
    /// running" was read as the provider being busy, so she showed as buzzing
    /// and every delivery to her was deferred for as long as the run took.
    #[test]
    fn a_background_shell_does_not_make_a_resting_prompt_busy() {
        let resting_with_shell = concat!(
            "✻ Churned for 48s · 1 shell still running\n",
            "❯\n",
            "⏵⏵ auto mode on · 1 shell · ← for agents\n",
        );

        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, resting_with_shell),
            ProviderActivity::Resting
        );
    }

    /// With no sign the turn has ended, a running shell is still the best
    /// evidence available and must keep coordination out.
    #[test]
    fn a_background_shell_still_counts_when_nothing_says_the_turn_ended() {
        let no_prompt = "Running the deploy…\n2 shells still running\n";

        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, no_prompt),
            ProviderActivity::Active
        );
    }

    /// The signal that actually means "do not interrupt" still outranks
    /// everything, including a prompt further up the screen.
    #[test]
    fn a_working_provider_is_never_read_as_resting() {
        let working = concat!("❯ do the thing\n", "✻ Working… (esc to interrupt)\n");

        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, working),
            ProviderActivity::Active
        );
    }

    /// The two situations that both classify as Resting, and must not read the
    /// same to an operator.
    #[test]
    fn a_finished_turn_reports_whether_it_left_something_running() {
        // The operator's own terminal text, from the report that produced the
        // resting-outranks-a-shell rule.
        let resting_with_shell = concat!(
            "✻ Churned for 48s · 1 shell still running\n",
            "❯\n",
            "⏵⏵ auto mode on · 1 shell · ← for agents\n",
        );
        let resting_alone = "❯\n";
        assert!(
            background_work_signal(ProviderKind::ClaudeCode, &resting_with_shell.to_lowercase()),
            "a shell the worker started is still running"
        );
        assert!(
            !background_work_signal(ProviderKind::ClaudeCode, &resting_alone.to_lowercase()),
            "and a bare prompt has nothing running behind it"
        );
        // Both are still Resting. The turn HAS ended in each; that is the whole
        // reason this is reported separately rather than as a third activity.
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, resting_with_shell),
            ProviderActivity::Resting
        );
        assert_eq!(
            classify_visible_text(ProviderKind::ClaudeCode, resting_alone),
            ProviderActivity::Resting
        );
    }
}
