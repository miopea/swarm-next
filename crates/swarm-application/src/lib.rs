use swarm_domain::{
    Task, TaskId, TaskPriority, TaskState, WorkerId, WorkerProfile, WorkerRole, WorkerSessionId,
};
use swarm_persistence::{TaskStore, TaskStoreError};
use thiserror::Error;

/// The durable agent identity resolved before an application command is invoked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentPrincipal {
    pub worker_id: WorkerId,
    pub role: WorkerRole,
    pub active_session_id: Option<WorkerSessionId>,
}

impl From<&WorkerProfile> for AgentPrincipal {
    fn from(profile: &WorkerProfile) -> Self {
        Self {
            worker_id: profile.id,
            role: profile.role,
            active_session_id: profile.active_session_id,
        }
    }
}

#[derive(Clone)]
pub struct TaskService {
    store: TaskStore,
}

impl TaskService {
    #[must_use]
    pub const fn new(store: TaskStore) -> Self {
        Self { store }
    }

    #[must_use]
    pub const fn store(&self) -> &TaskStore {
        &self.store
    }

    /// Lists the complete local Hive queue for an operator or Queen coordinator.
    ///
    /// # Errors
    /// Returns a persistence error when the task snapshot cannot be read.
    pub fn list_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_tasks().map_err(Into::into)
    }

    /// Creates a validated local draft through the shared application boundary.
    ///
    /// # Errors
    /// Propagates validation or persistence failures.
    pub fn create_operator_task(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, ApplicationError> {
        self.store
            .create_task_with_details(title, description, priority, workspace)
            .map_err(Into::into)
    }

    /// Updates supplied task details through the shared application boundary.
    ///
    /// # Errors
    /// Propagates validation and persistence failures.
    pub fn update_operator_task(
        &self,
        task_id: TaskId,
        update: &swarm_domain::TaskDetailsUpdate,
    ) -> Result<Task, ApplicationError> {
        self.store
            .update_task_details(task_id, update)
            .map_err(Into::into)
    }
    /// Assigns a local task to a running session already verified by the calling adapter.
    ///
    /// # Errors
    /// Propagates task lifecycle and persistence failures.
    pub fn assign_operator_task(
        &self,
        task_id: TaskId,
        session_id: WorkerSessionId,
    ) -> Result<Task, ApplicationError> {
        self.store
            .assign_task(task_id, session_id)
            .map_err(Into::into)
    }

    /// Applies one domain-valid task transition for the operator or Queen.
    ///
    /// # Errors
    /// Propagates lifecycle and persistence failures.
    pub fn transition_operator_task(
        &self,
        task_id: TaskId,
        target: TaskState,
    ) -> Result<Task, ApplicationError> {
        self.store
            .transition_task(task_id, target)
            .map_err(Into::into)
    }
    /// Lists the work visible to an agent. Queen sees the Hive queue; workers see only their
    /// current session assignment.
    ///
    /// # Errors
    /// Returns a persistence error when the task snapshot cannot be read.
    pub fn list_visible_tasks(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<Task>, ApplicationError> {
        let tasks = self.list_tasks()?;
        if principal.role == WorkerRole::Queen {
            return Ok(tasks);
        }
        Ok(principal
            .active_session_id
            .map_or_else(Vec::new, |session_id| {
                tasks
                    .into_iter()
                    .filter(|task| task.assigned_session_id == Some(session_id))
                    .collect()
            }))
    }

    /// Lists the local roster for Queen coordination.
    ///
    /// # Errors
    /// Denies worker callers and propagates persistence failures.
    pub fn list_workers(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<WorkerProfile>, ApplicationError> {
        require_queen(principal)?;
        self.store.list_worker_profiles().map_err(Into::into)
    }

    /// Creates a draft in the local Hive. Only Queen may originate durable work through MCP.
    ///
    /// # Errors
    /// Denies worker callers and propagates validation or persistence failures.
    pub fn create_task(
        &self,
        principal: AgentPrincipal,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        self.create_operator_task(title, description, priority, workspace)
    }

    /// Assigns a task to the active process incarnation of a stable worker.
    ///
    /// # Errors
    /// Denies worker callers, sleeping targets, and invalid persistence changes.
    pub fn assign_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        worker_id: WorkerId,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        let worker = self.store.get_worker_profile(worker_id)?;
        let session_id = worker
            .active_session_id
            .ok_or(ApplicationError::WorkerNotRunning)?;
        self.assign_operator_task(task_id, session_id)
    }

    /// Applies a domain-valid state transition within the caller's authority.
    ///
    /// Workers may report progress only for their own assignment and cannot approve completion.
    /// Queen may apply any transition accepted by the task lifecycle.
    ///
    /// # Errors
    /// Denies foreign assignments or worker completion and propagates domain failures.
    pub fn transition_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        target: TaskState,
    ) -> Result<Task, ApplicationError> {
        if principal.role != WorkerRole::Queen {
            let session_id = principal
                .active_session_id
                .ok_or(ApplicationError::WorkerNotRunning)?;
            let task = self.store.get_task(task_id)?;
            if task.assigned_session_id != Some(session_id) {
                return Err(ApplicationError::NotAuthorized);
            }
            if !matches!(
                target,
                TaskState::Active | TaskState::Blocked | TaskState::Review
            ) {
                return Err(ApplicationError::NotAuthorized);
            }
        }
        self.transition_operator_task(task_id, target)
    }
}

fn require_queen(principal: AgentPrincipal) -> Result<(), ApplicationError> {
    if principal.role == WorkerRole::Queen {
        Ok(())
    } else {
        Err(ApplicationError::NotAuthorized)
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("this agent is not authorized for that outcome")]
    NotAuthorized,
    #[error("the target worker does not have an active session")]
    WorkerNotRunning,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn setup() -> (TaskService, WorkerProfile, WorkerProfile) {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        (TaskService::new(store), queen, worker)
    }

    #[test]
    fn worker_visibility_is_limited_to_its_active_assignment() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let mine = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Mine",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let other = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Other",
                "",
                TaskPriority::Normal,
                "/workspace/other",
            )
            .unwrap();
        service
            .assign_task(AgentPrincipal::from(&queen), mine.id, worker.id)
            .unwrap();

        let current = service.store().get_worker_profile(worker.id).unwrap();
        assert_eq!(
            service
                .list_visible_tasks(AgentPrincipal::from(&current))
                .unwrap(),
            [service.store().get_task(mine.id).unwrap()]
        );
        assert!(service.store().get_task(other.id).is_ok());
    }

    #[test]
    fn worker_cannot_create_assign_or_approve_completion() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let running_worker = service.store().get_worker_profile(worker.id).unwrap();
        let worker_principal = AgentPrincipal::from(&running_worker);
        let task = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Guarded",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .assign_task(AgentPrincipal::from(&queen), task.id, worker.id)
            .unwrap();

        assert!(matches!(
            service.create_task(
                worker_principal,
                "Nope",
                "",
                TaskPriority::Normal,
                &worker.workspace
            ),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(matches!(
            service.assign_task(worker_principal, task.id, worker.id),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(matches!(
            service.transition_task(worker_principal, task.id, TaskState::Completed),
            Err(ApplicationError::NotAuthorized)
        ));
    }
}
