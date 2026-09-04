use swarm_domain::{
    OpsIntegrationScope, OpsTicketInput, OpsTicketValidationError, Task, TaskActivityPage,
};
use swarm_persistence::{OpsDeploymentPage, OpsTicketReceipt, TaskStore, TaskStoreError};
use thiserror::Error;

/// External intake never borrows an agent principal or exposes agent commands.
/// The transport must resolve current, non-revoked scope for every operation.
#[derive(Clone)]
pub struct OpsTicketService {
    store: TaskStore,
}

#[derive(Debug, Error)]
pub enum OpsTicketError {
    #[error(transparent)]
    InvalidCommand(#[from] OpsTicketValidationError),
    #[error(transparent)]
    Store(#[from] TaskStoreError),
}

/// Internal application projection. The transport exposes only selected task
/// progress fields, never serializes the entire internal task as its contract.
pub struct OpsTicketProgress {
    pub task: Task,
    pub activity: TaskActivityPage,
    pub deployments: OpsDeploymentPage,
}

impl OpsTicketService {
    #[must_use]
    pub const fn new(store: TaskStore) -> Self {
        Self { store }
    }

    /// Files a reviewed request as an inert, attributed draft.
    ///
    /// # Errors
    /// Refuses commands outside current scope, malformed content, changed retries,
    /// or unavailable persistence. No worker is assigned or awakened.
    pub fn submit(
        &self,
        scope: &OpsIntegrationScope,
        input: OpsTicketInput,
    ) -> Result<OpsTicketReceipt, OpsTicketError> {
        let command = scope.authorize(input)?;
        Ok(self.store.submit_ops_ticket(&command)?)
    }

    /// Reads a bounded progress snapshot of one currently scoped source request.
    /// Closure and recorded deployment remain independent facts in the task.
    ///
    /// # Errors
    /// Refuses unknown, removed or out-of-scope tickets and unavailable storage.
    pub fn progress(
        &self,
        scope: &OpsIntegrationScope,
        app_id: &str,
        request_id: &str,
    ) -> Result<OpsTicketProgress, OpsTicketError> {
        let task = self.store.ops_ticket_task(scope, app_id, request_id)?;
        let activity = self.store.list_task_activity(task.id, 50)?;
        let deployments = self.store.ops_ticket_deployments(task.id)?;
        Ok(OpsTicketProgress {
            task,
            activity,
            deployments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{OpsAppBinding, TaskActivityActor, TaskPriority, TaskState};
    #[test]
    fn scoped_intake_is_inert_and_progress_is_bounded_without_mutating_work() {
        let store = TaskStore::in_memory().unwrap();
        let service = OpsTicketService::new(store.clone());
        let scope = OpsIntegrationScope {
            integration_id: "console".into(),
            bindings: vec![OpsAppBinding {
                app_id: "app-one".into(),
                workspace: "/work/one".into(),
            }],
        };
        let input = OpsTicketInput {
            app_id: "app-one".into(),
            request_id: "request-one".into(),
            conversation_id: "feedback:1".into(),
            title: "Calendar export".into(),
            description: "Reviewed scope".into(),
            priority: TaskPriority::Normal,
        };
        let receipt = service.submit(&scope, input.clone()).unwrap();
        for _ in 0..60 {
            store
                .append_task_correction(
                    receipt.task_id,
                    "Recorded progress",
                    &TaskActivityActor::operator(),
                )
                .unwrap();
        }
        let progress = service.progress(&scope, "app-one", "request-one").unwrap();
        assert_eq!(progress.task.state, TaskState::Draft);
        assert!(progress.task.assigned_worker_id.is_none());
        assert!(!progress.task.deployment_recorded);
        assert!(progress.activity.truncated);
        assert_eq!(progress.activity.events.len(), 50);
        assert_eq!(
            service
                .progress(&scope, "app-one", "request-one")
                .unwrap()
                .task,
            progress.task
        );
        let mut revoked_scope = scope;
        revoked_scope.bindings.clear();
        assert!(service.submit(&revoked_scope, input).is_err());
        assert!(
            service
                .progress(&revoked_scope, "app-one", "request-one")
                .is_err()
        );
    }
}
