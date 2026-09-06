use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen};
use swarm_domain::{Task, TaskActivityActor, TaskBlockReassessment};

impl TaskService {
    /// Repair a current block without changing task/worker lifecycle.
    ///
    /// # Errors
    /// Refuses ordinary workers, stale observations or invalid reassessment.
    pub fn reassess_task_block(
        &self,
        principal: AgentPrincipal,
        input: &TaskBlockReassessment,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        Ok(self.store.reassess_task_block(
            input,
            &TaskActivityActor::worker(principal.worker_id),
            now,
        )?)
    }
}
