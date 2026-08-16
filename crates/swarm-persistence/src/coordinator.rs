use rusqlite::params;
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId};
use uuid::Uuid;

use super::{TaskStore, TaskStoreError, events::insert_control_room_event};

const MAX_WAKE_CLAIMS: i64 = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorWorkerWake {
    pub action_id: String,
    pub worker_id: WorkerId,
    pub task_id: TaskId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoordinatorStatus {
    pub completed_actions: usize,
    pub queen_calls_avoided: usize,
    pub uncertain_actions: usize,
    pub queued_actions: usize,
    pub last_action_at: Option<i64>,
}

pub(crate) fn enqueue_queen_worker_wake(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    worker_id: WorkerId,
    actor_id: Option<&str>,
    assignment_sequence: i64,
    task_state: &str,
    worker_is_sleeping: bool,
) -> Result<bool, TaskStoreError> {
    if task_state != "ready" || !worker_is_sleeping {
        return Ok(false);
    }
    let Some(actor_id) = actor_id else {
        return Ok(false);
    };
    let actor_is_queen: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles WHERE id = ?1 AND role = 'queen' AND archived_at IS NULL)",
        [actor_id],
        |row| row.get(0),
    )?;
    if !actor_is_queen {
        return Ok(false);
    }
    let idempotency_key =
        format!("wake-assigned-worker:{task_id}:{worker_id}:{assignment_sequence}");
    let changed = transaction.execute(
        "INSERT OR IGNORE INTO coordinator_actions
             (id, idempotency_key, kind, worker_id, task_id, state, reason)
         VALUES (?1, ?2, 'wake_assigned_worker', ?3, ?4, 'queued',
                 'Queen assigned Ready work to a sleeping worker')",
        params![
            Uuid::now_v7().to_string(),
            idempotency_key,
            worker_id.to_string(),
            task_id.to_string(),
        ],
    )? == 1;
    if changed {
        insert_control_room_event(transaction, ControlRoomEventKind::WorkersChanged)?;
    }
    Ok(changed)
}

impl TaskStore {
    /// Claims a bounded batch of deterministic worker wakes. A claimed action is
    /// never replayed after ambiguity; API startup marks it uncertain instead.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn claim_coordinator_worker_wakes(
        &self,
        now: i64,
    ) -> Result<Vec<CoordinatorWorkerWake>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE coordinator_actions SET state = 'cancelled', updated_at = ?1
             WHERE state = 'queued' AND (
                 NOT EXISTS (
                     SELECT 1 FROM tasks task
                     WHERE task.id = coordinator_actions.task_id AND task.state = 'ready'
                       AND task.assigned_worker_id = coordinator_actions.worker_id
                 ) OR EXISTS (
                     SELECT 1 FROM worker_sessions session
                     WHERE session.worker_id = coordinator_actions.worker_id AND session.ended_at IS NULL
                 )
             )",
            [now],
        )?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT action.id, action.worker_id, action.task_id
                 FROM coordinator_actions action
                 JOIN worker_profiles worker ON worker.id = action.worker_id AND worker.archived_at IS NULL
                 JOIN tasks task ON task.id = action.task_id
                 WHERE action.kind = 'wake_assigned_worker' AND action.state = 'queued'
                   AND task.state = 'ready' AND task.assigned_worker_id = action.worker_id
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_sessions session
                       WHERE session.worker_id = action.worker_id AND session.ended_at IS NULL
                   )
                 ORDER BY action.created_at, action.id LIMIT ?1",
            )?;
            statement
                .query_map([MAX_WAKE_CLAIMS], |row| {
                    Ok(CoordinatorWorkerWake {
                        action_id: row.get(0)?,
                        worker_id: row
                            .get::<_, String>(1)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        task_id: row
                            .get::<_, String>(2)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for action in &candidates {
            let changed = transaction.execute(
                "UPDATE coordinator_actions SET state = 'running', attempts = 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND state = 'queued'",
                params![action.action_id, now],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "coordinator wake claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records one acknowledged worker wake.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_coordinator_worker_wake(
        &self,
        action_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_coordinator_worker_wake(action_id, "completed", now)
    }

    /// Records an ambiguous worker wake without permitting replay.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn mark_coordinator_worker_wake_uncertain(
        &self,
        action_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_coordinator_worker_wake(action_id, "uncertain", now)
    }

    fn finish_coordinator_worker_wake(
        &self,
        action_id: &str,
        state: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE coordinator_actions SET state = ?2,
                 finished_at = CASE WHEN ?2 = 'completed' THEN ?3 ELSE finished_at END,
                 updated_at = ?3 WHERE id = ?1 AND state = 'running'",
            params![action_id, state, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns content-free cumulative coordinator evidence.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn coordinator_status(&self) -> Result<CoordinatorStatus, TaskStoreError> {
        let connection = self.connection()?;
        let (completed, uncertain, queued, last_action_at): (i64, i64, i64, Option<i64>) =
            connection.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'uncertain' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state IN ('queued','running') THEN 1 ELSE 0 END), 0),
                    MAX(CASE WHEN state = 'completed' THEN finished_at ELSE updated_at END)
                 FROM coordinator_actions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        Ok(CoordinatorStatus {
            completed_actions: usize::try_from(completed).unwrap_or_default(),
            queen_calls_avoided: usize::try_from(completed).unwrap_or_default(),
            uncertain_actions: usize::try_from(uncertain).unwrap_or_default(),
            queued_actions: usize::try_from(queued).unwrap_or_default(),
            last_action_at,
        })
    }

    /// Converts crash-interrupted worker wakes to explicit uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_coordinator_actions(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE coordinator_actions SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'running'",
            [],
        )?)
    }
}

pub(super) fn migrate_coordinator(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind = 'wake_assigned_worker'),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA user_version = 62;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskActivityActor, TaskPriority};

    #[test]
    fn queen_assignment_queues_one_durable_sleeping_worker_wake() {
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
        let task = store
            .create_task_with_details(
                "Polish the task board",
                "Keep it dense and readable.",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();

        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let actions = store.claim_coordinator_worker_wakes(100).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].worker_id, worker.id);
        assert_eq!(actions[0].task_id, task.id);
        assert!(
            store
                .complete_coordinator_worker_wake(&actions[0].action_id, 101)
                .unwrap()
        );
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.completed_actions, 1);
        assert_eq!(status.queen_calls_avoided, 1);
    }

    #[test]
    fn operator_assignment_does_not_claim_unattended_authority() {
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
        let task = store
            .create_task("Operator-directed work", "/workspace/petal")
            .unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        assert!(
            store
                .claim_coordinator_worker_wakes(100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn interrupted_wake_becomes_uncertain_and_never_replays() {
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
        let task = store
            .create_task("Wake safely", "/workspace/petal")
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        assert_eq!(store.claim_coordinator_worker_wakes(100).unwrap().len(), 1);
        assert_eq!(store.recover_inflight_coordinator_actions().unwrap(), 1);
        assert!(
            store
                .claim_coordinator_worker_wakes(101)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.coordinator_status().unwrap().uncertain_actions, 1);
    }
}
