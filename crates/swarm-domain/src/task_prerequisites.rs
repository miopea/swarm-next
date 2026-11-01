use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{NextMoveOwner, TaskId, TaskState, WorkerId};

pub const MAX_TASK_PREREQUISITES: usize = 32;
pub const MAX_HIVE_PREREQUISITES: usize = 4096;
pub const MAX_PREREQUISITE_REASON_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrerequisiteOperation {
    Add,
    Remove,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPrerequisiteChange {
    pub task_id: TaskId,
    pub prerequisite_id: TaskId,
    pub operation: PrerequisiteOperation,
    pub reason: String,
}

/// Recorded relation and current prerequisite facts, not inferred queue order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskPrerequisite {
    pub task_id: TaskId,
    pub prerequisite_id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub assigned_worker_id: Option<WorkerId>,
    pub removed: bool,
    pub reason: String,
    pub created_at: i64,
}

impl TaskPrerequisite {
    #[must_use]
    pub fn satisfied(&self) -> bool {
        !self.removed && self.state == TaskState::Completed
    }

    /// Not finished, and still able to finish. An abandoned or removed
    /// prerequisite never will, so it holds nothing: the task is Queen's to
    /// re-route, as the wake count already says.
    #[must_use]
    pub fn outstanding(&self) -> bool {
        !self.removed && !matches!(self.state, TaskState::Completed | TaskState::Abandoned)
    }
}

impl NextMoveOwner {
    /// Review may wait on another task without losing its finished-work state.
    ///
    /// So may a Draft (ADR 0135). A draft cannot move to Blocked, so one filed
    /// with an unfinished prerequisite stayed Queen's to triage, and no
    /// assessment could cover it while it waited. Every Queen run then ended
    /// incomplete. It comes back to Queen when the prerequisite finishes.
    ///
    /// ⚠️ ONLY WORK THAT CAN STILL FINISH HOLDS IT. A prerequisite that was
    /// abandoned or removed never completes, so waiting on it would take the
    /// task off Queen's list for good while the wake count still names it.
    #[must_use]
    pub fn after_unfinished_prerequisites(
        self,
        state: TaskState,
        prerequisites: &[TaskPrerequisite],
    ) -> Self {
        if matches!(state, TaskState::Review | TaskState::Draft)
            && self != Self::Operator
            && prerequisites.iter().any(TaskPrerequisite::outstanding)
        {
            Self::Blocked
        } else {
            self
        }
    }
    /// Without a current structured gate Queen must reassess the block.
    /// This assigns verification, not permission to resume or override a note.
    #[must_use]
    pub fn after_blocker_evidence(
        self,
        awaiting_operator: bool,
        prerequisites: &[TaskPrerequisite],
        future_hold: bool,
    ) -> Self {
        if self == Self::Blocked
            && !awaiting_operator
            && !future_hold
            && prerequisites.iter().all(TaskPrerequisite::satisfied)
        {
            Self::Queen
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskPrerequisiteError {
    InvalidReason,
    MustBeBlocked,
    SelfReference,
    Cycle,
    Capacity,
    Conflict,
    Unresolved,
    Unauthorized,
}

impl std::fmt::Display for TaskPrerequisiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidReason => "A prerequisite change needs a nonempty reason of at most 2048 bytes",
            Self::MustBeBlocked => "Finished work cannot gain a prerequisite: a completed or abandoned task's record is settled. Any unfinished state may name what it waits on",
            Self::SelfReference => "A task cannot depend on itself",
            Self::Cycle => "This prerequisite would create a dependency cycle",
            Self::Capacity => "The bounded prerequisite graph is full; remove obsolete links before adding more",
            Self::Conflict => "This prerequisite already has a different reason; remove it explicitly before replacing it",
            Self::Unresolved => "This task still has unresolved prerequisites; Queen must reconcile them before resuming work",
            Self::Unauthorized => "Only Queen or the operator may change task prerequisites",
        })
    }
}

impl std::error::Error for TaskPrerequisiteError {}

/// Validate a proposed new edge against one consistent, bounded Hive graph.
/// Persistence owns existence, Hive identity and atomicity; graph policy is here.
///
/// # Errors
/// Rejects invalid text, state, self-links, cycles and exhausted graph capacity.
pub fn validate_task_prerequisite(
    task_id: TaskId,
    state: TaskState,
    prerequisite_id: TaskId,
    reason: &str,
    edges: &[(TaskId, TaskId)],
) -> Result<(), TaskPrerequisiteError> {
    validate_prerequisite_reason(reason)?;
    // ⚠️ THE RULE IS ABOUT FINISHED WORK, NOT ABOUT BEING BLOCKED, and reading
    // it the other way produced a deadlock. Blocked work must now name what it
    // waits for BEFORE it may be parked, while a prerequisite could only be
    // added AFTER parking — so neither could happen first. A test caught it;
    // nothing in production would have, because the only symptom is a refusal
    // that looks reasonable in isolation.
    //
    // "This waits on that" is true regardless of which state the work is in.
    // What must stay protected is work that has FINISHED: a completed or
    // abandoned task gaining a dependency would rewrite a settled record.
    if matches!(state, TaskState::Completed | TaskState::Abandoned) {
        return Err(TaskPrerequisiteError::MustBeBlocked);
    }
    if task_id == prerequisite_id {
        return Err(TaskPrerequisiteError::SelfReference);
    }
    if edges.len() >= MAX_HIVE_PREREQUISITES
        || edges
            .iter()
            .filter(|(source, _)| *source == task_id)
            .count()
            >= MAX_TASK_PREREQUISITES
    {
        return Err(TaskPrerequisiteError::Capacity);
    }
    let mut graph: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    for &(source, target) in edges {
        graph.entry(source).or_default().push(target);
    }
    let mut seen = HashSet::new();
    let mut pending = vec![prerequisite_id];
    while let Some(current) = pending.pop() {
        if current == task_id {
            return Err(TaskPrerequisiteError::Cycle);
        }
        if !seen.insert(current) {
            continue;
        }
        if seen.len() > MAX_HIVE_PREREQUISITES {
            return Err(TaskPrerequisiteError::Capacity);
        }
        if let Some(next) = graph.get(&current) {
            pending.extend(next.iter().copied());
        }
    }
    Ok(())
}

/// # Errors
/// Rejects an empty or oversized change explanation.
pub fn validate_prerequisite_reason(reason: &str) -> Result<(), TaskPrerequisiteError> {
    if reason.trim().is_empty() || reason.len() > MAX_PREREQUISITE_REASON_BYTES {
        return Err(TaskPrerequisiteError::InvalidReason);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerequisites_reject_cycles_without_conflating_queue_order() {
        let (a, b, c) = (TaskId::new(), TaskId::new(), TaskId::new());
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, a, "Self", &[]),
            Err(TaskPrerequisiteError::SelfReference)
        );
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, b, "Contract", &[(b, c), (c, a)]),
            Err(TaskPrerequisiteError::Cycle)
        );
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, b, "Contract", &[(b, c)]),
            Ok(())
        );
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Review, b, "Contract", &[]),
            Ok(())
        );
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, b, "  ", &[]),
            Err(TaskPrerequisiteError::InvalidReason)
        );
    }

    #[test]
    fn prerequisite_capacity_is_explicit() {
        let (a, b) = (TaskId::new(), TaskId::new());
        let edges: Vec<_> = (0..MAX_TASK_PREREQUISITES)
            .map(|_| (a, TaskId::new()))
            .collect();
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, b, "Contract", &edges),
            Err(TaskPrerequisiteError::Capacity)
        );
        let edges = vec![(b, TaskId::new()); MAX_HIVE_PREREQUISITES];
        assert_eq!(
            validate_task_prerequisite(a, TaskState::Blocked, b, "Contract", &edges),
            Err(TaskPrerequisiteError::Capacity)
        );
    }

    #[test]
    fn only_present_completed_work_satisfies_a_prerequisite() {
        let mut prerequisite = TaskPrerequisite {
            task_id: TaskId::new(),
            prerequisite_id: TaskId::new(),
            title: "Contract".to_owned(),
            state: TaskState::Completed,
            assigned_worker_id: None,
            removed: false,
            reason: "Required".to_owned(),
            created_at: 1,
        };
        assert!(prerequisite.satisfied());
        prerequisite.removed = true;
        assert!(!prerequisite.satisfied());
        prerequisite.removed = false;
        for state in [
            TaskState::Abandoned,
            TaskState::Ready,
            TaskState::Review,
            TaskState::AwaitingRelease,
        ] {
            prerequisite.state = state;
            assert!(!prerequisite.satisfied());
        }
    }

    #[test]
    fn reassessment_owner_preserves_operator_and_current_gates() {
        let empty = [];
        assert_eq!(
            NextMoveOwner::Blocked.after_blocker_evidence(false, &empty, false),
            NextMoveOwner::Queen
        );
        assert_eq!(
            NextMoveOwner::Blocked.after_blocker_evidence(false, &empty, true),
            NextMoveOwner::Blocked
        );
        assert_eq!(
            NextMoveOwner::Operator.after_blocker_evidence(true, &empty, false),
            NextMoveOwner::Operator
        );
        for owner in [
            NextMoveOwner::Worker,
            NextMoveOwner::Release,
            NextMoveOwner::Nobody,
        ] {
            assert_eq!(owner.after_blocker_evidence(false, &empty, false), owner);
        }
    }

    /// A draft cannot move to Blocked, so before ADR 0135 a draft filed behind
    /// an unfinished task stayed Queen's, uncoverable, and every run read
    /// incomplete. It now waits like Review does and returns when the
    /// prerequisite completes.
    #[test]
    fn a_draft_behind_unfinished_work_waits_and_returns_when_it_finishes() {
        let mut prerequisite = TaskPrerequisite {
            task_id: TaskId::new(),
            prerequisite_id: TaskId::new(),
            title: "Contract".to_owned(),
            state: TaskState::Active,
            assigned_worker_id: None,
            removed: false,
            reason: "Required".to_owned(),
            created_at: 1,
        };
        for state in [TaskState::Draft, TaskState::Review] {
            assert_eq!(
                NextMoveOwner::Queen
                    .after_unfinished_prerequisites(state, std::slice::from_ref(&prerequisite)),
                NextMoveOwner::Blocked,
                "{state:?} waits on unfinished work"
            );
            assert_eq!(
                NextMoveOwner::Operator
                    .after_unfinished_prerequisites(state, std::slice::from_ref(&prerequisite)),
                NextMoveOwner::Operator,
                "a pending operator ruling still owns {state:?}"
            );
        }
        assert_eq!(
            NextMoveOwner::Queen.after_unfinished_prerequisites(TaskState::Draft, &[]),
            NextMoveOwner::Queen,
            "a draft with nothing to wait on is Queen's to triage"
        );
        assert_eq!(
            NextMoveOwner::Worker.after_unfinished_prerequisites(
                TaskState::Ready,
                std::slice::from_ref(&prerequisite)
            ),
            NextMoveOwner::Worker,
            "other states keep their owner"
        );
        prerequisite.state = TaskState::Completed;
        assert_eq!(
            NextMoveOwner::Queen.after_unfinished_prerequisites(
                TaskState::Draft,
                std::slice::from_ref(&prerequisite)
            ),
            NextMoveOwner::Queen,
            "the draft comes back to Queen once its prerequisite finishes"
        );

        // Work that will never finish holds nothing: Queen must re-route it.
        prerequisite.state = TaskState::Abandoned;
        for state in [TaskState::Draft, TaskState::Review] {
            assert_eq!(
                NextMoveOwner::Queen
                    .after_unfinished_prerequisites(state, std::slice::from_ref(&prerequisite)),
                NextMoveOwner::Queen,
                "an abandoned prerequisite does not strand {state:?}"
            );
        }
        prerequisite.state = TaskState::Active;
        prerequisite.removed = true;
        assert_eq!(
            NextMoveOwner::Queen.after_unfinished_prerequisites(
                TaskState::Draft,
                std::slice::from_ref(&prerequisite)
            ),
            NextMoveOwner::Queen,
            "a removed prerequisite does not strand the draft"
        );
    }
}
