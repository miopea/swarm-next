use rusqlite::params;
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId, WorkerSessionId};
use uuid::Uuid;

use super::{TaskStore, TaskStoreError, events::insert_control_room_event};

/// Automatic starts are intentionally serialized. A fresh resource sample is
/// required before the next sleeping worker can be claimed.
pub const AUTOMATIC_WAKE_BATCH_LIMIT: u8 = 1;
const MAX_WAKE_CLAIMS: i64 = AUTOMATIC_WAKE_BATCH_LIMIT as i64;
const MAX_STALE_CANDIDATES: i64 = 32;
const MAX_EXITED_WORK_CANDIDATES: i64 = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorWorkerWake {
    pub action_id: String,
    pub worker_id: WorkerId,
    pub task_id: TaskId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleOwnedWorkCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitedWorkerOwnedWorkCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorAttention {
    pub action_id: String,
    pub kind: String,
    pub worker_id: WorkerId,
    pub worker_name: String,
    pub task_id: TaskId,
    pub task_title: String,
    pub reason: String,
    pub observed_at: i64,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoordinatorStatus {
    pub completed_actions: usize,
    pub queen_calls_avoided: usize,
    pub uncertain_actions: usize,
    pub queued_actions: usize,
    pub stale_attention_actions: usize,
    pub worker_exit_attention_actions: usize,
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
    /// Returns bounded durable candidates whose worker process ended while it
    /// still owned Active work. The newest ended session is the exact process
    /// incarnation bound into the observation; a replacement live session
    /// suppresses the candidate.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn exited_worker_owned_work_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<ExitedWorkerOwnedWorkCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - session.ended_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN worker_sessions session ON session.session_id = (
                 SELECT latest.session_id FROM worker_sessions latest
                 WHERE latest.worker_id = worker.id AND latest.ended_at IS NOT NULL
                 ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                 LIMIT 1
             )
             WHERE task.state = 'active' AND session.ended_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions live
                   WHERE live.worker_id = worker.id AND live.ended_at IS NULL
               )
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'owned_work_worker_exited_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY session.ended_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_EXITED_WORK_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(ExitedWorkerOwnedWorkCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Records one exact worker-exit observation after the grace period. The
    /// task revision, owner, ended session, lack of a replacement session, and
    /// lack of operator engagement are rechecked atomically.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_exited_worker_owned_work_attention(
        &self,
        candidate: &ExitedWorkerOwnedWorkCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN worker_sessions session ON session.session_id = ?4
                 WHERE task.id = ?1 AND task.state = 'active'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.worker_id = ?2 AND session.ended_at IS NOT NULL
                   AND session.ended_at + ?5 <= ?6
                   AND session.session_id = (
                       SELECT latest.session_id FROM worker_sessions latest
                       WHERE latest.worker_id = ?2 AND latest.ended_at IS NOT NULL
                       ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                       LIMIT 1
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_sessions live
                       WHERE live.worker_id = ?2 AND live.ended_at IS NULL
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = ?2 AND engagement.expires_at > ?6
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                candidate.session_id.to_string(),
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "owned-work-worker-exited:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'owned_work_worker_exited_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Active work lost its loaded worker after the process exited',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns bounded durable candidates for stale-owned-work observation.
    /// Runtime/provider evidence is deliberately evaluated by the API before
    /// any attention action is recorded.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn stale_owned_work_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<StaleOwnedWorkCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - task.updated_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN worker_sessions session
               ON session.worker_id = worker.id AND session.ended_at IS NULL
             WHERE task.state = 'active' AND task.updated_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'stale_owned_work_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY task.updated_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_STALE_CANDIDATES],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .map(|row| {
                let (worker_id, session_id, task_id, task_revision, age_seconds) = row?;
                Ok::<_, rusqlite::Error>(StaleOwnedWorkCandidate {
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: session_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_revision,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Records one exact stale-owned-work observation after provider activity
    /// confirmed that the loaded worker was resting. All durable preconditions
    /// are rechecked atomically so a concurrent task or engagement change wins.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_stale_owned_work_attention(
        &self,
        candidate: &StaleOwnedWorkCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN worker_sessions session
                   ON session.worker_id = task.assigned_worker_id AND session.ended_at IS NULL
                 WHERE task.id = ?1 AND task.state = 'active'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.session_id = ?4 AND task.updated_at + ?5 <= ?6
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = task.assigned_worker_id
                         AND engagement.expires_at > ?6
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
                candidate.session_id.to_string(),
                minimum_age_seconds,
                now,
            ],
            |row| row.get(0),
        )?;
        if !still_current {
            transaction.commit()?;
            return Ok(false);
        }
        let idempotency_key = format!(
            "stale-owned-work:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'stale_owned_work_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Active work is unchanged while its loaded worker is resting',
                     ?8, ?8)",
            params![
                Uuid::now_v7().to_string(),
                idempotency_key,
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.age_seconds,
                now,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Lists current stale-work attention whose task revision, owner, and
    /// worker incarnation still match the observation.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn current_coordinator_attention(
        &self,
    ) -> Result<Vec<CoordinatorAttention>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT action.id, action.kind, worker.id, worker.name, task.id, task.title,
                    action.reason, action.finished_at, action.observed_age_seconds
             FROM coordinator_actions action
             JOIN tasks task ON task.id = action.task_id
             JOIN worker_profiles worker ON worker.id = action.worker_id
             JOIN worker_sessions session ON session.session_id = action.session_id
             WHERE action.kind IN ('stale_owned_work_attention','owned_work_worker_exited_attention')
               AND action.state = 'completed' AND task.state = 'active'
               AND task.assigned_worker_id = action.worker_id
               AND task.updated_at = action.evidence_revision
               AND session.worker_id = action.worker_id
               AND (
                   (action.kind = 'stale_owned_work_attention' AND session.ended_at IS NULL)
                   OR (action.kind = 'owned_work_worker_exited_attention'
                       AND session.ended_at IS NOT NULL
                       AND session.session_id = (
                           SELECT latest.session_id FROM worker_sessions latest
                           WHERE latest.worker_id = action.worker_id
                             AND latest.ended_at IS NOT NULL
                           ORDER BY latest.ended_at DESC, latest.started_at DESC, latest.session_id DESC
                           LIMIT 1
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM worker_sessions live
                           WHERE live.worker_id = action.worker_id AND live.ended_at IS NULL
                       ))
               )
             ORDER BY action.finished_at DESC, action.id DESC LIMIT 32",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })?
            .map(|row| {
                let (
                    action_id,
                    kind,
                    worker_id,
                    worker_name,
                    task_id,
                    task_title,
                    reason,
                    observed_at,
                    age_seconds,
                ) = row?;
                Ok::<_, rusqlite::Error>(CoordinatorAttention {
                    action_id,
                    kind,
                    worker_id: worker_id
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    worker_name,
                    task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_title,
                    reason,
                    observed_at,
                    age_seconds,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Claims at most one deterministic worker wake. A claimed action is never
    /// replayed after ambiguity; API startup marks it uncertain instead. The
    /// next action stays queued until a later coordination pass obtains fresh
    /// resource evidence.
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
        let (completed, uncertain, queued, stale_attention, worker_exit_attention, last_action_at):
            (i64, i64, i64, i64, i64, Option<i64>) =
            connection.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'uncertain' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state IN ('queued','running') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'stale_owned_work_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'owned_work_worker_exited_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    MAX(CASE WHEN state = 'completed' THEN finished_at ELSE updated_at END)
                 FROM coordinator_actions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )?;
        let wake_completed: i64 = connection.query_row(
            "SELECT COUNT(*) FROM coordinator_actions
             WHERE kind = 'wake_assigned_worker' AND state = 'completed'",
            [],
            |row| row.get(0),
        )?;
        Ok(CoordinatorStatus {
            completed_actions: usize::try_from(completed).unwrap_or_default(),
            queen_calls_avoided: usize::try_from(wake_completed).unwrap_or_default(),
            uncertain_actions: usize::try_from(uncertain).unwrap_or_default(),
            queued_actions: usize::try_from(queued).unwrap_or_default(),
            stale_attention_actions: usize::try_from(stale_attention).unwrap_or_default(),
            worker_exit_attention_actions: usize::try_from(worker_exit_attention)
                .unwrap_or_default(),
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

pub(super) fn migrate_coordinator_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 2 {
        // Some narrow historical migration fixtures contain only the table
        // whose versioned change they exercise. They cannot contain real
        // coordinator actions, so advancing the version is both safe and
        // avoids forcing SQLite to validate unrelated absent foreign tables.
        transaction.pragma_update(None, "user_version", 63)?;
        return Ok(());
    }
    transaction.execute_batch(
        "PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v62;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention')),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             session_id TEXT,
             evidence_revision INTEGER,
             observed_age_seconds INTEGER,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO coordinator_actions (
             id, idempotency_key, kind, worker_id, task_id, state, reason,
             attempts, attempted_at, finished_at, created_at, updated_at
         ) SELECT id, idempotency_key, kind, worker_id, task_id, state, reason,
                  attempts, attempted_at, finished_at, created_at, updated_at
           FROM coordinator_actions_v62;
         DROP TABLE coordinator_actions_v62;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 63;",
    )
}

pub(super) fn migrate_coordinator_worker_exit_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles', 'coordinator_actions')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 3 {
        transaction.pragma_update(None, "user_version", 64)?;
        return Ok(());
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS coordinator_actions_queue;
         PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v63;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention')),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             session_id TEXT,
             evidence_revision INTEGER,
             observed_age_seconds INTEGER,
             state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
             reason TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
             attempted_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO coordinator_actions (
             id, idempotency_key, kind, worker_id, task_id, session_id,
             evidence_revision, observed_age_seconds, state, reason, attempts,
             attempted_at, finished_at, created_at, updated_at
         ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason, attempts,
                  attempted_at, finished_at, created_at, updated_at
           FROM coordinator_actions_v63;
         DROP TABLE coordinator_actions_v63;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 64;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskActivityActor, TaskPriority, TaskState};

    fn active_owned_work(
        store: &TaskStore,
        worker_name: &str,
        updated_at: i64,
    ) -> (WorkerId, WorkerSessionId, TaskId) {
        let worker = store
            .create_worker(
                worker_name,
                ProviderKind::ClaudeCode,
                &format!("/workspace/{worker_name}"),
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task(
                "Keep the release moving",
                &format!("/workspace/{worker_name}"),
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
                params![task.id.to_string(), updated_at],
            )
            .unwrap();
        (worker.id, session, task.id)
    }

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
    fn automatic_worker_wakes_are_serialized_between_resource_checks() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let petal = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let pollen = store
            .create_worker(
                "Pollen",
                ProviderKind::ClaudeCode,
                "/workspace/pollen",
                false,
                2,
            )
            .unwrap();
        for (title, workspace, worker_id) in [
            ("Polish Petal", "/workspace/petal", petal.id),
            ("Polish Pollen", "/workspace/pollen", pollen.id),
        ] {
            let task = store.create_task(title, workspace).unwrap();
            store
                .transition_task(task.id, swarm_domain::TaskState::Ready)
                .unwrap();
            store
                .assign_task_to_worker_as(task.id, worker_id, &TaskActivityActor::worker(queen.id))
                .unwrap();
        }

        let first_pass = store.claim_coordinator_worker_wakes(100).unwrap();
        assert_eq!(first_pass.len(), usize::from(AUTOMATIC_WAKE_BATCH_LIMIT));
        assert_eq!(store.coordinator_status().unwrap().queued_actions, 2);
        assert!(
            store
                .complete_coordinator_worker_wake(&first_pass[0].action_id, 101)
                .unwrap()
        );

        let second_pass = store.claim_coordinator_worker_wakes(130).unwrap();
        assert_eq!(second_pass.len(), usize::from(AUTOMATIC_WAKE_BATCH_LIMIT));
        assert_ne!(first_pass[0].worker_id, second_pass[0].worker_id);
        assert_eq!(store.coordinator_status().unwrap().queued_actions, 1);
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

    #[test]
    fn schema_v62_wake_actions_survive_both_attention_migrations() {
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
            .create_task("Preserve the queued wake", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        transaction
            .execute_batch(
                "DROP INDEX coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v63;
                 CREATE TABLE coordinator_actions (
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
                 INSERT INTO coordinator_actions (
                     id, idempotency_key, kind, worker_id, task_id, state, reason,
                     attempts, attempted_at, finished_at, created_at, updated_at
                 ) SELECT id, idempotency_key, kind, worker_id, task_id, state, reason,
                          attempts, attempted_at, finished_at, created_at, updated_at
                   FROM coordinator_actions_v63;
                 DROP TABLE coordinator_actions_v63;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF;
                 PRAGMA user_version = 62;",
            )
            .unwrap();

        migrate_coordinator_attention(&transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            63
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT kind FROM coordinator_actions WHERE task_id = ?1",
                    [task.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "wake_assigned_worker"
        );
        let new_columns = transaction
            .prepare("PRAGMA table_info(coordinator_actions)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(new_columns.contains(&"session_id".to_owned()));
        assert!(new_columns.contains(&"evidence_revision".to_owned()));
        migrate_coordinator_worker_exit_attention(&transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            64
        );
        let table_sql: String = transaction
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_actions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_sql.contains("owned_work_worker_exited_attention"));
        transaction.commit().unwrap();
    }

    #[test]
    fn stale_owned_work_requires_loaded_unengaged_active_ownership() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, session, task) = active_owned_work(&store, "Petal", 100);

        let candidate = store.stale_owned_work_candidates(1_000, 600).unwrap();
        assert_eq!(candidate.len(), 1);
        assert_eq!(candidate[0].worker_id, worker);
        assert_eq!(candidate[0].session_id, session);
        assert_eq!(candidate[0].task_id, task);
        assert_eq!(candidate[0].task_revision, 100);
        assert_eq!(candidate[0].age_seconds, 900);

        store
            .renew_worker_engagement(session, None, 1_000, 300)
            .unwrap();
        assert!(
            store
                .stale_owned_work_candidates(1_001, 600)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stale_attention_is_revision_bound_visible_and_idempotent() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, task) = active_owned_work(&store, "Clover", 100);
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();

        assert!(
            store
                .record_stale_owned_work_attention(&candidate, 1_000, 600)
                .unwrap()
        );
        assert!(
            !store
                .record_stale_owned_work_attention(&candidate, 1_001, 600)
                .unwrap()
        );
        let attention = store.current_coordinator_attention().unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].worker_name, "Clover");
        assert_eq!(attention[0].task_title, "Keep the release moving");
        assert_eq!(attention[0].age_seconds, 900);
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.completed_actions, 1);
        assert_eq!(status.stale_attention_actions, 1);
        assert_eq!(status.queen_calls_avoided, 0);

        store
            .transition_task_with_note(task, TaskState::Review, "Ready for review")
            .unwrap();
        assert!(store.current_coordinator_attention().unwrap().is_empty());
    }

    #[test]
    fn stale_attention_rechecks_revision_before_recording() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, task) = active_owned_work(&store, "Daisy", 100);
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 900 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();

        assert!(
            !store
                .record_stale_owned_work_attention(&candidate, 1_000, 600)
                .unwrap()
        );
        assert!(store.current_coordinator_attention().unwrap().is_empty());
    }

    #[test]
    fn exited_worker_attention_is_grace_perioded_revision_bound_and_clears_on_recovery() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, session, task) = active_owned_work(&store, "Poppy", 100);
        assert!(store.release_worker_session(session).unwrap());
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_sessions SET ended_at = 400 WHERE session_id = ?1",
                [session.to_string()],
            )
            .unwrap();

        assert!(
            store
                .exited_worker_owned_work_candidates(699, 300)
                .unwrap()
                .is_empty()
        );
        let candidate = store
            .exited_worker_owned_work_candidates(700, 300)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(candidate.worker_id, worker);
        assert_eq!(candidate.session_id, session);
        assert_eq!(candidate.task_id, task);
        assert_eq!(candidate.task_revision, 100);
        assert_eq!(candidate.age_seconds, 300);
        assert!(
            store
                .record_exited_worker_owned_work_attention(&candidate, 700, 300)
                .unwrap()
        );
        assert!(
            !store
                .record_exited_worker_owned_work_attention(&candidate, 701, 300)
                .unwrap()
        );

        let attention = store.current_coordinator_attention().unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].kind, "owned_work_worker_exited_attention");
        assert_eq!(attention[0].worker_name, "Poppy");
        assert_eq!(attention[0].age_seconds, 300);
        let first_action_id = attention[0].action_id.clone();
        let status = store.coordinator_status().unwrap();
        assert_eq!(status.worker_exit_attention_actions, 1);
        assert_eq!(status.stale_attention_actions, 0);

        let replacement = WorkerSessionId::new();
        store.bind_worker_session(worker, replacement).unwrap();
        assert!(store.current_coordinator_attention().unwrap().is_empty());

        assert!(store.release_worker_session(replacement).unwrap());
        let replacement_candidate = store
            .exited_worker_owned_work_candidates(i64::MAX / 2, 0)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(replacement_candidate.session_id, replacement);
        assert!(
            store
                .record_exited_worker_owned_work_attention(&replacement_candidate, i64::MAX / 2, 0,)
                .unwrap()
        );
        let attention = store.current_coordinator_attention().unwrap();
        assert_eq!(attention.len(), 1);
        assert_ne!(attention[0].action_id, first_action_id);
    }

    #[test]
    fn exited_worker_attention_rechecks_task_revision_before_recording() {
        let store = TaskStore::in_memory().unwrap();
        let (_, session, task) = active_owned_work(&store, "Aster", 100);
        assert!(store.release_worker_session(session).unwrap());
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_sessions SET ended_at = 400 WHERE session_id = ?1",
                [session.to_string()],
            )
            .unwrap();
        let candidate = store
            .exited_worker_owned_work_candidates(700, 300)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 650 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();

        assert!(
            !store
                .record_exited_worker_owned_work_attention(&candidate, 700, 300)
                .unwrap()
        );
        assert!(store.current_coordinator_attention().unwrap().is_empty());
    }
}
