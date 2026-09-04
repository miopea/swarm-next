//! Bounded recovery of provider context. Starting a process is not evidence
//! that it restored a conversation. Adapters own execution, not this policy.

use crate::{ConversationRecoveryId, ProviderConversationId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationRecoveryStep {
    Exact {
        conversation: ProviderConversationId,
    },
    Continue,
    Fresh,
}

/// Carries the expected step so delayed evidence from an earlier attempt cannot
/// complete or advance a later one. Scoped to one recovery operation by its owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConversationRecoveryAttempt {
    pub recovery_id: ConversationRecoveryId,
    pub number: u8,
    pub step: ConversationRecoveryStep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationRecoveryEvidence {
    /// Provider-authoritative evidence, not a running PID or a timer.
    Restored(ProviderConversationId),
    /// Positive failure to recover context. Transport/auth errors are Unknown.
    ContextUnavailable,
    FreshStarted(ProviderConversationId),
    /// Ambiguous outcome: do not manufacture absence or silently discard context.
    Unknown,
}

/// Provider-neutral lifecycle evidence, normalized by the provider adapter.
/// These values do not authenticate a callback or authorize a conversation switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSessionStartKind {
    New,
    Resumed,
    Reset,
    Compacted,
    Forked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRecoveryStop {
    ProviderCannotResume,
    UncertainOutcome,
    UnexpectedConversation,
    FreshStartFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConversationRecoveryState {
    Attempt {
        attempt: ConversationRecoveryAttempt,
    },
    Restored {
        conversation: ProviderConversationId,
        via_continue: bool,
    },
    /// Explicitly not restored context. This never authorizes replaying a task.
    Fresh {
        conversation: ProviderConversationId,
    },
    Manual {
        reason: ConversationRecoveryStop,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationRecovery {
    state: ConversationRecoveryState,
}

impl ConversationRecovery {
    /// Applies authenticated startup evidence only to its bound engine session.
    /// Non-startup lifecycle events cannot settle or advance the recovery ladder.
    pub fn observe_provider_start(
        &mut self,
        bound_session: crate::WorkerSessionId,
        observed_session: crate::WorkerSessionId,
        attempt: ConversationRecoveryAttempt,
        kind: ProviderSessionStartKind,
        conversation: ProviderConversationId,
    ) -> bool {
        if bound_session != observed_session {
            return false;
        }
        let evidence = match kind {
            ProviderSessionStartKind::Resumed => {
                ConversationRecoveryEvidence::Restored(conversation)
            }
            ProviderSessionStartKind::New => {
                ConversationRecoveryEvidence::FreshStarted(conversation)
            }
            ProviderSessionStartKind::Reset
            | ProviderSessionStartKind::Compacted
            | ProviderSessionStartKind::Forked
            | ProviderSessionStartKind::Unknown => return false,
        };
        self.observe(attempt, evidence)
    }

    #[must_use]
    pub fn new(chosen: Option<ProviderConversationId>, provider_can_resume: bool) -> Self {
        let state = if provider_can_resume {
            ConversationRecoveryState::Attempt {
                attempt: ConversationRecoveryAttempt {
                    recovery_id: ConversationRecoveryId::new(),
                    number: 1,
                    step: match chosen {
                        Some(conversation) => ConversationRecoveryStep::Exact { conversation },
                        None => ConversationRecoveryStep::Continue,
                    },
                },
            }
        } else {
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::ProviderCannotResume,
            }
        };
        Self { state }
    }

    #[must_use]
    pub const fn state(self) -> ConversationRecoveryState {
        self.state
    }

    /// Returns false for stale/duplicate evidence. Completed operations cannot
    /// be reopened, and no transition changes provider or chooses by timestamp.
    pub fn observe(
        &mut self,
        observed: ConversationRecoveryAttempt,
        evidence: ConversationRecoveryEvidence,
    ) -> bool {
        let ConversationRecoveryState::Attempt { attempt } = self.state else {
            return false;
        };
        if attempt != observed {
            return false;
        }
        self.state = match evidence {
            ConversationRecoveryEvidence::Unknown => ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::UncertainOutcome,
            },
            ConversationRecoveryEvidence::ContextUnavailable => match attempt.step {
                ConversationRecoveryStep::Exact { .. } => {
                    Self::next(attempt, ConversationRecoveryStep::Continue)
                }
                ConversationRecoveryStep::Continue => {
                    Self::next(attempt, ConversationRecoveryStep::Fresh)
                }
                ConversationRecoveryStep::Fresh => ConversationRecoveryState::Manual {
                    reason: ConversationRecoveryStop::FreshStartFailed,
                },
            },
            ConversationRecoveryEvidence::Restored(conversation) => match attempt.step {
                ConversationRecoveryStep::Exact {
                    conversation: chosen,
                } if chosen == conversation => ConversationRecoveryState::Restored {
                    conversation,
                    via_continue: false,
                },
                ConversationRecoveryStep::Continue => ConversationRecoveryState::Restored {
                    conversation,
                    via_continue: true,
                },
                _ => ConversationRecoveryState::Manual {
                    reason: ConversationRecoveryStop::UnexpectedConversation,
                },
            },
            ConversationRecoveryEvidence::FreshStarted(conversation) => match attempt.step {
                ConversationRecoveryStep::Fresh => {
                    ConversationRecoveryState::Fresh { conversation }
                }
                _ => ConversationRecoveryState::Manual {
                    reason: ConversationRecoveryStop::UnexpectedConversation,
                },
            },
        };
        true
    }

    const fn next(
        previous: ConversationRecoveryAttempt,
        step: ConversationRecoveryStep,
    ) -> ConversationRecoveryState {
        // Only Exact -> Continue -> Fresh can reach this function: at most three.
        ConversationRecoveryState::Attempt {
            attempt: ConversationRecoveryAttempt {
                recovery_id: previous.recovery_id,
                number: previous.number + 1,
                step,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(recovery: ConversationRecovery) -> ConversationRecoveryAttempt {
        let ConversationRecoveryState::Attempt { attempt } = recovery.state() else {
            panic!("expected attempt");
        };
        attempt
    }

    #[test]
    fn exact_continue_fresh_is_a_three_attempt_ladder_not_a_loop() {
        let chosen = ProviderConversationId::new();
        let fresh = ProviderConversationId::new();
        let mut recovery = ConversationRecovery::new(Some(chosen), true);
        let first = attempt(recovery);
        assert_eq!(
            first.step,
            ConversationRecoveryStep::Exact {
                conversation: chosen
            }
        );
        assert!(recovery.observe(first, ConversationRecoveryEvidence::ContextUnavailable));
        let second = attempt(recovery);
        assert_eq!(
            second,
            ConversationRecoveryAttempt {
                recovery_id: first.recovery_id,
                number: 2,
                step: ConversationRecoveryStep::Continue
            }
        );
        assert!(!recovery.observe(first, ConversationRecoveryEvidence::Restored(chosen)));
        assert!(recovery.observe(second, ConversationRecoveryEvidence::ContextUnavailable));
        let third = attempt(recovery);
        assert_eq!(
            third,
            ConversationRecoveryAttempt {
                recovery_id: first.recovery_id,
                number: 3,
                step: ConversationRecoveryStep::Fresh
            }
        );
        assert!(recovery.observe(third, ConversationRecoveryEvidence::FreshStarted(fresh)));
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Fresh {
                conversation: fresh
            }
        );
        assert!(!recovery.observe(third, ConversationRecoveryEvidence::ContextUnavailable));
    }

    #[test]
    fn exact_recovery_must_restore_the_chosen_identity() {
        for matching in [false, true] {
            let chosen = ProviderConversationId::new();
            let mut recovery = ConversationRecovery::new(Some(chosen), true);
            let returned = if matching {
                chosen
            } else {
                ProviderConversationId::new()
            };
            recovery.observe(
                attempt(recovery),
                ConversationRecoveryEvidence::Restored(returned),
            );
            assert_eq!(
                recovery.state(),
                if matching {
                    ConversationRecoveryState::Restored {
                        conversation: chosen,
                        via_continue: false,
                    }
                } else {
                    ConversationRecoveryState::Manual {
                        reason: ConversationRecoveryStop::UnexpectedConversation,
                    }
                }
            );
        }
    }

    #[test]
    fn native_continue_is_distinct_from_exact_recovery() {
        let mut recovery = ConversationRecovery::new(None, true);
        let returned = ProviderConversationId::new();
        assert_eq!(attempt(recovery).step, ConversationRecoveryStep::Continue);
        recovery.observe(
            attempt(recovery),
            ConversationRecoveryEvidence::Restored(returned),
        );
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Restored {
                conversation: returned,
                via_continue: true
            }
        );
    }

    #[test]
    fn unknown_outcomes_do_not_advance_or_restart() {
        let mut recovery = ConversationRecovery::new(Some(ProviderConversationId::new()), true);
        let first = attempt(recovery);
        recovery.observe(first, ConversationRecoveryEvidence::Unknown);
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::UncertainOutcome
            }
        );
        assert!(!recovery.observe(first, ConversationRecoveryEvidence::ContextUnavailable));
    }

    #[test]
    fn unsupported_providers_remain_manual() {
        assert_eq!(
            ConversationRecovery::new(None, false).state(),
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::ProviderCannotResume
            }
        );
    }

    #[test]
    fn silent_fresh_start_cannot_masquerade_as_continuity() {
        let mut recovery = ConversationRecovery::new(None, true);
        recovery.observe(
            attempt(recovery),
            ConversationRecoveryEvidence::FreshStarted(ProviderConversationId::new()),
        );
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::UnexpectedConversation
            }
        );
    }

    #[test]
    fn evidence_from_another_recovery_cannot_advance_this_one() {
        let mut current = ConversationRecovery::new(None, true);
        let other = ConversationRecovery::new(None, true);
        let before = current.state();
        assert!(!current.observe(
            attempt(other),
            ConversationRecoveryEvidence::ContextUnavailable
        ));
        assert_eq!(current.state(), before);
    }

    #[test]
    fn a_failed_final_start_stops_instead_of_starting_again() {
        let mut recovery = ConversationRecovery::new(None, true);
        recovery.observe(
            attempt(recovery),
            ConversationRecoveryEvidence::ContextUnavailable,
        );
        let last = attempt(recovery);
        assert_eq!(last.step, ConversationRecoveryStep::Fresh);
        recovery.observe(last, ConversationRecoveryEvidence::ContextUnavailable);
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::FreshStartFailed
            }
        );
        assert!(!recovery.observe(last, ConversationRecoveryEvidence::ContextUnavailable));
    }

    #[test]
    fn changing_the_attempt_step_does_not_skip_recovery() {
        let mut recovery = ConversationRecovery::new(Some(ProviderConversationId::new()), true);
        let before = recovery.state();
        let mut forged = attempt(recovery);
        forged.step = ConversationRecoveryStep::Fresh;
        assert!(!recovery.observe(
            forged,
            ConversationRecoveryEvidence::FreshStarted(ProviderConversationId::new())
        ));
        assert_eq!(recovery.state(), before);
    }

    #[test]
    fn lifecycle_evidence_requires_current_process_and_matching_attempt() {
        let session = crate::WorkerSessionId::new();
        let conversation = ProviderConversationId::new();
        let mut recovery = ConversationRecovery::new(None, true);
        let current = attempt(recovery);
        for kind in [
            ProviderSessionStartKind::Reset,
            ProviderSessionStartKind::Compacted,
            ProviderSessionStartKind::Forked,
            ProviderSessionStartKind::Unknown,
        ] {
            assert!(!recovery.observe_provider_start(
                session,
                session,
                current,
                kind,
                conversation
            ));
        }
        assert!(!recovery.observe_provider_start(
            session,
            crate::WorkerSessionId::new(),
            current,
            ProviderSessionStartKind::Resumed,
            conversation
        ));
        let other = attempt(ConversationRecovery::new(None, true));
        assert!(!recovery.observe_provider_start(
            session,
            session,
            other,
            ProviderSessionStartKind::Resumed,
            conversation
        ));
        assert!(recovery.observe_provider_start(
            session,
            session,
            current,
            ProviderSessionStartKind::Resumed,
            conversation
        ));
        assert_eq!(
            recovery.state(),
            ConversationRecoveryState::Restored {
                conversation,
                via_continue: true
            }
        );
        assert!(!recovery.observe_provider_start(
            session,
            session,
            current,
            ProviderSessionStartKind::Resumed,
            conversation
        ));
    }

    #[test]
    fn startup_is_fresh_only_when_fresh_was_the_authorized_attempt() {
        let session = crate::WorkerSessionId::new();
        let conversation = ProviderConversationId::new();
        let mut continuing = ConversationRecovery::new(None, true);
        let current = attempt(continuing);
        assert!(continuing.observe_provider_start(
            session,
            session,
            current,
            ProviderSessionStartKind::New,
            conversation
        ));
        assert_eq!(
            continuing.state(),
            ConversationRecoveryState::Manual {
                reason: ConversationRecoveryStop::UnexpectedConversation
            }
        );
        let mut fresh = ConversationRecovery::new(None, true);
        fresh.observe(
            attempt(fresh),
            ConversationRecoveryEvidence::ContextUnavailable,
        );
        let current = attempt(fresh);
        assert!(fresh.observe_provider_start(
            session,
            session,
            current,
            ProviderSessionStartKind::New,
            conversation
        ));
        assert_eq!(
            fresh.state(),
            ConversationRecoveryState::Fresh { conversation }
        );
    }
}
