use serde::{Deserialize, Serialize};

use crate::TaskId;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskBlockReassessment {
    pub task_id: TaskId,
    pub expected_activity_sequence: i64,
    pub reason: String,
    pub evidence: String,
    pub not_before: Option<i64>,
}

impl TaskBlockReassessment {
    /// Validate an explicit in-place reassessment, never infer a hold from prose.
    ///
    /// # Errors
    /// Refuses unbounded/empty content, invalid revision or a non-future hold.
    pub fn validate(&self, now: i64) -> Result<(), &'static str> {
        if self.expected_activity_sequence <= 0 {
            return Err("read current task history and supply its latest activity sequence");
        }
        if self.reason.trim().is_empty() || self.reason.len() > 1000 {
            return Err("current blocker reason must contain 1 to 1000 bytes");
        }
        if self.evidence.trim().is_empty() || self.evidence.len() > 2000 {
            return Err("reassessment evidence must contain 1 to 2000 bytes");
        }
        if self.not_before.is_some_and(|date| date <= now) {
            return Err("not_before must be a verified future time, or null for no scheduled hold");
        }
        Ok(())
    }
}
