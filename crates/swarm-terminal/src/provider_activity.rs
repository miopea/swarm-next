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
    if active_signal(provider, &normalized) {
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
        return ProviderActivity::Resting;
    }

    ProviderActivity::Unknown
}

fn active_signal(provider: ProviderKind, normalized: &str) -> bool {
    let common = normalized.contains("esc to interrupt")
        || normalized.contains("esc to int…")
        || normalized.contains("esc to stop")
        || normalized.contains("esc to …");
    if common {
        return true;
    }
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

    #[test]
    fn interruptible_and_background_work_remain_active_even_with_a_prompt() {
        for text in [
            "✻ Verifying… · esc to interrupt\r\n❯ ",
            "✻ Sautéed for 2m · 1 monitor still running\r\nauto mode on\r\n❯ ",
        ] {
            assert_eq!(
                classify_provider_activity(ProviderKind::ClaudeCode, &snapshot(text)),
                ProviderActivity::Active
            );
        }
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
}
