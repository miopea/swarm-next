use serde::{Deserialize, Serialize};

use crate::{QueenAutomationOutcome, QueenAutomationTrigger};

/// Content-free facts about one accepted explicit finish, not task success.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueenRunEvidence {
    pub run_id: String,
    /// Build serving the finish; a run may span App/API replacements.
    pub finished_on_build: Option<String>,
    pub trigger: QueenAutomationTrigger,
    pub requested_at: Option<i64>,
    pub delivered_at: Option<i64>,
    pub finished_at: i64,
    pub attempts: u32,
    pub initial_actionable_count: u32,
    pub requested_outcome: QueenAutomationOutcome,
    pub accepted_outcome: QueenAutomationOutcome,
}

impl QueenRunEvidence {
    /// Queue time through recorded delivery; missing/inconsistent clocks are unknown.
    #[must_use]
    pub fn delivery_wait_seconds(&self) -> Option<u64> {
        nonnegative_interval(self.requested_at?, self.delivered_at?)
    }

    /// Recorded delivery through finish, including any interruptions/continuations.
    #[must_use]
    pub fn delivered_to_finish_seconds(&self) -> Option<u64> {
        nonnegative_interval(self.delivered_at?, self.finished_at)
    }
}

fn nonnegative_interval(start: i64, end: i64) -> Option<u64> {
    if start < 0 || end < 0 {
        return None;
    }
    end.checked_sub(start)
        .and_then(|value| u64::try_from(value).ok())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueenRunHistory {
    pub retention_days: u32,
    pub max_retained: u32,
    pub retained_count: u32,
    pub records: Vec<QueenRunEvidence>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> QueenRunEvidence {
        QueenRunEvidence {
            run_id: "run".into(),
            finished_on_build: None,
            trigger: QueenAutomationTrigger::ActionableWork,
            requested_at: Some(10),
            delivered_at: Some(20),
            finished_at: 30,
            attempts: 1,
            initial_actionable_count: 3,
            requested_outcome: QueenAutomationOutcome::Completed,
            accepted_outcome: QueenAutomationOutcome::Incomplete,
        }
    }

    #[test]
    fn timing_preserves_queue_and_execution_boundaries() {
        let evidence = sample();
        assert_eq!(evidence.delivery_wait_seconds(), Some(10));
        assert_eq!(evidence.delivered_to_finish_seconds(), Some(10));
        assert_ne!(evidence.requested_outcome, evidence.accepted_outcome);
    }

    #[test]
    fn absent_or_inconsistent_clocks_are_not_zero_duration() {
        let mut evidence = sample();
        evidence.requested_at = None;
        assert_eq!(evidence.delivery_wait_seconds(), None);
        evidence.delivered_at = Some(31);
        assert_eq!(evidence.delivered_to_finish_seconds(), None);
        evidence.delivered_at = Some(-1);
        assert_eq!(evidence.delivered_to_finish_seconds(), None);
        evidence.delivered_at = Some(30);
        assert_eq!(evidence.delivered_to_finish_seconds(), Some(0));
    }
}
