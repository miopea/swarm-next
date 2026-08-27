use rusqlite::params;
use std::str::FromStr;
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId, WorkerSessionId};
use uuid::Uuid;

use super::{
    TaskStore, TaskStoreError, WORKER_FILED_DRAFT_SCHEMA_VERSION, events::insert_control_room_event,
};

/// Automatic starts are intentionally serialized. A fresh resource sample is
/// required before the next sleeping worker can be claimed.
pub const AUTOMATIC_WAKE_BATCH_LIMIT: u8 = 1;
const MAX_WAKE_CLAIMS: i64 = AUTOMATIC_WAKE_BATCH_LIMIT as i64;
const MAX_STALE_CANDIDATES: i64 = 32;
const MAX_EXITED_WORK_CANDIDATES: i64 = 32;
const MAX_UNSTARTED_WORK_CANDIDATES: i64 = 32;

/// The one definition of a coordination-attention record that is still true.
///
/// Three questions are asked of it: what Queen sees when she reviews, how much
/// is actionable, and — crucially — the fingerprint that decides she should run
/// at all. They were three copies of one predicate, and a fourth kind added to
/// only the first was missed by the other two. The result was exactly what the
/// operator saw: a worker filed several tasks, the record existed, Queen could
/// have read it, and nothing ever woke her to look.
///
/// Every branch re-verifies against live state, so a record whose reason has
/// passed stops counting without anything having to delete it.
pub(super) const LIVE_ATTENTION_SOURCE: &str = "FROM coordinator_actions action
             JOIN tasks task ON task.id = action.task_id
             JOIN worker_profiles worker ON worker.id = action.worker_id
             JOIN worker_sessions session ON session.session_id = action.session_id
             WHERE action.kind IN ('stale_owned_work_attention','owned_work_never_briefed_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','reviewed_work_without_evidence_attention')
               AND action.state = 'completed'
               AND task.updated_at = action.evidence_revision
               AND session.worker_id = action.worker_id
               AND (
                   (action.kind = 'worker_filed_draft_attention'
                       AND task.state = 'draft' AND task.assigned_worker_id IS NULL)
                   -- The decision this was raised for is named in the
                   -- idempotency key rather than in a column, so the joins
                   -- above are untouched: the row still hangs off the worker
                   -- that asked and the task it concerns. It counts only while
                   -- that decision is genuinely still waiting, so answering it
                   -- clears the attention without anything having to delete it.
                   OR (action.kind = 'decision_deadline_passed_attention'
                       AND EXISTS (
                           SELECT 1 FROM decision_requests request
                           WHERE 'decision-deadline:' || request.id = action.idempotency_key
                             AND request.state = 'pending'
                             AND request.deadline IS NOT NULL
                             AND request.deadline <= unixepoch()
                       ))
                   -- Finished work nobody can close. Clears itself the
                   -- moment either kind of evidence exists, or the task leaves
                   -- review, so recording the claim is the whole fix and
                   -- nothing has to be dismissed by hand.
                   OR (action.kind = 'reviewed_work_without_evidence_attention'
                       AND task.state = 'review'
                       AND NOT EXISTS (
                           SELECT 1 FROM task_deployments deployment
                           WHERE deployment.task_id = task.id
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM task_completion_exemptions exemption
                           WHERE exemption.task_id = task.id
                       ))
                   OR (action.kind = 'stale_owned_work_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active' AND session.ended_at IS NULL)
                   -- Clears itself the moment a brief is confirmed delivered,
                   -- so redelivering the work is the whole fix and nothing has
                   -- to be dismissed by hand.
                   OR (action.kind = 'owned_work_never_briefed_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active' AND session.ended_at IS NULL
                       AND NOT EXISTS (
                           SELECT 1 FROM task_dispatches delivered
                           WHERE delivered.task_id = action.task_id
                             AND delivered.worker_id = action.worker_id
                             AND delivered.delivered_at IS NOT NULL
                       ))
                   OR (action.kind = 'owned_work_worker_exited_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'active'
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
                   OR (action.kind = 'assigned_ready_work_not_started_attention'
                       AND task.assigned_worker_id = action.worker_id
                       AND task.state = 'ready' AND session.ended_at IS NULL
                       -- The reason has passed once its worker picks up other
                       -- work: the briefing is queued behind that, not ignored.
                       -- Guarding only where the row is CREATED stops new false
                       -- rows and leaves existing ones surfacing forever, which
                       -- is what a Queen hit four automation runs in a row --
                       -- watching a live-computed age climb past an hour on a
                       -- situation that was never a problem.
                       AND NOT EXISTS (
                           SELECT 1 FROM tasks busy
                           WHERE busy.assigned_worker_id = action.worker_id
                             AND busy.state = 'active' AND busy.removed_at IS NULL
                             AND busy.id <> task.id
                       )
                       AND EXISTS (
                           SELECT 1 FROM task_assignments assignment
                           JOIN task_dispatches dispatch
                             ON dispatch.assignment_id = assignment.id
                                AND dispatch.state = 'delivered'
                           WHERE assignment.task_id = task.id
                             AND dispatch.worker_id = action.worker_id
                             AND assignment.worker_session_id = action.session_id
                             AND assignment.released_at IS NULL
                       ))
               )";

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
pub struct AssignedReadyWorkNotStartedCandidate {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub age_seconds: i64,
}

/// Finished work that cannot be closed, because neither kind of completion
/// evidence exists for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedWorkWithoutEvidenceCandidate {
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
    /// How long this has been standing, computed when the row is read.
    ///
    /// It used to be `observed_age_seconds`, written once at observation and
    /// returned verbatim ever after, so it read like a live age and was not
    /// one. Measured 2026-08-23: three records reported 301, 303 and 310
    /// seconds while the conditions were 176 to 178 minutes old — understated
    /// by a factor of thirty — and one value moved DOWNWARD between two calls,
    /// so it was inconsistent as well as stale.
    ///
    /// The harm landed. Queen read 303s, concluded the briefs were five
    /// minutes old and therefore not yet evidence of a stall, and wrote that
    /// recommendation into a live operator decision record. At the real age she
    /// would have restarted the worker.
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
    pub unstarted_attention_actions: usize,
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

    /// Returns delivered Ready assignments whose loaded worker has remained
    /// resting without starting the task. Runtime/provider evidence is
    /// deliberately evaluated by the API before attention is recorded.
    ///
    /// # Errors
    /// Returns a persistence or identity-integrity error.
    pub fn assigned_ready_work_not_started_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<AssignedReadyWorkNotStartedCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - dispatch.delivered_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN task_assignments assignment
               ON assignment.task_id = task.id AND assignment.released_at IS NULL
             JOIN worker_sessions session
               ON session.session_id = assignment.worker_session_id
                  AND session.worker_id = worker.id AND session.ended_at IS NULL
             JOIN task_dispatches dispatch
               ON dispatch.assignment_id = assignment.id AND dispatch.worker_id = worker.id
                  AND dispatch.state = 'delivered'
             WHERE task.state = 'ready' AND dispatch.delivered_at IS NOT NULL
               AND dispatch.delivered_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker.id AND engagement.expires_at > ?1
               )
               -- A worker carrying OTHER active work has not ignored this
               -- briefing; it is queued behind the thing that worker is doing.
               -- Without this the attention list and the held-briefing list
               -- describe the same situation and disagree about whether it is a
               -- problem: delivery already holds a briefing back for exactly
               -- this reason and reports it as worker_already_working, while
               -- this flag called it delivered-but-never-started.
               --
               -- Queen hit it three times in one night on one task and had to
               -- read a transcript and /proc each time to establish that nothing
               -- was wrong. A flag that cannot tell queued-behind-higher-
               -- priority-work from delivered-and-ignored trains its reader
               -- to check every instance by hand, which is the same as not
               -- having it, and worse, because reaching that conclusion costs
               -- attention every time.
               AND NOT EXISTS (
                   SELECT 1 FROM tasks active
                   WHERE active.assigned_worker_id = worker.id
                     AND active.state = 'active' AND active.removed_at IS NULL
                     AND active.id <> task.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = 'assigned_ready_work_not_started_attention'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             ORDER BY dispatch.delivered_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
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
                Ok::<_, rusqlite::Error>(AssignedReadyWorkNotStartedCandidate {
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

    /// Records one exact unstarted-work observation after the delivered brief
    /// has exceeded its grace period. Assignment, session, revision, delivery,
    /// and lack of operator engagement are rechecked atomically.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_assigned_ready_work_not_started_attention(
        &self,
        candidate: &AssignedReadyWorkNotStartedCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 JOIN task_assignments assignment
                   ON assignment.task_id = task.id AND assignment.released_at IS NULL
                 JOIN worker_sessions session
                   ON session.session_id = assignment.worker_session_id
                      AND session.worker_id = ?2 AND session.ended_at IS NULL
                 JOIN task_dispatches dispatch
                   ON dispatch.assignment_id = assignment.id AND dispatch.worker_id = ?2
                      AND dispatch.state = 'delivered'
                 WHERE task.id = ?1 AND task.state = 'ready'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND session.session_id = ?4 AND dispatch.delivered_at IS NOT NULL
                   AND dispatch.delivered_at + ?5 <= ?6
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
            "assigned-ready-work-not-started:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'assigned_ready_work_not_started_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'Ready work was delivered but its loaded worker did not start it',
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

    /// Work in review that has neither a deployment nor a no-deployment claim.
    ///
    /// Unlike every other kind here, the session is not required to be live:
    /// the worst version of this is exactly the one where the worker has moved
    /// on, because then nothing else will ever record the claim.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn reviewed_work_without_evidence_candidates(
        &self,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<Vec<ReviewedWorkWithoutEvidenceCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.assigned_worker_id, session.session_id, task.id,
                    task.updated_at, MAX(0, ?1 - task.updated_at)
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             JOIN task_assignments assignment
               ON assignment.task_id = task.id
             JOIN worker_sessions session
               ON session.session_id = assignment.worker_session_id
                  AND session.worker_id = worker.id
             WHERE task.state = 'review' AND task.removed_at IS NULL
               AND task.updated_at + ?2 <= ?1
               AND NOT EXISTS (
                   SELECT 1 FROM task_deployments deployment
                   WHERE deployment.task_id = task.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM task_completion_exemptions exemption
                   WHERE exemption.task_id = task.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM coordinator_actions action
                   WHERE action.kind = \'reviewed_work_without_evidence_attention\'
                     AND action.task_id = task.id AND action.worker_id = worker.id
                     AND action.session_id = session.session_id
                     AND action.evidence_revision = task.updated_at
               )
             GROUP BY task.id
             ORDER BY task.updated_at, task.id LIMIT ?3",
        )?;
        statement
            .query_map(
                params![now, minimum_age_seconds, MAX_UNSTARTED_WORK_CANDIDATES],
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
                Ok::<_, rusqlite::Error>(ReviewedWorkWithoutEvidenceCandidate {
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

    /// Records one exact observation of finished work with no evidence.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_reviewed_work_without_evidence_attention(
        &self,
        candidate: &ReviewedWorkWithoutEvidenceCandidate,
        now: i64,
        minimum_age_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_current: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM tasks task
                 WHERE task.id = ?1 AND task.state = \'review\'
                   AND task.assigned_worker_id = ?2 AND task.updated_at = ?3
                   AND task.updated_at + ?4 <= ?5
                   AND NOT EXISTS (
                       SELECT 1 FROM task_deployments deployment
                       WHERE deployment.task_id = task.id
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM task_completion_exemptions exemption
                       WHERE exemption.task_id = task.id
                   )
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string(),
                candidate.task_revision,
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
            "reviewed-work-without-evidence:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, \'reviewed_work_without_evidence_attention\', ?3, ?4, ?5, ?6, ?7,
                     \'completed\', \'Work reached review with neither a deployment nor a no-deployment claim\',
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

    /// Tells Queen that a worker filed work and cannot route it.
    ///
    /// A worker has no channel to Queen at all: her inbox is written by
    /// detectors and nothing else, so a worker wanting work routed had to
    /// interrupt the operator instead — the opposite of what Queen is for. It
    /// can file a draft, but nothing said one was waiting.
    ///
    /// Recorded when the draft is filed rather than found by a later sweep,
    /// because the filing is the event. One record per task, and it stops being
    /// shown the moment the task is no longer an unrouted draft.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_worker_filed_draft_attention(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        session_id: WorkerSessionId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let revision: i64 = transaction.query_row(
            "SELECT updated_at FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        // Deliberately not INSERT OR IGNORE on the conflict clause alone: that
        // suppresses a CHECK violation exactly as it suppresses a duplicate, so
        // an unadmitted kind would do nothing and say nothing. Only a repeat of
        // the same filing is ignorable.
        let changed = transaction.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'worker_filed_draft_attention', ?3, ?4, ?5, ?6, 0,
                     'completed', 'A worker filed this work and cannot route it',
                     unixepoch(), unixepoch())
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                Uuid::now_v7().to_string(),
                format!("worker-filed-draft:{task_id}"),
                worker_id.to_string(),
                task_id.to_string(),
                session_id.to_string(),
                revision,
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
               -- Work stops changing while its answer is with the operator, and
               -- that is the system working rather than a fault. Reporting it as
               -- unchanged sends the operator to look at a worker that is
               -- already waiting on them.
               AND NOT EXISTS (
                   SELECT 1 FROM decision_requests decision
                   WHERE decision.task_id = task.id AND decision.state = 'pending'
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
        // Whether this worker was ever actually handed the work.
        //
        // The two cases look identical from outside — no commits, no files
        // touched, a resting worker on Active work — so this reported both as
        // "unchanged while resting", which names the worker. Measured
        // 2026-08-19: a high-priority brief sat with one attempt and no
        // delivery for 27 hours while the worker was blamed for being idle. It
        // had nothing to act on.
        //
        // The dispatch row carried the answer the whole time; nothing consulted
        // it. A brief that was never confirmed delivered is a different problem
        // with a different fix, so it is a different kind of attention.
        //
        // Only an `uncertain` row counts, because only that one is provably
        // going nowhere: queued and dispatching are still in flight, and the
        // absence of any row proves nothing at all — old rows are trimmed once
        // there are more than 1024 of them, so inferring "never briefed" from a
        // missing row would invent the same false accusation in the other
        // direction.
        let never_briefed: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM task_dispatches dispatch
                 WHERE dispatch.task_id = ?1 AND dispatch.worker_id = ?2
                   AND dispatch.state = 'uncertain' AND dispatch.delivered_at IS NULL
             ) AND NOT EXISTS(
                 SELECT 1 FROM task_dispatches delivered
                 WHERE delivered.task_id = ?1 AND delivered.worker_id = ?2
                   AND delivered.delivered_at IS NOT NULL
             )",
            params![
                candidate.task_id.to_string(),
                candidate.worker_id.to_string()
            ],
            |row| row.get(0),
        )?;
        let (kind, reason, key) = if never_briefed {
            (
                "owned_work_never_briefed_attention",
                "This worker was never given this brief — the delivery was never confirmed, so it has nothing to act on. Re-assign the task to send it again rather than steering the worker.",
                "owned-work-never-briefed",
            )
        } else {
            (
                "stale_owned_work_attention",
                "Active work is unchanged while its loaded worker is resting",
                "stale-owned-work",
            )
        };
        let idempotency_key = format!(
            "{key}:{}:{}:{}:{}",
            candidate.task_id, candidate.worker_id, candidate.session_id, candidate.task_revision
        );
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, ?9, ?3, ?4, ?5, ?6, ?7,
                     'completed', ?10,
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
                kind,
                reason,
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
        now: i64,
    ) -> Result<Vec<CoordinatorAttention>, TaskStoreError> {
        let connection = self.connection()?;
        // Derived here rather than read from the row it was written on. A
        // cached age is indistinguishable from a live one in the payload, and
        // every conclusion drawn from it inherits the error silently.
        //
        // The sum of both halves, not just the time since. The condition was
        // already `observed_age_seconds` old when it was first noticed — a
        // detector with a grace period never fires at zero — so reporting only
        // the elapsed time would understate it by that grace on every read.
        // What Queen needs is how long the situation has been true.
        let mut statement = connection.prepare(&format!(
            "SELECT action.id, action.kind, worker.id, worker.name, task.id, task.title,
                        action.reason, action.finished_at,
                        action.observed_age_seconds + MAX(0, ?1 - action.finished_at)
                 {LIVE_ATTENTION_SOURCE}
                 ORDER BY action.finished_at DESC, action.id DESC LIMIT 32"
        ))?;
        statement
            .query_map([now], |row| {
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
        let (
            completed,
            uncertain,
            queued,
            stale_attention,
            worker_exit_attention,
            unstarted_attention,
            last_action_at,
        ): (i64, i64, i64, i64, i64, i64, Option<i64>) =
            connection.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state = 'uncertain' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN state IN ('queued','running') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'stale_owned_work_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'owned_work_worker_exited_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN kind = 'assigned_ready_work_not_started_attention' AND state = 'completed' THEN 1 ELSE 0 END), 0),
                    MAX(CASE WHEN state = 'completed' THEN finished_at ELSE updated_at END)
                 FROM coordinator_actions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
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
            unstarted_attention_actions: usize::try_from(unstarted_attention).unwrap_or_default(),
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

pub(super) fn migrate_coordinator_unstarted_work_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let prerequisite_tables = transaction.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles', 'coordinator_actions')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if prerequisite_tables != 3 {
        transaction.pragma_update(None, "user_version", 65)?;
        return Ok(());
    }
    transaction.execute_batch(
        "DROP INDEX IF EXISTS coordinator_actions_queue;
         PRAGMA legacy_alter_table = ON;
         ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v64;
         CREATE TABLE coordinator_actions (
             id TEXT PRIMARY KEY,
             idempotency_key TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention')),
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
           FROM coordinator_actions_v64;
         DROP TABLE coordinator_actions_v64;
         CREATE INDEX coordinator_actions_queue
             ON coordinator_actions(state, created_at, id);
         PRAGMA legacy_alter_table = OFF;
         PRAGMA user_version = 65;",
    )
}

/// Admits the one kind of attention a worker can raise itself.
///
/// `kind` carries a CHECK, and the writer uses INSERT OR IGNORE — which
/// suppresses a constraint violation as readily as a duplicate. A new kind
/// therefore fails silently and completely until the constraint admits it,
/// which is why this migration exists rather than the insert simply working.
///
/// # Errors
/// Returns an error when the step cannot be applied.
/// Admits the kind an overdue decision will be recorded under, before anything
/// writes one.
///
/// Item 53's hazard, applied deliberately this time rather than discovered:
/// `coordinator_actions.kind` carries a CHECK and the detectors write with a
/// conflict clause, so a kind the schema does not admit fails in a way that is
/// easy to read as "nothing to do". Widening first means the detector that
/// follows cannot be silently inert.
///
/// Nothing writes this kind yet. The detector needs Queen's inbox to tolerate a
/// row whose worker has no live session — a worker that files a decision and
/// stops is exactly the case — and that is a change to a query which has
/// already been the source of one three-copies bug. It is worth doing on its
/// own rather than inside a release cut to test an upgrade.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_decision_deadline_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%decision_deadline_passed_attention%')",
        [],
        |row| row.get(0),
    )?;
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v84;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention')),
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
               FROM coordinator_actions_v84;
             DROP TABLE coordinator_actions_v84;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    Ok(())
}

pub(super) fn migrate_worker_filed_draft_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    // The steps that built this table's current shape bail when their own
    // prerequisites are absent, so a database can arrive here still carrying an
    // older one. Rebuild only what is actually the shape this widens; anything
    // else is left alone rather than rewritten from a guess.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%assigned_ready_work_not_started_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v80;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention')),
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
               FROM coordinator_actions_v80;
             DROP TABLE coordinator_actions_v80;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(None, "user_version", WORKER_FILED_DRAFT_SCHEMA_VERSION)
}

/// Tells "the worker has the brief and is idle" apart from "the worker was
/// never given the brief".
///
/// Those look identical from outside — no commits, no files touched, a resting
/// worker on Active work — and `stale_owned_work_attention` reported both as
/// the first, which points the reader at a worker that is innocent. Measured
/// 2026-08-19: a high-priority brief sat undelivered for 27 hours while every
/// board read the task as work in progress.
///
/// A separate kind rather than a different wording, because the two need
/// different actions: one is steer the worker, the other is redeliver the
/// brief.
pub(super) fn migrate_undelivered_brief_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%owned_work_never_briefed_attention%')",
        [],
        |row| row.get(0),
    )?;
    // As with the step before it: a database can arrive here still carrying an
    // older shape, so widen only the shape this actually widens.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%worker_filed_draft_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v88;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention')),
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
               FROM coordinator_actions_v88;
             DROP TABLE coordinator_actions_v88;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::UNDELIVERED_BRIEF_ATTENTION_SCHEMA_VERSION,
    )
}

/// Adds the kind that says finished work is waiting on evidence nobody gave it.
///
/// Review was the one task state no attention kind covered. Work that was done
/// and work that was abandoned looked identical on the board, and the only way
/// to tell them apart was reading the handoff prose — which is how three tasks
/// sat stranded in a day.
pub(super) fn migrate_reviewed_work_evidence_attention(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%reviewed_work_without_evidence_attention%')",
        [],
        |row| row.get(0),
    )?;
    // Only widen the shape this actually widens: a database can arrive here
    // carrying an older one.
    let ready: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions'
           AND sql LIKE '%owned_work_never_briefed_attention%')",
        [],
        |row| row.get(0),
    )?;
    if ready && !present {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS coordinator_actions_queue;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v89;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention')),
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
               FROM coordinator_actions_v89;
             DROP TABLE coordinator_actions_v89;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::REVIEWED_WORK_EVIDENCE_ATTENTION_SCHEMA_VERSION,
    )
}

#[cfg(test)]
mod reviewed_work_tests {
    use crate::TaskStore;
    use swarm_domain::{ProviderKind, TaskState, WorkerSessionId};

    /// Review was the one task state no attention kind covered, so finished
    /// work and abandoned work looked identical on the board. Three tasks sat
    /// stranded in a day because the only way to tell them apart was reading
    /// the handoff prose.
    #[test]
    fn finished_work_with_no_evidence_is_surfaced_and_clears_when_the_claim_lands() {
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
            .create_task("Read-only investigation", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        let now = i64::MAX / 4;
        let grace = 15 * 60;
        let fresh = store
            .reviewed_work_without_evidence_candidates(0, grace)
            .unwrap();
        assert!(fresh.is_empty(), "work just reported is not yet stranded");

        let candidates = store
            .reviewed_work_without_evidence_candidates(now, grace)
            .unwrap();
        assert_eq!(candidates.len(), 1, "no evidence of either kind");
        assert_eq!(candidates[0].task_id, task.id);
        assert!(
            store
                .record_reviewed_work_without_evidence_attention(&candidates[0], now, grace)
                .unwrap()
        );
        assert!(
            store
                .current_coordinator_attention(now)
                .unwrap()
                .iter()
                .any(|attention| attention.kind == "reviewed_work_without_evidence_attention"),
            "Queen can see it"
        );

        // Recording the claim is the whole fix: nothing is dismissed by hand.
        store
            .claim_completion_exemption(task.id, "Read-only investigation", Some(worker.id), now)
            .unwrap();
        assert!(
            !store
                .current_coordinator_attention(now)
                .unwrap()
                .iter()
                .any(|attention| attention.kind == "reviewed_work_without_evidence_attention"),
            "the claim clears it without anything having to delete the row"
        );
    }

    /// A task that shipped is not stranded, so it is never raised.
    #[test]
    fn recorded_deployment_is_not_stranded_work() {
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
            .create_task("Shipped work", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        let now = i64::MAX / 4;
        store
            .record_task_deployment(task.id, "production", "abc123", now)
            .unwrap();

        assert!(
            store
                .reviewed_work_without_evidence_candidates(now, 15 * 60)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkerId, WorkerSessionId};
    use crate::TaskStore;

    /// A decision that has gone past its deadline, with everything the inbox
    /// joins: a worker that has held a session, and a task to hang it off.
    fn overdue_decision(
        store: &TaskStore,
        deadline_ago: i64,
    ) -> (WorkerId, swarm_domain::DecisionRequestId) {
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
            .create_task("Answer this", "/workspace/petal")
            .unwrap();
        let actions = vec!["continue".into(), "stop".into()];
        let decision = store
            .create_decision_request(&crate::decisions::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task.id),
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Which environment?",
                summary: "Staging or production for the first run.",
                reason: "Both are plausible and the choice is not reversible.",
                risk: "Deploying to the wrong one is visible to customers.",
                evidence: "Both environments are healthy.",
                suggested_action: "continue",
                allowed_actions: &actions,
                questions: &[],
                // A deadline in the past is refused at creation, so it is set
                // in the future and then aged below — which is also what
                // actually happens to one.
                deadline: Some(unix_now(store) + 3_600),
                requested_command: None,
            })
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE decision_requests SET deadline = unixepoch() - ?2 WHERE id = ?1",
                rusqlite::params![decision.id.to_string(), deadline_ago],
            )
            .unwrap();
        (worker.id, decision.id)
    }

    fn unix_now(store: &TaskStore) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap()
    }

    fn attention_kinds(store: &TaskStore) -> Vec<String> {
        store
            .current_coordinator_attention(0)
            .unwrap()
            .into_iter()
            .map(|attention| attention.kind)
            .collect()
    }

    /// The operator's ruling: "queen should make this a needs you item." Before
    /// this, a deadline was recorded, shown on the roster as overdue, and acted
    /// on by nobody.
    #[test]
    fn a_decision_past_its_deadline_reaches_queens_inbox() {
        let store = TaskStore::in_memory().unwrap();
        let (_, decision_id) = overdue_decision(&store, 600);

        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].decision_id, decision_id.to_string());
        assert!(candidates[0].overdue_seconds >= 600);

        assert!(
            store
                .record_overdue_decision_attention(&candidates[0])
                .unwrap()
        );
        assert!(attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()));

        // Raising it twice for one decision would make Queen review the same
        // fact repeatedly.
        assert!(
            !store
                .record_overdue_decision_attention(&candidates[0])
                .unwrap()
        );
        assert_eq!(
            attention_kinds(&store)
                .iter()
                .filter(|kind| *kind == "decision_deadline_passed_attention")
                .count(),
            1
        );
    }

    /// Answering it is what clears it. Nothing deletes the row.
    #[test]
    fn answering_the_decision_clears_the_attention() {
        let store = TaskStore::in_memory().unwrap();
        let (_, decision_id) = overdue_decision(&store, 600);
        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        store
            .record_overdue_decision_attention(&candidates[0])
            .unwrap();
        assert!(attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()));

        // Answered the way an operator answers it, not by editing the row.
        store
            .resolve_decision_request(decision_id, "continue", "Staging first.", "operator")
            .unwrap();

        assert!(
            !attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned())
        );
        assert!(
            store
                .overdue_decision_candidates(unix_now(&store))
                .unwrap()
                .is_empty()
        );
    }

    /// The inbox only counts an observation whose evidence still matches the
    /// task it was made against, and the idempotency key stops a second row
    /// being raised — so without re-stamping, a task that moved on would
    /// silence an attention whose decision is still waiting.
    #[test]
    fn a_task_moving_on_does_not_silence_a_decision_still_waiting() {
        let store = TaskStore::in_memory().unwrap();
        overdue_decision(&store, 600);
        let candidates = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        store
            .record_overdue_decision_attention(&candidates[0])
            .unwrap();

        // The task moves on. Advanced explicitly rather than by transitioning
        // it: updated_at is a whole-second stamp, so a real edit inside the
        // same second as the observation leaves the revision unchanged and the
        // test would prove nothing.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = updated_at + 1 WHERE id = ?1",
                [candidates[0].task_id.to_string()],
            )
            .unwrap();
        assert!(
            !attention_kinds(&store).contains(&"decision_deadline_passed_attention".to_owned()),
            "the observation should have gone stale with the task"
        );

        // The next pass re-stamps it rather than raising a second one.
        let again = store.overdue_decision_candidates(unix_now(&store)).unwrap();
        assert!(store.record_overdue_decision_attention(&again[0]).unwrap());
        assert_eq!(
            attention_kinds(&store)
                .iter()
                .filter(|kind| *kind == "decision_deadline_passed_attention")
                .count(),
            1
        );
    }

    /// The grace period is what stops a prompt answered in ten seconds from
    /// becoming an item in the operator's queue.
    #[test]
    fn a_refusal_is_silent_until_it_has_lasted_long_enough_to_matter() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "Queen cannot review while her terminal has an unanswered prompt",
                1_000,
            )
            .unwrap();

        assert!(
            store
                .standing_coordinator_refusals(1_030, 120)
                .unwrap()
                .is_empty()
        );
        // Still being refused, as a real hold is — every retry re-observes it.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "Queen cannot review while her terminal has an unanswered prompt",
                1_190,
            )
            .unwrap();
        let standing = store.standing_coordinator_refusals(1_200, 120).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].subject, "queen-review");
    }

    /// The operator's report: "I see this, but it isn't true."
    ///
    /// Two task briefs had been refused four times and once, half an hour
    /// earlier, and had not been retried since — the dispatches had moved on
    /// without ever succeeding, so nothing cleared them. The queue was still
    /// announcing them as work waiting at a prompt, with a retry count frozen
    /// where it had stopped.
    ///
    /// Success is not the only way a hold ends. Held work is retried every few
    /// seconds, so what is genuinely held is re-observed constantly; anything
    /// not seen for minutes has ended, whatever ended it.
    #[test]
    fn a_refusal_nobody_is_retrying_any_more_is_not_still_standing() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "task-brief:abandoned",
                None,
                None,
                "a briefing is waiting",
                1_000,
            )
            .unwrap();

        // Still being retried: still standing.
        assert_eq!(
            store
                .standing_coordinator_refusals(1_150, 120)
                .unwrap()
                .len(),
            1
        );

        // Nothing has touched it since. It is not waiting on anything now.
        let stale = 1_000 + TaskStore::STALE_REFUSAL_SECONDS + 1;
        assert!(
            store
                .standing_coordinator_refusals(stale, 120)
                .unwrap()
                .is_empty()
        );
    }

    /// A hold that is still happening keeps being reported, however long it has
    /// lasted. The window is about whether anyone is still retrying, not about
    /// how patient the operator should be.
    #[test]
    fn a_hold_that_is_still_being_retried_keeps_standing_however_old() {
        let store = TaskStore::in_memory().unwrap();
        for minute in 0..30 {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    "queen-review",
                    None,
                    None,
                    "Queen's review is waiting",
                    1_000 + minute * 60,
                )
                .unwrap();
        }

        let standing = store
            .standing_coordinator_refusals(1_000 + 29 * 60, 120)
            .unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].observations, 30);
    }

    /// One row per stuck thing, not one per attempt. The measured case retried
    /// 1503 times in twenty-four hours; that is a count, not 1503 entries.
    #[test]
    fn repeated_refusals_are_one_row_with_a_count() {
        let store = TaskStore::in_memory().unwrap();
        for at in [1_000, 1_030, 1_060] {
            store
                .record_coordinator_refusal(
                    REFUSAL_DELIVERY_HELD,
                    "queen-review",
                    None,
                    None,
                    "held",
                    at,
                )
                .unwrap();
        }

        let standing = store.standing_coordinator_refusals(1_200, 120).unwrap();
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].observations, 3);
        // The age is measured from when it started, not from the last check.
        assert_eq!(standing[0].first_observed_at, 1_000);
    }

    /// Answering the prompt clears it, and nothing has to delete the row.
    #[test]
    fn clearing_a_refusal_takes_it_out_of_the_queue() {
        let store = TaskStore::in_memory().unwrap();
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                1_000,
            )
            .unwrap();
        assert!(
            store
                .clear_coordinator_refusal(REFUSAL_DELIVERY_HELD, "queen-review", 1_100)
                .unwrap()
        );
        assert!(
            store
                .standing_coordinator_refusals(1_400, 120)
                .unwrap()
                .is_empty()
        );

        // And a recurrence is a new occurrence, not a continuation of the old
        // one — otherwise a hold that cleared last week would appear to have
        // been standing ever since.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                2_000,
            )
            .unwrap();
        // Recurring, so still being observed when the queue is read.
        store
            .record_coordinator_refusal(
                REFUSAL_DELIVERY_HELD,
                "queen-review",
                None,
                None,
                "held",
                2_190,
            )
            .unwrap();
        let again = store.standing_coordinator_refusals(2_200, 120).unwrap();
        assert_eq!(again[0].first_observed_at, 2_000);
        assert_eq!(again[0].observations, 2);
    }

    /// A deadline that has not passed is not overdue, whatever else is true.
    #[test]
    fn a_decision_inside_its_deadline_is_left_alone() {
        let store = TaskStore::in_memory().unwrap();
        overdue_decision(&store, -3_600);
        assert!(
            store
                .overdue_decision_candidates(unix_now(&store))
                .unwrap()
                .is_empty()
        );
    }

    /// Item 53's hazard, avoided on purpose this time. The kind CHECK and the
    /// conflict clause together turn an unadmitted kind into a write that does
    /// nothing and reports nothing, so the schema admits the kind before any
    /// detector writes it.
    #[test]
    fn the_overdue_decision_kind_is_admitted_before_anything_writes_one() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();

        let admitted: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%decision_deadline_passed_attention%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            admitted,
            "the kind is not in the CHECK, so writing it would be silently inert"
        );

        // And a kind nobody admitted still fails loudly rather than quietly.
        let rejected = connection.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, state, reason)
             VALUES ('x', 'x', 'not_a_kind', 'w', 't', 'completed', 'r')",
            [],
        );
        assert!(rejected.is_err(), "an unadmitted kind was accepted");
    }
    use super::*;
    use crate::decisions::NewDecisionRequest;
    use swarm_domain::{
        DecisionRequestKind, DecisionUrgency, ProviderKind, TaskActivityActor, TaskPriority,
        TaskState,
    };

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

    fn assert_v64_to_v65_preserves_action(
        transaction: &rusqlite::Transaction<'_>,
        task_id: TaskId,
    ) {
        migrate_coordinator_unstarted_work_attention(transaction).unwrap();
        assert_eq!(
            transaction
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            65
        );
        let (kind, table_sql): (String, String) = (
            transaction
                .query_row(
                    "SELECT kind FROM coordinator_actions WHERE task_id = ?1",
                    [task_id.to_string()],
                    |row| row.get(0),
                )
                .unwrap(),
            transaction
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_actions'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
        );
        assert_eq!(kind, "wake_assigned_worker");
        assert!(table_sql.contains("assigned_ready_work_not_started_attention"));
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
    fn schema_v62_wake_actions_survive_all_attention_migrations() {
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
        assert_v64_to_v65_preserves_action(&transaction, task.id);
        transaction.commit().unwrap();
    }

    #[test]
    fn delivered_ready_work_surfaces_only_after_loaded_worker_stays_resting() {
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
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Begin the delivered work", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store.claim_task_dispatches(100).unwrap().remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 90 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();

        assert!(
            store
                .assigned_ready_work_not_started_candidates(400, 300)
                .unwrap()
                .is_empty()
        );
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(candidate.worker_id, worker.id);
        assert_eq!(candidate.session_id, session);
        assert_eq!(candidate.task_id, task.id);
        assert_eq!(candidate.task_revision, 90);
        assert_eq!(candidate.age_seconds, 300);
        assert!(
            store
                .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
                .unwrap()
        );
        assert!(
            !store
                .record_assigned_ready_work_not_started_attention(&candidate, 402, 300)
                .unwrap()
        );
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(
            attention[0].kind,
            "assigned_ready_work_not_started_attention"
        );
        assert_eq!(attention[0].worker_name, "Petal");
        assert_eq!(attention[0].age_seconds, 300);
        assert_eq!(
            store
                .coordinator_status()
                .unwrap()
                .unstarted_attention_actions,
            1
        );

        store.transition_task(task.id, TaskState::Active).unwrap();
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }

    #[test]
    fn delivered_ready_work_attention_rechecks_revision_and_engagement() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Start after briefing", "/workspace/clover")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let dispatch = store.claim_task_dispatches(100).unwrap().remove(0);
        assert!(
            store
                .complete_task_dispatch(&dispatch.assignment_id, 101)
                .unwrap()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 90 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();

        store
            .renew_worker_engagement(session, None, 401, 300)
            .unwrap();
        assert!(
            store
                .assigned_ready_work_not_started_candidates(401, 300)
                .unwrap()
                .is_empty()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "DELETE FROM worker_engagements WHERE worker_id = ?1",
                [worker.id.to_string()],
            )
            .unwrap();
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 400 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
        assert!(
            !store
                .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
                .unwrap()
        );
    }

    #[test]
    fn work_waiting_on_an_operator_decision_is_not_stale() {
        // Observed on 2026-08-18: the detector raised stale attention against
        // the BFG Operations worker while that worker was neither stuck nor
        // crashed. It had filed a decision and correctly stopped, because the
        // answer was not its to give. "Active work is unchanged while its
        // loaded worker is resting" describes a fault; waiting on the operator
        // is the system working.
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Petal", 100);

        assert_eq!(
            store.stale_owned_work_candidates(1_000, 600).unwrap().len(),
            1,
            "unchanged active work is stale while nothing is pending"
        );

        store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker,
                task_id: Some(task),
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Which reading of the number is right?",
                summary: "Whether to proceed, and what it costs if we do not.",
                reason: "The console and the wire disagree.",
                risk: "",
                evidence: "",
                suggested_action: "Treat the wire as authoritative",
                allowed_actions: &["Treat the wire as authoritative".to_owned()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();

        assert!(
            store
                .stale_owned_work_candidates(1_000, 600)
                .unwrap()
                .is_empty(),
            "work waiting on an operator answer must not be reported as stale"
        );
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

    /// A worker was blamed for 27 hours for being idle on work it had never
    /// been given.
    ///
    /// The brief was claimed, the API died before it landed, and the row sat
    /// `uncertain` with no delivery. From outside that is indistinguishable
    /// from a resting worker — no commits, no files touched — so the detector
    /// reported "unchanged while its loaded worker is resting" and pointed at
    /// the wrong thing. The dispatch row held the answer all along.
    #[test]
    fn work_whose_brief_was_never_delivered_names_the_brief_not_the_worker() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, _session, task) = active_owned_work(&store, "Clover", 100);

        // The API dies mid-delivery: claimed, attempted, never delivered.
        let claimed = store.claim_task_dispatches(200).unwrap();
        assert_eq!(claimed.len(), 1, "the brief is in flight");
        assert_eq!(store.recover_inflight_task_dispatches().unwrap(), 1);

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
        let attention = store.current_coordinator_attention(0).unwrap();
        assert_eq!(attention.len(), 1);
        assert_eq!(
            attention[0].kind, "owned_work_never_briefed_attention",
            "a brief that never landed must not be reported as an idle worker: {:?}",
            attention[0]
        );
        assert!(
            attention[0].reason.contains("never given this brief"),
            "and it must say what to do about it: {}",
            attention[0].reason
        );

        // Re-assigning is Queen's lever for stranded work, and for Active work
        // it used to bind a session and deliver nothing. It now redelivers,
        // which is the whole repair — no walking the task through `blocked` to
        // reach `ready`, which lies on the board while it happens.
        store
            .assign_task_to_worker_as(task, worker, &TaskActivityActor::operator())
            .unwrap();
        let redelivered = store.claim_task_dispatches(2_000).unwrap();
        assert!(
            redelivered.iter().any(|dispatch| dispatch.task_id == task),
            "re-assigning Active work must send the brief again"
        );
        store
            .complete_task_dispatch(&redelivered[0].assignment_id, 2_001)
            .unwrap();

        // And the attention clears itself once the brief lands, rather than
        // needing to be dismissed by hand.
        assert!(
            store.current_coordinator_attention(0).unwrap().is_empty(),
            "a delivered brief answers the question the attention was asking"
        );
    }

    /// Two tasks sat assigned and unreachable for twenty-one minutes, looking
    /// routed on the board, until the operator asked from the other side why he
    /// could not see those workers in the list. They were not running, and
    /// assignment to a sleeping worker creates no session binding and no
    /// briefing, so nothing anywhere said the work could not be delivered.
    ///
    /// The control matters as much as the case. The original report was made
    /// twice with `pgrep -af <worker id>`, which matched the reporting shell's
    /// own command line and so could never answer NO. This asserts both
    /// directions against `worker_sessions`.
    #[test]
    fn work_assigned_to_a_worker_that_is_not_running_is_visible_as_such() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let sleeping = store
            .create_worker(
                "RCG Hub",
                ProviderKind::ClaudeCode,
                "/workspace/hub",
                false,
                1,
            )
            .unwrap();
        let running = store
            .create_worker(
                "Scout",
                ProviderKind::ClaudeCode,
                "/workspace/scout",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(running.id, WorkerSessionId::new())
            .unwrap();

        let parked = store
            .create_task_with_details(
                "Latent hub work",
                "",
                TaskPriority::Normal,
                "/workspace/hub",
            )
            .unwrap();
        store.transition_task(parked.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(parked.id, sleeping.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        // The control: same shape, on a worker that is actually running.
        let reachable = store
            .create_task_with_details("Scout work", "", TaskPriority::Normal, "/workspace/scout")
            .unwrap();
        store
            .transition_task(reachable.id, TaskState::Ready)
            .unwrap();
        store
            .assign_task_to_worker_as(
                reachable.id,
                running.id,
                &TaskActivityActor::worker(queen.id),
            )
            .unwrap();

        // The wake itself IS armed: assignment queues one for the sleeping
        // worker. So the promise in the tool description is kept up to this
        // point, and what failed was further along — the coordinator declining
        // to start anything and saying nothing about it.
        let wakes = store.claim_coordinator_worker_wakes(1_000).unwrap();
        assert!(
            wakes.iter().any(|wake| wake.worker_id == sleeping.id),
            "assignment must queue a wake for a sleeping worker: {wakes:?}"
        );

        let unreachable = store
            .work_assigned_to_a_worker_that_is_not_running()
            .unwrap();
        assert_eq!(
            unreachable.len(),
            1,
            "exactly the work that cannot be delivered: {unreachable:?}"
        );
        assert_eq!(unreachable[0].task_id, parked.id.to_string());
        assert_eq!(unreachable[0].worker_name, "RCG Hub");

        // And it stops being reported the moment the worker is running, so
        // starting it is the whole fix.
        store
            .bind_worker_session(sleeping.id, WorkerSessionId::new())
            .unwrap();
        assert!(
            store
                .work_assigned_to_a_worker_that_is_not_running()
                .unwrap()
                .is_empty(),
            "a worker that is up must not still be reported as unreachable"
        );
    }

    /// The age has to advance with the clock, and a single call cannot tell.
    ///
    /// It was written once at observation and returned verbatim ever after, so
    /// it read like a live age and was not one. Measured 2026-08-23: 303s
    /// reported for a condition 176 minutes old, and one value moved DOWNWARD
    /// between two calls. Queen read it, concluded the briefs were five minutes
    /// old and so not yet evidence of a stall, and wrote that into a live
    /// operator decision record.
    ///
    /// Two calls with a real gap between them, which is the only shape that
    /// catches it — that is why it survived.
    #[test]
    fn attention_age_advances_with_the_clock_and_never_goes_backwards() {
        let store = TaskStore::in_memory().unwrap();
        let (_, _, _task) = active_owned_work(&store, "Clover", 100);
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

        let first = store.current_coordinator_attention(1_000).unwrap();
        assert_eq!(first.len(), 1);
        let action_id = first[0].action_id.clone();
        // Not zero at the moment of observation: the work had already been
        // unchanged for fifteen minutes when the detector fired, and reporting
        // only the time SINCE would throw that away on every read.
        assert_eq!(
            first[0].age_seconds, 900,
            "the first read must carry the age the condition already had"
        );

        // Half an hour later, on the same unchanged record.
        let elapsed = 1_800;
        let second = store
            .current_coordinator_attention(1_000 + elapsed)
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].action_id, action_id, "the same record");
        assert_eq!(
            second[0].age_seconds - first[0].age_seconds,
            elapsed,
            "age must advance by the wall clock, not sit at its observation value"
        );
        assert_eq!(
            second[0].observed_at, first[0].observed_at,
            "and observed_at must not move, so the two can be reconciled"
        );

        // Never backwards, which is the anomaly actually observed.
        let mut previous = 0;
        for step in [0, 60, 900, 5_000, 10_579] {
            let age = store.current_coordinator_attention(1_000 + step).unwrap()[0].age_seconds;
            assert!(
                age >= previous,
                "age went backwards: {previous} then {age} at +{step}"
            );
            previous = age;
        }
        // The reported case: 176 minutes elapsed since the observation. It must
        // read in hours, not the 303 seconds Queen was given.
        assert_eq!(previous, 900 + 10_579);
        assert!(
            previous > 60 * 60 * 2,
            "at over two hours old it must not read as minutes: {previous}"
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
        let attention = store.current_coordinator_attention(0).unwrap();
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
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

        let attention = store.current_coordinator_attention(0).unwrap();
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());

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
        let attention = store.current_coordinator_attention(0).unwrap();
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
    }
}

/// A decision nobody answered by the time its asker said it needed one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverdueDecisionCandidate {
    pub decision_id: String,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub task_id: TaskId,
    pub task_revision: i64,
    pub title: String,
    pub overdue_seconds: i64,
}

impl TaskStore {
    /// Decisions whose deadline has passed while they are still pending.
    ///
    /// The operator's ruling: "queen should make this a needs you item." Until
    /// now a deadline was recorded, shown on the roster as "overdue", and acted
    /// on by nobody — the worker held indefinitely and Queen was never told.
    ///
    /// Two limits, both deliberate rather than overlooked. A decision with no
    /// task cannot be recorded, because a coordinator action must name one; one
    /// of sixteen on this Hive is like that, and inventing a task to hang it
    /// off would be worse than not raising it. A worker that has never held a
    /// session is skipped for the same reason — the attention inbox joins one,
    /// and a worker that has never run has not asked anything either.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn overdue_decision_candidates(
        &self,
        now: i64,
    ) -> Result<Vec<OverdueDecisionCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT request.id, request.requesting_worker_id, request.task_id, request.title,
                    task.updated_at, ?1 - request.deadline,
                    (SELECT session.session_id FROM worker_sessions session
                     WHERE session.worker_id = request.requesting_worker_id
                     ORDER BY session.started_at DESC, session.session_id DESC LIMIT 1)
             FROM decision_requests request
             JOIN tasks task ON task.id = request.task_id
             WHERE request.state = 'pending'
               AND request.deadline IS NOT NULL
               AND request.deadline <= ?1
               AND task.removed_at IS NULL
             ORDER BY request.deadline",
        )?;
        let rows = statement.query_map([now], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let (decision_id, worker_id, task_id, title, task_revision, overdue_seconds, session) =
                row?;
            let (Ok(worker_id), Ok(task_id)) =
                (WorkerId::from_str(&worker_id), TaskId::from_str(&task_id))
            else {
                continue;
            };
            let Some(session_id) = session.and_then(|id| WorkerSessionId::from_str(&id).ok())
            else {
                continue;
            };
            candidates.push(OverdueDecisionCandidate {
                decision_id,
                worker_id,
                session_id,
                task_id,
                task_revision,
                title,
                overdue_seconds,
            });
        }
        Ok(candidates)
    }

    /// Tells Queen that a decision has gone unanswered past its deadline.
    ///
    /// Re-stamps rather than duplicating. The inbox only counts an observation
    /// whose evidence still matches the task it was made against, so a task
    /// that moved on would otherwise silence an attention whose decision is
    /// still waiting — and the idempotency key would stop a second one being
    /// raised. Refreshing the revision keeps one row alive for one decision.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_overdue_decision_attention(
        &self,
        candidate: &OverdueDecisionCandidate,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // Not INSERT OR IGNORE: that suppresses a CHECK violation exactly as it
        // suppresses a duplicate, which is how an unadmitted kind once did
        // nothing and said nothing.
        let changed = transaction.execute(
            "INSERT INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'decision_deadline_passed_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', 'A decision passed the deadline its asker set',
                     unixepoch(), unixepoch())
             ON CONFLICT(idempotency_key) DO UPDATE
                 SET evidence_revision = excluded.evidence_revision,
                     observed_age_seconds = excluded.observed_age_seconds,
                     updated_at = unixepoch()
                 WHERE coordinator_actions.evidence_revision <> excluded.evidence_revision",
            params![
                Uuid::now_v7().to_string(),
                format!("decision-deadline:{}", candidate.decision_id),
                candidate.worker_id.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.overdue_seconds,
            ],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}

/// Something the coordinator wanted to do and could not, with how long it has
/// been true.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorRefusal {
    pub kind: String,
    pub subject: String,
    pub worker_id: Option<WorkerId>,
    pub worker_name: Option<String>,
    pub reason: String,
    pub first_observed_at: i64,
    pub last_observed_at: i64,
    pub observations: i64,
}

/// A delivery that cannot be written because the session has an unanswered
/// provider question.
pub const REFUSAL_DELIVERY_HELD: &str = "delivery_held_open_prompt";

/// A delivery that cannot be written because the prompt already holds text
/// that was typed and never sent.
///
/// Separate from [`REFUSAL_DELIVERY_HELD`] because the remedy is the opposite
/// one. There is no question to answer — the operator has to clear a line they
/// left behind. Told to answer a prompt that is not there, they open the
/// terminal, see nothing to do, and close it again, which is what happened for
/// three hours on 2026-08-23 while the board sat at zero active tasks.
pub const REFUSAL_DELIVERY_HELD_UNSENT_TEXT: &str = "delivery_held_unsent_text";

/// A wake whose outcome could not be confirmed, and which will not replay.
///
/// The work is assigned and was never started, and nothing about the task says
/// so — it reads as routed. Deliberately not retried: a worker woken twice gets
/// its briefing twice.
/// Assigned work whose worker is not running, so no briefing can reach it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct UnreachableAssignment {
    pub task_id: String,
    pub title: String,
    pub worker_id: String,
    pub worker_name: String,
    pub assigned_at: i64,
}

pub const REFUSAL_WAKE_UNCERTAIN: &str = "wake_uncertain";
/// The coordinator was not admitted to start anything, so queued wakes were
/// not even attempted.
pub const REFUSAL_WAKE_NOT_ADMITTED: &str = "wake_not_admitted";

impl TaskStore {
    /// Work that is assigned and cannot be reached, because the worker holding
    /// it is not running.
    ///
    /// Assignment to a sleeping worker creates no session binding and no
    /// briefing — there is nowhere to put one — so the task carries an owner
    /// and nothing else. On the board that is indistinguishable from work whose
    /// brief is simply queued behind something, which is how two tasks sat
    /// assigned and unreachable for twenty-one minutes while the operator asked
    /// why he could not see those workers in the list.
    ///
    /// Answers from `worker_sessions`, so it can say NO. The report that led
    /// here was made twice with `pgrep -af <worker id>`, which matched the
    /// reporting shell's own command line and could never return a negative.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn work_assigned_to_a_worker_that_is_not_running(
        &self,
    ) -> Result<Vec<UnreachableAssignment>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.id, task.title, worker.id, worker.name, task.updated_at
             FROM tasks task
             JOIN worker_profiles worker
               ON worker.id = task.assigned_worker_id AND worker.archived_at IS NULL
             WHERE task.state = 'ready' AND task.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions session
                   WHERE session.worker_id = worker.id AND session.ended_at IS NULL
               )
             ORDER BY task.updated_at, task.id LIMIT 32",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(UnreachableAssignment {
                task_id: row.get::<_, String>(0)?,
                title: row.get(1)?,
                worker_id: row.get::<_, String>(2)?,
                worker_name: row.get(3)?,
                assigned_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Records that the coordinator declined to act, or that it is still
    /// declining.
    ///
    /// One row per subject rather than one per attempt. A stranded prompt is
    /// retried every thirty seconds, and the useful statement is "held since
    /// 01:49, 1503 checks" rather than 1503 rows nobody will read.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn record_coordinator_refusal(
        &self,
        kind: &str,
        subject: &str,
        worker_id: Option<WorkerId>,
        session_id: Option<WorkerSessionId>,
        reason: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO coordinator_refusals
                 (kind, subject, worker_id, session_id, reason,
                  first_observed_at, last_observed_at, observations, cleared_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1, NULL)
             ON CONFLICT(kind, subject) DO UPDATE SET
                 last_observed_at = excluded.last_observed_at,
                 observations = CASE
                     WHEN coordinator_refusals.cleared_at IS NULL
                     THEN coordinator_refusals.observations + 1
                     ELSE 1
                 END,
                 -- A refusal that had cleared and is happening again is a new
                 -- occurrence, not a continuation of the old one.
                 first_observed_at = CASE
                     WHEN coordinator_refusals.cleared_at IS NULL
                     THEN coordinator_refusals.first_observed_at
                     ELSE excluded.first_observed_at
                 END,
                 reason = excluded.reason,
                 worker_id = excluded.worker_id,
                 session_id = excluded.session_id,
                 cleared_at = NULL",
            params![
                kind,
                subject,
                worker_id.map(|id| id.to_string()),
                session_id.map(|id| id.to_string()),
                reason,
                now
            ],
        )?;
        Ok(())
    }

    /// Records that whatever was blocking has stopped blocking.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn clear_coordinator_refusal(
        &self,
        kind: &str,
        subject: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE coordinator_refusals SET cleared_at = ?3
             WHERE kind = ?1 AND subject = ?2 AND cleared_at IS NULL",
            params![kind, subject, now],
        )? > 0)
    }

    /// Refusals still in force, oldest first, that have been true long enough
    /// to be worth the operator's attention.
    ///
    /// The grace period is what stops a prompt answered in ten seconds from
    /// becoming an item. Nothing here is a judgment about whether the refusal
    /// was right — refusing to type into a session with an open question is
    /// correct, and staying silent about it for a day is not.
    ///
    /// # Errors
    /// Returns a persistence error.
    /// A refusal stops standing when the coordinator stops re-observing it.
    ///
    /// Success clears a refusal, but success is not the only way one ends: a
    /// dispatch can be cancelled, the task can move, the worker can be taken
    /// away. None of those clear anything, and the query filtered on
    /// `cleared_at` alone — so a hold that stopped being retried half an hour
    /// ago was still reported as waiting, with a retry count frozen at 4.
    ///
    /// Held work is retried every few seconds, so anything genuinely held is
    /// re-observed constantly. Not having been seen for this long means it is
    /// not held now, whatever ended it.
    pub const STALE_REFUSAL_SECONDS: i64 = 180;

    /// The refusals the coordinator is still making, for the operator's queue.
    ///
    /// # Errors
    /// Returns an error when the refusal ledger cannot be read.
    pub fn standing_coordinator_refusals(
        &self,
        now: i64,
        grace_seconds: i64,
    ) -> Result<Vec<CoordinatorRefusal>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT refusal.kind, refusal.subject, refusal.worker_id, worker.name,
                    refusal.reason, refusal.first_observed_at, refusal.last_observed_at,
                    refusal.observations
             FROM coordinator_refusals refusal
             LEFT JOIN worker_profiles worker ON worker.id = refusal.worker_id
             WHERE refusal.cleared_at IS NULL
               AND ?1 - refusal.first_observed_at >= ?2
               AND ?1 - refusal.last_observed_at <= ?3
             ORDER BY refusal.first_observed_at",
        )?;
        let rows = statement.query_map(
            params![now, grace_seconds, Self::STALE_REFUSAL_SECONDS],
            |row| {
                Ok(CoordinatorRefusal {
                    kind: row.get(0)?,
                    subject: row.get(1)?,
                    worker_id: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|id| WorkerId::from_str(&id).ok()),
                    worker_name: row.get(3)?,
                    reason: row.get(4)?,
                    first_observed_at: row.get(5)?,
                    last_observed_at: row.get(6)?,
                    observations: row.get(7)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
