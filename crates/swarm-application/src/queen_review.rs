use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen};
use swarm_domain::{
    QueenReviewDispositionInput, QueenTaskReviewEvidence, TaskActivityActor, TaskId,
    VerifiedQueenReviewReceipt,
};

impl TaskService {
    /// Read consistent facts for an explicit Queen assessment.
    ///
    /// # Errors
    /// Refuses non-Queen callers or unavailable evidence.
    pub fn read_queen_review_evidence(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<QueenTaskReviewEvidence, ApplicationError> {
        require_queen(principal)?;
        Ok(self.store.queen_task_review_snapshot(task_id)?)
    }

    /// Record checked waiting evidence without granting authority or resuming work.
    ///
    /// # Errors
    /// Refuses unauthorized callers, stale evidence and invalid source references.
    pub fn record_queen_review_disposition(
        &self,
        principal: AgentPrincipal,
        input: &QueenReviewDispositionInput,
        now: i64,
    ) -> Result<VerifiedQueenReviewReceipt, ApplicationError> {
        require_queen(principal)?;
        Ok(self.store.record_queen_review_disposition(
            input,
            &TaskActivityActor::worker(principal.worker_id),
            now,
        )?)
    }
}
