use crate::{DecisionRequestId, DecisionRequestState, TaskId, TaskState};

pub const MAX_DECISION_TASK_LINKS: usize = 32;
pub const MAX_TASK_DECISION_LINKS: usize = 32;
pub const MAX_HIVE_DECISION_LINKS: usize = 4096;
pub const MAX_DECISION_LINK_REASON_BYTES: usize = 2048;

/// A blocker reference, never propagation of an operator's command permission.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDecisionLink {
    pub task_id: TaskId,
    pub decision_id: DecisionRequestId,
    pub reason: String,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDecisionLinkOperation {
    Add,
    Remove,
}

/// Authenticated application command. A link never broadens approval scope.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDecisionLinkChange {
    pub task_id: TaskId,
    pub decision_id: DecisionRequestId,
    pub operation: TaskDecisionLinkOperation,
    pub reason: String,
    pub expected_evidence_revision: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskDecisionLinkError {
    InvalidReason,
    DecisionNotPending,
    TaskFinished,
    Capacity,
    Conflict,
    StaleEvidence,
    Unauthorized,
}

impl std::fmt::Display for TaskDecisionLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidReason => "A decision link needs a nonempty reason of at most 2048 bytes",
            Self::DecisionNotPending => "Only a pending or authenticated resolved decision can gain a task link",
            Self::TaskFinished => "Finished work cannot gain a decision blocker",
            Self::Capacity => "Decision-link capacity reached; explicitly remove obsolete links first",
            Self::Conflict => "This decision link has a different reason; remove it explicitly before replacing it",
            Self::StaleEvidence => "Task or decision evidence changed; read the current blockers before changing links",
            Self::Unauthorized => "Only Queen or the operator may change shared decision blockers",
        })
    }
}

impl std::error::Error for TaskDecisionLinkError {}

/// Validate a new relation after persistence checks identity and exact replay.
/// Counts must come from the same transaction that inserts the relation.
///
/// # Errors
/// Rejects settled targets, invalid reasons and exhausted bounded capacity.
pub fn validate_task_decision_link(
    task_state: TaskState,
    decision_state: DecisionRequestState,
    reason: &str,
    decision_links: usize,
    task_links: usize,
    hive_links: usize,
) -> Result<(), TaskDecisionLinkError> {
    validate_decision_link_reason(reason)?;
    if !matches!(
        decision_state,
        DecisionRequestState::Pending | DecisionRequestState::Resolved
    ) {
        return Err(TaskDecisionLinkError::DecisionNotPending);
    }
    if matches!(task_state, TaskState::Completed | TaskState::Abandoned) {
        return Err(TaskDecisionLinkError::TaskFinished);
    }
    if decision_links >= MAX_DECISION_TASK_LINKS
        || task_links >= MAX_TASK_DECISION_LINKS
        || hive_links >= MAX_HIVE_DECISION_LINKS
    {
        return Err(TaskDecisionLinkError::Capacity);
    }
    Ok(())
}

/// # Errors
/// Refuses empty, oversized or control-bearing explanations.
pub fn validate_decision_link_reason(reason: &str) -> Result<(), TaskDecisionLinkError> {
    if reason.trim().is_empty()
        || reason.len() > MAX_DECISION_LINK_REASON_BYTES
        || reason
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(TaskDecisionLinkError::InvalidReason);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_decision_link_accepts_unfinished_work_without_transitioning_it() {
        for state in [
            TaskState::Draft,
            TaskState::Ready,
            TaskState::Active,
            TaskState::Blocked,
            TaskState::Review,
            TaskState::AwaitingRelease,
        ] {
            assert_eq!(
                validate_task_decision_link(
                    state,
                    DecisionRequestState::Pending,
                    "Needs the same operator-provided session",
                    31,
                    31,
                    4095
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn shared_decision_link_accepts_resolved_evidence_but_not_withdrawn_requests() {
        assert_eq!(
            validate_task_decision_link(
                TaskState::Blocked,
                DecisionRequestState::Resolved,
                "Same original scope",
                0,
                0,
                0
            ),
            Ok(())
        );
        for state in [DecisionRequestState::Withdrawn] {
            assert_eq!(
                validate_task_decision_link(TaskState::Ready, state, "Shared gate", 0, 0, 0),
                Err(TaskDecisionLinkError::DecisionNotPending)
            );
        }
        for state in [TaskState::Completed, TaskState::Abandoned] {
            assert_eq!(
                validate_task_decision_link(
                    state,
                    DecisionRequestState::Pending,
                    "Shared gate",
                    0,
                    0,
                    0
                ),
                Err(TaskDecisionLinkError::TaskFinished)
            );
        }
    }

    #[test]
    fn shared_decision_link_has_independent_capacity_limits() {
        for counts in [(32, 0, 0), (0, 32, 0), (0, 0, 4096), (usize::MAX, 0, 0)] {
            assert_eq!(
                validate_task_decision_link(
                    TaskState::Review,
                    DecisionRequestState::Pending,
                    "Shared gate",
                    counts.0,
                    counts.1,
                    counts.2
                ),
                Err(TaskDecisionLinkError::Capacity)
            );
        }
    }

    #[test]
    fn shared_decision_link_reasons_are_bounded_without_normalization() {
        for reason in [
            " ".into(),
            "\0bad".into(),
            "x".repeat(2049),
            "é".repeat(1025),
        ] {
            assert_eq!(
                validate_decision_link_reason(&reason),
                Err(TaskDecisionLinkError::InvalidReason)
            );
        }
        assert_eq!(validate_decision_link_reason(&"é".repeat(1024)), Ok(()));
        assert_eq!(
            validate_decision_link_reason("Exact reason\nwith a second line"),
            Ok(())
        );
    }
}
