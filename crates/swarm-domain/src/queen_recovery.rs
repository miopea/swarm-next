//! Evidence gates for Queen's recovery responsibility, not task completion.
//! Application/persistence must supply current observations and verified sources.

use crate::{DecisionRequestId, TaskId, WorkerId, WorkerSessionId};

/// Every receipt belongs to one exact observed obligation, including its session.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueenRecoveryIdentity {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub attention_id: String,
    pub evidence_revision: String,
    /// Filled only by a current canonical-terminal read, never by persistence alone.
    pub terminal_revision: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryTerminalActivity {
    Working,
    Resting,
    AwaitingOperator,
    Unknown,
}

/// Server-observed permission to request continuation, never task completion.
/// Not deserializable: a provider cannot assert its own terminal safety.
#[derive(Clone, Copy, Debug)]
pub struct QueenReviewContinuationObservation {
    pub current_complete_snapshot: bool,
    pub activity: RecoveryTerminalActivity,
    pub background_work: bool,
    pub operator_engaged: bool,
    pub unsent_input: Option<bool>,
}

impl QueenReviewContinuationObservation {
    #[must_use]
    pub fn permits_continuation(self) -> bool {
        self.current_complete_snapshot
            && self.activity == RecoveryTerminalActivity::Resting
            && !self.background_work
            && !self.operator_engaged
            && self.unsent_input == Some(false)
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::*;

    #[test]
    fn continuation_requires_known_empty_idle_terminal_and_no_other_owner() {
        let safe = QueenReviewContinuationObservation {
            current_complete_snapshot: true,
            activity: RecoveryTerminalActivity::Resting,
            background_work: false,
            operator_engaged: false,
            unsent_input: Some(false),
        };
        assert!(safe.permits_continuation());
        for activity in [
            RecoveryTerminalActivity::Working,
            RecoveryTerminalActivity::AwaitingOperator,
            RecoveryTerminalActivity::Unknown,
        ] {
            assert!(
                !QueenReviewContinuationObservation { activity, ..safe }.permits_continuation()
            );
        }
        for observation in [
            QueenReviewContinuationObservation {
                current_complete_snapshot: false,
                ..safe
            },
            QueenReviewContinuationObservation {
                background_work: true,
                ..safe
            },
            QueenReviewContinuationObservation {
                operator_engaged: true,
                ..safe
            },
            QueenReviewContinuationObservation {
                unsent_input: Some(true),
                ..safe
            },
            QueenReviewContinuationObservation {
                unsent_input: None,
                ..safe
            },
        ] {
            assert!(!observation.permits_continuation());
        }
    }
}

/// Not deserializable: callers obtain these facts from their authoritative owners,
/// never from a model's assertion that it inspected or delivered something.
#[derive(Clone, Debug)]
pub struct QueenRecoveryFacts {
    pub identity: QueenRecoveryIdentity,
    pub terminal_is_current: bool,
    pub activity: RecoveryTerminalActivity,
    pub operator_engaged: bool,
    pub unsent_input: Option<bool>,
    /// Current exact task/assignee/session-bound guarded delivery, not an old send.
    pub pending_message_id: Option<String>,
    /// A current pending task-linked operator decision verified by persistence.
    pub pending_decision_id: Option<DecisionRequestId>,
    /// An external wait checked for this run, with its source validated separately.
    pub verified_external_wait: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueenRecoveryDisposition {
    ObservedWorking,
    ProtectOperatorInput,
    AwaitDelivery {
        message_id: String,
    },
    AwaitOperator {
        decision_id: DecisionRequestId,
    },
    /// A checked Queen judgment, not machine proof or operator authority.
    VerifiedExternalWait {
        checked_evidence: String,
    },
}

impl QueenRecoveryDisposition {
    /// Explain the evidence required by a refused disposition, without claiming
    /// that a rejected assessment means database corruption or stale identity.
    #[must_use]
    pub const fn evidence_requirement(&self) -> &'static str {
        match self {
            Self::ObservedWorking => {
                "observed_working requires current terminal evidence of active or background execution; a resting worker is not working"
            }
            Self::ProtectOperatorInput => {
                "protect_operator_input requires current operator engagement or actual unsent input; a provider suggestion is not operator input"
            }
            Self::AwaitDelivery { .. } => {
                "await_delivery requires this exact message to be queued or dispatching to the current assignee. A delivered message is not pending delivery. Read the worker's response and verify the actual dependency, operator decision or external condition; do not resend merely to obtain coverage"
            }
            Self::AwaitOperator { .. } => {
                "await_operator requires this exact pending task-linked operator decision; a resolved or unrelated decision does not qualify"
            }
            Self::VerifiedExternalWait { .. } => {
                "verified_external_wait requires a freshly checked condition, evidence and source for this run; it is not permission to hide actionable work"
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueenRecoveryRecord {
    pub run_id: String,
    pub identity: QueenRecoveryIdentity,
    pub disposition: QueenRecoveryDisposition,
    pub reason: String,
    pub source: String,
}

impl QueenRecoveryRecord {
    /// # Errors
    /// Rejects unbounded explanations and missing source/run identity.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let QueenRecoveryDisposition::VerifiedExternalWait { checked_evidence } =
            &self.disposition
            && (checked_evidence.trim().is_empty() || checked_evidence.len() > 2000)
        {
            return Err("external wait requires concise checked evidence and its source");
        }
        if self.run_id.parse::<uuid::Uuid>().is_err()
            || self.reason.trim().is_empty()
            || self.reason.len() > 1000
            || self.source.trim().is_empty()
            || self.source.len() > 2000
        {
            return Err(
                "recovery assessment requires a current run, concise reason and checked source",
            );
        }
        Ok(())
    }
}

/// A covered wait is not proof of execution, completion, or permission to inject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueenRecoveryAssessment {
    Working,
    Waiting,
    Uncovered,
}

#[must_use]
pub fn assess_queen_recovery(
    expected: &QueenRecoveryIdentity,
    facts: &QueenRecoveryFacts,
    disposition: &QueenRecoveryDisposition,
) -> QueenRecoveryAssessment {
    if expected != &facts.identity
        || expected.attention_id.is_empty()
        || expected.attention_id.len() > 128
        || expected.evidence_revision.len() != 64
        || !expected
            .evidence_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || expected.terminal_revision.as_ref().is_some_and(|revision| {
            revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return QueenRecoveryAssessment::Uncovered;
    }
    match disposition {
        QueenRecoveryDisposition::ObservedWorking
            if facts.terminal_is_current
                && facts.identity.terminal_revision.is_some()
                && facts.activity == RecoveryTerminalActivity::Working =>
        {
            QueenRecoveryAssessment::Working
        }
        QueenRecoveryDisposition::ProtectOperatorInput
            if facts.operator_engaged
                || (facts.terminal_is_current
                    && facts.identity.terminal_revision.is_some()
                    && facts.unsent_input == Some(true)) =>
        {
            QueenRecoveryAssessment::Waiting
        }
        QueenRecoveryDisposition::AwaitDelivery { message_id }
            if !message_id.is_empty() && facts.pending_message_id.as_ref() == Some(message_id) =>
        {
            QueenRecoveryAssessment::Waiting
        }
        QueenRecoveryDisposition::AwaitOperator { decision_id }
            if facts.pending_decision_id == Some(*decision_id) =>
        {
            QueenRecoveryAssessment::Waiting
        }
        QueenRecoveryDisposition::VerifiedExternalWait { checked_evidence }
            if facts.verified_external_wait
                && !checked_evidence.trim().is_empty()
                && checked_evidence.len() <= 2000 =>
        {
            QueenRecoveryAssessment::Waiting
        }
        _ => QueenRecoveryAssessment::Uncovered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> QueenRecoveryFacts {
        QueenRecoveryFacts {
            identity: QueenRecoveryIdentity {
                task_id: TaskId::new(),
                worker_id: WorkerId::new(),
                session_id: WorkerSessionId::new(),
                attention_id: "attention".into(),
                evidence_revision: "a".repeat(64),
                terminal_revision: Some("b".repeat(64)),
            },
            terminal_is_current: true,
            activity: RecoveryTerminalActivity::Resting,
            operator_engaged: false,
            unsent_input: Some(false),
            pending_message_id: None,
            pending_decision_id: None,
            verified_external_wait: false,
        }
    }

    #[test]
    fn resting_unknown_and_awaiting_operator_are_not_working() {
        let mut facts = facts();
        for activity in [
            RecoveryTerminalActivity::Resting,
            RecoveryTerminalActivity::Unknown,
            RecoveryTerminalActivity::AwaitingOperator,
        ] {
            facts.activity = activity;
            assert_eq!(
                assess_queen_recovery(
                    &facts.identity,
                    &facts,
                    &QueenRecoveryDisposition::ObservedWorking
                ),
                QueenRecoveryAssessment::Uncovered
            );
        }
        facts.activity = RecoveryTerminalActivity::Working;
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Working
        );
        facts.terminal_is_current = false;
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Uncovered
        );
    }

    #[test]
    fn suggestions_and_unknown_input_do_not_establish_operator_engagement() {
        let mut facts = facts();
        for input in [None, Some(false)] {
            facts.unsent_input = input;
            assert_eq!(
                assess_queen_recovery(
                    &facts.identity,
                    &facts,
                    &QueenRecoveryDisposition::ProtectOperatorInput
                ),
                QueenRecoveryAssessment::Uncovered
            );
        }
        facts.unsent_input = Some(true);
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::ProtectOperatorInput
            ),
            QueenRecoveryAssessment::Waiting
        );
    }

    #[test]
    fn delivered_or_replaced_requests_do_not_cover_a_pending_delivery_claim() {
        let mut facts = facts();
        let disposition = QueenRecoveryDisposition::AwaitDelivery {
            message_id: "request-1".into(),
        };
        facts.pending_message_id = Some("request-1".into());
        assert_eq!(
            assess_queen_recovery(&facts.identity, &facts, &disposition),
            QueenRecoveryAssessment::Waiting
        );
        for pending in [None, Some("request-2".into())] {
            facts.pending_message_id = pending;
            assert_eq!(
                assess_queen_recovery(&facts.identity, &facts, &disposition),
                QueenRecoveryAssessment::Uncovered
            );
        }
    }

    #[test]
    fn changed_session_or_evidence_invalidates_an_assessment() {
        let mut facts = facts();
        let expected = facts.identity.clone();
        facts.activity = RecoveryTerminalActivity::Working;
        facts.identity.session_id = WorkerSessionId::new();
        assert_eq!(
            assess_queen_recovery(
                &expected,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Uncovered
        );
        facts.identity = expected.clone();
        facts.identity.evidence_revision = "new-revision".into();
        assert_eq!(
            assess_queen_recovery(
                &expected,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Uncovered
        );
        facts.identity = expected.clone();
        facts.identity.terminal_revision = Some("c".repeat(64));
        assert_eq!(
            assess_queen_recovery(
                &expected,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Uncovered
        );
        facts.identity.terminal_revision = None;
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::ObservedWorking
            ),
            QueenRecoveryAssessment::Uncovered
        );
    }

    #[test]
    fn decisions_and_external_waits_require_current_verified_sources() {
        let mut facts = facts();
        let decision_id = DecisionRequestId::new();
        let disposition = QueenRecoveryDisposition::AwaitOperator { decision_id };
        assert_eq!(
            assess_queen_recovery(&facts.identity, &facts, &disposition),
            QueenRecoveryAssessment::Uncovered
        );
        facts.pending_decision_id = Some(decision_id);
        assert_eq!(
            assess_queen_recovery(&facts.identity, &facts, &disposition),
            QueenRecoveryAssessment::Waiting
        );
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::VerifiedExternalWait {
                    checked_evidence: "Checked fixture endpoint".into()
                }
            ),
            QueenRecoveryAssessment::Uncovered
        );
        facts.verified_external_wait = true;
        assert_eq!(
            assess_queen_recovery(
                &facts.identity,
                &facts,
                &QueenRecoveryDisposition::VerifiedExternalWait {
                    checked_evidence: "Checked fixture endpoint".into()
                }
            ),
            QueenRecoveryAssessment::Waiting
        );
    }
}
