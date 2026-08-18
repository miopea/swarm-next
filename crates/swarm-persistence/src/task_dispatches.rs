use std::str::FromStr;

use rusqlite::params;
use swarm_domain::{TaskDispatchState, TaskId, TaskPriority, WorkerId, WorkerSessionId};

use super::{TaskStore, TaskStoreError, insert_control_room_event};
use swarm_domain::ControlRoomEventKind;

const MAX_DISPATCH_CLAIMS: i64 = 16;
const MAX_DISPATCH_ATTEMPTS: i64 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDispatch {
    pub assignment_id: String,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub workspace: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskDispatchFailure {
    Retryable,
    Uncertain,
}

impl TaskStore {
    /// Atomically claims a bounded batch of current assignments whose worker is quiet.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn claim_task_dispatches(&self, now: i64) -> Result<Vec<TaskDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT td.assignment_id, td.task_id, td.worker_id, a.worker_session_id,
                        t.title, t.description, t.priority, t.workspace
                 FROM task_dispatches td
                 JOIN task_assignments a ON a.id = td.assignment_id AND a.released_at IS NULL
                 JOIN tasks t ON t.id = td.task_id
                 JOIN worker_sessions ws ON ws.session_id = a.worker_session_id
                     AND ws.worker_id = td.worker_id AND ws.ended_at IS NULL
                 WHERE td.state = 'queued' AND t.removed_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements e
                       WHERE e.worker_id = td.worker_id AND e.expires_at > ?1
                   )
                 ORDER BY td.updated_at, td.assignment_id
                 LIMIT ?2",
            )?;
            statement
                .query_map(params![now, MAX_DISPATCH_CLAIMS], |row| {
                    let priority = TaskPriority::from_str(&row.get::<_, String>(6)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(TaskDispatch {
                        assignment_id: row.get(0)?,
                        task_id: row
                            .get::<_, String>(1)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        worker_id: row
                            .get::<_, String>(2)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        session_id: row
                            .get::<_, String>(3)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        title: row.get(4)?,
                        description: row.get(5)?,
                        priority,
                        workspace: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for delivery in &candidates {
            let updated = transaction.execute(
                "UPDATE task_dispatches SET state = 'dispatching', attempts = attempts + 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE assignment_id = ?1 AND state = 'queued' AND attempts < ?3",
                params![delivery.assignment_id, now, MAX_DISPATCH_ATTEMPTS],
            )?;
            if updated != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "task dispatch claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records an acknowledged task briefing.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_task_dispatch(assignment_id, now, None)
    }

    /// Records a definitive retryable failure or an ambiguous outcome.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn fail_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
        failure: TaskDispatchFailure,
    ) -> Result<bool, TaskStoreError> {
        self.finish_task_dispatch(assignment_id, now, Some(failure))
    }

    /// Returns a claimed briefing to its durable queue without consuming an
    /// attempt when the provider is waiting for operator input.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn defer_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_dispatches
             SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE assignment_id = ?1 AND state = 'dispatching'",
            params![assignment_id, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn finish_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
        failure: Option<TaskDispatchFailure>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, delivered_at) = match failure {
            None => (TaskDispatchState::Delivered.to_string(), Some(now)),
            Some(TaskDispatchFailure::Uncertain) => {
                (TaskDispatchState::Uncertain.to_string(), None)
            }
            Some(TaskDispatchFailure::Retryable) => {
                let attempts: i64 = transaction.query_row(
                    "SELECT attempts FROM task_dispatches
                     WHERE assignment_id = ?1 AND state = 'dispatching'",
                    [assignment_id],
                    |row| row.get(0),
                )?;
                (
                    if attempts >= MAX_DISPATCH_ATTEMPTS {
                        "uncertain"
                    } else {
                        "queued"
                    }
                    .into(),
                    None,
                )
            }
        };
        let changed = transaction.execute(
            "UPDATE task_dispatches SET state = ?2, delivered_at = ?3, updated_at = ?4
             WHERE assignment_id = ?1 AND state = 'dispatching'",
            params![assignment_id, state, delivered_at, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Converts crash-interrupted task briefings to explicit, non-retrying uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_task_dispatches(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_dispatches SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, ProviderKind, TaskDispatchState};

    fn assigned_task() -> (TaskStore, TaskId, WorkerSessionId) {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task_with_details(
                "Polish the empty state",
                "Keep the bee expressive at mobile size.",
                TaskPriority::High,
                "/workspace/petal",
            )
            .unwrap();
        store.assign_task(task.id, session).unwrap();
        (store, task.id, session)
    }

    #[test]
    fn assignment_waits_for_engagement_then_delivers_to_current_session() {
        let (store, task_id, session) = assigned_task();
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Queued)
        );
        store
            .renew_worker_engagement(session, Some(PresenceDeviceId::new()), 100, 300)
            .unwrap();
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());

        let dispatches = store.claim_task_dispatches(401).unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].task_id, task_id);
        assert_eq!(dispatches[0].session_id, session);
        assert_eq!(dispatches[0].priority, TaskPriority::High);
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Dispatching)
        );

        assert!(
            store
                .complete_task_dispatch(&dispatches[0].assignment_id, 402)
                .unwrap()
        );
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Delivered)
        );
    }

    #[test]
    fn reassignment_and_session_release_cancel_only_queued_briefings() {
        let (store, task_id, first_session) = assigned_task();
        let second = store
            .create_worker(
                "Violet",
                ProviderKind::ClaudeCode,
                "/workspace/violet",
                false,
                2,
            )
            .unwrap();
        let second_session = WorkerSessionId::new();
        store
            .bind_worker_session(second.id, second_session)
            .unwrap();

        store.assign_task(task_id, second_session).unwrap();
        let dispatches = store.claim_task_dispatches(100).unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].session_id, second_session);
        assert_ne!(dispatches[0].session_id, first_session);

        let second_task = store
            .create_task_with_details(
                "Verify mobile controls",
                "",
                TaskPriority::Normal,
                "/workspace/violet",
            )
            .unwrap();
        store.assign_task(second_task.id, second_session).unwrap();
        assert_eq!(
            store.release_session_assignments(second_session).unwrap(),
            2
        );
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());
        assert_eq!(store.get_task(second_task.id).unwrap().dispatch_state, None);
    }

    #[test]
    fn crash_ambiguity_never_replays_a_task_briefing() {
        let (store, task_id, _) = assigned_task();
        assert_eq!(store.claim_task_dispatches(100).unwrap().len(), 1);
        assert_eq!(store.recover_inflight_task_dispatches().unwrap(), 1);
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Uncertain)
        );
    }
}
