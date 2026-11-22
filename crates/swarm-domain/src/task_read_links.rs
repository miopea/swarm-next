//! Read links (ADR 0182): another task a task's worker may read.
//!
//! A worker reads only its own work. When its task depends on rules or
//! findings recorded on another task, often in another repository, it was
//! refused that task and rebuilt the rules from Queen's summary. A read link
//! names that other task. Only Queen or the operator sets one: a worker edits
//! its own task's text, and email and Jira text become task text, so a rule
//! keyed on "any task the text names" would open any task.

use serde::{Deserialize, Serialize};

use crate::{TaskId, TaskState};

/// At most this many read links on one task.
pub const MAX_TASK_READ_LINKS: usize = 32;
/// At most this many read links in one Hive.
pub const MAX_HIVE_READ_LINKS: usize = 4096;
pub const MAX_READ_LINK_REASON_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadLinkOperation {
    Add,
    Remove,
}

/// Queen's or the operator's request to add or remove one read link.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReadLinkChange {
    /// The task whose worker may read.
    pub task_id: TaskId,
    /// The task it may read.
    pub readable_task_id: TaskId,
    pub operation: ReadLinkOperation,
    pub reason: String,
}

/// A task this task's worker may read, as that task stands now.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskReadLink {
    pub readable_task_id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub workspace: String,
    pub reason: String,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskReadLinkError {
    InvalidReason,
    SelfReference,
    Capacity,
    Conflict,
    Unauthorized,
}

impl std::fmt::Display for TaskReadLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidReason => "a read link needs a reason of 1 to 2048 bytes",
            Self::SelfReference => {
                "a task cannot be linked to read itself: its worker already reads it"
            }
            Self::Capacity => "a task holds at most 32 read links, and a Hive at most 4096",
            Self::Conflict => {
                "this read link already exists with a different reason; remove it first to change the reason"
            }
            Self::Unauthorized => "only Queen or the operator sets a read link",
        })
    }
}

impl std::error::Error for TaskReadLinkError {}

/// Checks what a change can be checked for without the store.
///
/// # Errors
/// Refuses an empty or oversized reason, and a task linked to itself.
pub fn validate_task_read_link(change: &TaskReadLinkChange) -> Result<(), TaskReadLinkError> {
    let reason = change.reason.trim();
    if reason.is_empty() || reason.len() > MAX_READ_LINK_REASON_BYTES {
        return Err(TaskReadLinkError::InvalidReason);
    }
    if change.task_id == change.readable_task_id {
        return Err(TaskReadLinkError::SelfReference);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(task: TaskId, readable: TaskId, reason: &str) -> TaskReadLinkChange {
        TaskReadLinkChange {
            task_id: task,
            readable_task_id: readable,
            operation: ReadLinkOperation::Add,
            reason: reason.into(),
        }
    }

    #[test]
    fn a_link_to_another_task_with_a_reason_is_valid() {
        let link = change(
            TaskId::new(),
            TaskId::new(),
            "Holds the rules this work follows.",
        );
        assert_eq!(validate_task_read_link(&link), Ok(()));
    }

    #[test]
    fn a_link_needs_a_reason_within_its_bound() {
        let (task, readable) = (TaskId::new(), TaskId::new());
        assert_eq!(
            validate_task_read_link(&change(task, readable, "  ")),
            Err(TaskReadLinkError::InvalidReason)
        );
        let long = "x".repeat(MAX_READ_LINK_REASON_BYTES + 1);
        assert_eq!(
            validate_task_read_link(&change(task, readable, &long)),
            Err(TaskReadLinkError::InvalidReason)
        );
    }

    #[test]
    fn a_task_cannot_be_linked_to_itself() {
        let task = TaskId::new();
        assert_eq!(
            validate_task_read_link(&change(task, task, "Itself.")),
            Err(TaskReadLinkError::SelfReference)
        );
    }

    #[test]
    fn a_change_names_its_fields_exactly() {
        let parsed: Result<TaskReadLinkChange, _> = serde_json::from_value(serde_json::json!({
            "task_id": TaskId::new(), "readable_task_id": TaskId::new(),
            "operation": "add", "reason": "Why.", "write": true
        }));
        assert!(parsed.is_err(), "an unknown field must not be accepted");
    }
}
