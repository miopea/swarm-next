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
    let normalized = visible.to_lowercase();
    if active_signal(&normalized) {
        return ProviderActivity::Active;
    }

    if provider == ProviderKind::ClaudeCode
        && normalized.contains("esc to cancel")
        && (normalized.contains("enter to confirm")
            || visible.lines().any(|line| is_choice_cursor(line.trim())))
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

    if provider == ProviderKind::ClaudeCode
        && recent.iter().any(|line| is_choice_cursor(line))
        && recent
            .iter()
            .filter(|line| is_numbered_choice(line))
            .count()
            >= 2
    {
        return ProviderActivity::AwaitingOperator;
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

/// The provider is working on the operator's turn and must not be interrupted.
fn active_signal(normalized: &str) -> bool {
    normalized.contains("esc to interrupt")
        || normalized.contains("esc to int…")
        || normalized.contains("esc to stop")
        || normalized.contains("esc to …")
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
        ProviderKind::Codex => false,
    }
}

fn idle_prompt(provider: ProviderKind, line: &str) -> bool {
    match provider {
        ProviderKind::ClaudeCode => line == "❯" || line.starts_with("❯ "),
        ProviderKind::Codex => line == "›" || line.starts_with("› "),
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
    }
}

fn is_choice_cursor(line: &str) -> bool {
    line.strip_prefix('❯')
        .is_some_and(|tail| is_numbered_choice(tail.trim_start()))
}

fn is_numbered_choice(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && line[digits..].starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(text: &str) -> TerminalSnapshot {
        let mut state = crate::CanonicalTerminalState::new(
            crate::JournalLimits::new(64, 64 * 1024),
            crate::TerminalSize::new(24, 100),
        );
        state.push(text.as_bytes().to_vec());
        state.snapshot()
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
    fn wrapped_claude_confirmation_footer_is_awaiting_operator() {
        let activity = classify_provider_activity(
            ProviderKind::ClaudeCode,
            &snapshot(
                "Quick safety check:\r\n❯\r\n1.\r\nYes, I trust this folder\r\n2.\r\nNo, exit\r\nEnter to confirm · Esc to cancel",
            ),
        );
        assert_eq!(activity, ProviderActivity::AwaitingOperator);
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
}
