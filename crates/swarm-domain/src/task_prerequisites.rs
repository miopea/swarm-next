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
}

impl NextMoveOwner {
    /// Completed prerequisites return the next move to Queen for reassessment,
    /// without changing lifecycle or overriding an outstanding operator request.
    #[must_use]
    pub fn after_prerequisites(
        self,
        awaiting_operator: bool,
        prerequisites: &[TaskPrerequisite],
    ) -> Self {
        if self == Self::Blocked
            && !awaiting_operator
            && !prerequisites.is_empty()
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
            Self::MustBeBlocked => "Only blocked work can gain a prerequisite; record its actual block first",
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
    if state != TaskState::Blocked {
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
            Err(TaskPrerequisiteError::MustBeBlocked)
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
}
