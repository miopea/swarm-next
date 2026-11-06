//! When Queen's next start begins a new conversation (ADR 0160).
//!
//! The operator chose a weekly rotation, for Queen only. A rotation happens at
//! a start and never stops anything: Swarm does not schedule Queen's restarts,
//! it only decides, when one happens, whether to resume her conversation or
//! move her onto a new one. Over the 28 days before this was written Queen
//! started 37 times, never more than 3.6 days apart, so the first start after
//! the week is up lands within about a week and a half.

use swarm_domain::{ProviderConversationId, ProviderKind, WorkerProfile, WorkerRole};
use swarm_persistence::RotationHold;

/// How long Queen keeps one conversation before her next start rotates it.
pub const QUEEN_CONVERSATION_SECONDS: i64 = 7 * 24 * 60 * 60;

/// The conversation this start should move away from, if a rotation is due.
///
/// `rotated_at` is when a rotation began the CURRENT pinned conversation. When
/// none did, its age is unknown and it is due: it predates rotations, or the
/// operator or Claude moved the pin, and in each case nothing says it is young.
///
/// Only Queen, only Claude, and only a conversation that has already run. A
/// worker that never started has nothing to rotate, and its pinned id is the
/// one its first start creates.
///
/// `hold` puts a due rotation off to a later start. Queen asked that none
/// happen during a live automation run or a takeover pause; with a fork she
/// carries the same context either way, so waiting costs only a few days of
/// growth in the old file.
#[must_use]
pub fn conversation_due_for_rotation(
    profile: &WorkerProfile,
    rotated_at: Option<i64>,
    hold: Option<RotationHold>,
    now: i64,
) -> Option<ProviderConversationId> {
    if profile.role != WorkerRole::Queen
        || profile.provider != ProviderKind::ClaudeCode
        || !profile.has_session_history
        || hold.is_some()
    {
        return None;
    }
    let pinned = profile.provider_conversation_id?;
    match rotated_at {
        Some(at) if now.saturating_sub(at) < QUEEN_CONVERSATION_SECONDS => None,
        _ => Some(pinned),
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderConversationId, ProviderKind, WorkerRole};
    use swarm_persistence::{RotationHold, TaskStore};

    use super::{QUEEN_CONVERSATION_SECONDS, conversation_due_for_rotation};

    const NOW: i64 = 1_800_000_000;

    fn queen() -> swarm_domain::WorkerProfile {
        let store = TaskStore::in_memory().unwrap();
        let mut profile = store
            .create_worker("Queen", ProviderKind::ClaudeCode, "/queen", false, 1)
            .unwrap();
        profile.role = WorkerRole::Queen;
        profile.has_session_history = true;
        profile.provider_conversation_id = Some(ProviderConversationId::new());
        profile
    }

    #[test]
    fn a_week_old_conversation_is_due_and_a_younger_one_is_not() {
        let queen = queen();
        let pinned = queen.provider_conversation_id;

        assert_eq!(
            conversation_due_for_rotation(
                &queen,
                Some(NOW - QUEEN_CONVERSATION_SECONDS),
                None,
                NOW
            ),
            pinned
        );
        assert_eq!(
            conversation_due_for_rotation(
                &queen,
                Some(NOW - QUEEN_CONVERSATION_SECONDS + 1),
                None,
                NOW
            ),
            None
        );
    }

    /// Queen's conversation on the day this shipped had run since August with
    /// no rotation on record. Unknown age must not read as young.
    #[test]
    fn a_conversation_no_rotation_started_is_due() {
        let queen = queen();

        assert_eq!(
            conversation_due_for_rotation(&queen, None, None, NOW),
            queen.provider_conversation_id
        );
    }

    /// The operator scoped this to Queen.
    #[test]
    fn only_queen_rotates() {
        let mut worker = queen();
        worker.role = WorkerRole::Worker;
        assert_eq!(
            conversation_due_for_rotation(&worker, None, None, NOW),
            None
        );
    }

    /// Queen's condition: a live run or a takeover pause puts it off, and
    /// the next start without one rotates.
    #[test]
    fn a_hold_puts_a_due_rotation_off() {
        let queen = queen();
        for hold in [
            RotationHold::AutomationRunLive,
            RotationHold::StewardTakeover,
        ] {
            assert_eq!(
                conversation_due_for_rotation(&queen, None, Some(hold), NOW),
                None
            );
        }
        assert_eq!(
            conversation_due_for_rotation(&queen, None, None, NOW),
            queen.provider_conversation_id
        );
    }

    #[test]
    fn nothing_rotates_before_it_has_run_or_without_claude() {
        let mut fresh = queen();
        fresh.has_session_history = false;
        assert_eq!(conversation_due_for_rotation(&fresh, None, None, NOW), None);

        let mut unpinned = queen();
        unpinned.provider_conversation_id = None;
        assert_eq!(
            conversation_due_for_rotation(&unpinned, None, None, NOW),
            None
        );

        let mut codex = queen();
        codex.provider = ProviderKind::Codex;
        assert_eq!(conversation_due_for_rotation(&codex, None, None, NOW), None);
    }
}
