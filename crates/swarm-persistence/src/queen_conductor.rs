use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, PresenceMode, QueenActionClass, QueenAutomationOutcome,
    QueenAutomationState, QueenAutomationStatus, QueenAutomationTrigger, WorkerSessionId,
};
use uuid::Uuid;

use super::{TaskStore, TaskStoreError, events::insert_control_room_event};
use crate::{
    orchestration::queen_autonomy_policy_from_connection,
    presence::operator_presence_from_connection,
};

const MAX_AUTOMATION_ATTEMPTS: i64 = 3;
const MAX_FINGERPRINT_TASKS: i64 = 256;
const RUN_TIMEOUT_SECONDS: i64 = 60 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueenAutomationDelivery {
    pub run_id: String,
    pub session_id: WorkerSessionId,
    pub trigger: QueenAutomationTrigger,
    pub actionable_count: usize,
    pub presence: PresenceMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueenAutomationFailure {
    Retryable,
    Uncertain,
}

impl TaskStore {
    /// Returns bounded, content-free automation state for the operator UI.
    ///
    /// # Errors
    /// Returns an error when automation state or actionable task evidence is unavailable.
    pub fn queen_automation_status(
        &self,
        now: i64,
    ) -> Result<QueenAutomationStatus, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        expire_stale_run(&transaction, now)?;
        resume_run_delivered_to_an_ended_queen_session(&transaction, now)?;
        let (_, actionable_count) = actionable_fingerprint(&transaction)?;
        let row = transaction.query_row(
            "SELECT enabled, state, run_id, trigger, attempts, requested_at,
                    delivered_at, finished_at, outcome
             FROM queen_automation WHERE id = 1",
            [],
            |row| {
                let state = QueenAutomationState::from_str(&row.get::<_, String>(1)?)
                    .map_err(|()| rusqlite::Error::InvalidQuery)?;
                let trigger = row
                    .get::<_, Option<String>>(3)?
                    .map(|value| {
                        QueenAutomationTrigger::from_str(&value)
                            .map_err(|()| rusqlite::Error::InvalidQuery)
                    })
                    .transpose()?;
                let outcome = row
                    .get::<_, Option<String>>(8)?
                    .map(|value| parse_outcome(&value))
                    .transpose()?;
                Ok((
                    row.get::<_, bool>(0)?,
                    state,
                    row.get(2)?,
                    trigger,
                    row.get::<_, i64>(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    outcome,
                ))
            },
        )?;
        let waiting_reason = waiting_reason(&transaction, row.1, now)?;
        transaction.commit()?;
        Ok(QueenAutomationStatus {
            enabled: row.0,
            state: row.1,
            run_id: row.2,
            trigger: row.3,
            actionable_count,
            attempts: usize::try_from(row.4).unwrap_or_default(),
            requested_at: row.5,
            delivered_at: row.6,
            finished_at: row.7,
            outcome: row.8,
            waiting_reason,
        })
    }

    /// Enables or disables future automatic Queen reviews without interrupting a running turn.
    ///
    /// # Errors
    /// Returns an error when the preference or resulting durable status cannot be stored.
    pub fn set_queen_automation_enabled(
        &self,
        enabled: bool,
        now: i64,
    ) -> Result<QueenAutomationStatus, TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE queen_automation SET enabled = ?1, updated_at = ?2,
                 state = CASE WHEN ?1 = 0 AND state = 'queued' THEN 'idle' ELSE state END,
                 run_id = CASE WHEN ?1 = 0 AND state = 'queued' THEN NULL ELSE run_id END,
                 pending_fingerprint = CASE WHEN ?1 = 0 AND state = 'queued' THEN NULL ELSE pending_fingerprint END
             WHERE id = 1",
            params![enabled, now],
        )?;
        drop(connection);
        if enabled {
            self.observe_queen_automation(now)?;
        }
        self.queen_automation_status(now)
    }

    /// Queues an explicit operator-requested review even when automatic reviews are disabled.
    ///
    /// # Errors
    /// Returns an error when another run is active or the request cannot be stored atomically.
    pub fn request_queen_automation_run(
        &self,
        now: i64,
    ) -> Result<QueenAutomationStatus, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        expire_stale_run(&transaction, now)?;
        resume_run_delivered_to_an_ended_queen_session(&transaction, now)?;
        let state: String = transaction.query_row(
            "SELECT state FROM queen_automation WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        if matches!(state.as_str(), "queued" | "delivering" | "running") {
            return Err(TaskStoreError::IntegrityFailure(
                "Queen automation already has an active run".into(),
            ));
        }
        if state == "uncertain" {
            let changed = transaction.execute(
                "UPDATE queen_automation
                 SET state = 'queued', trigger = 'manual', attempts = 0,
                     attempted_at = NULL, delivered_at = NULL, finished_at = NULL,
                     outcome = NULL, updated_at = ?1
                 WHERE id = 1 AND state = 'uncertain' AND run_id IS NOT NULL",
                [now],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "uncertain Queen automation review could not be resumed".into(),
                ));
            }
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
            transaction.commit()?;
            drop(connection);
            return self.queen_automation_status(now);
        }
        let (fingerprint, _) = actionable_fingerprint(&transaction)?;
        queue_run(
            &transaction,
            &fingerprint,
            QueenAutomationTrigger::Manual,
            now,
        )?;
        transaction.commit()?;
        drop(connection);
        self.queen_automation_status(now)
    }

    /// Detects a changed actionable queue and creates at most one durable review request.
    ///
    /// # Errors
    /// Returns an error when task evidence cannot be read or the new request cannot be stored.
    pub fn observe_queen_automation(&self, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        expire_stale_run(&transaction, now)?;
        resume_run_delivered_to_an_ended_queen_session(&transaction, now)?;
        let (fingerprint, count) = actionable_fingerprint(&transaction)?;
        let (enabled, state, delivered): (bool, String, String) = transaction.query_row(
            "SELECT enabled, state, delivered_fingerprint FROM queen_automation WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let queued = enabled
            && count > 0
            && !matches!(
                state.as_str(),
                "queued" | "delivering" | "running" | "uncertain"
            )
            && fingerprint != delivered;
        if queued {
            queue_run(
                &transaction,
                &fingerprint,
                QueenAutomationTrigger::ActionableWork,
                now,
            )?;
        }
        transaction.commit()?;
        Ok(queued)
    }

    /// Atomically claims the queued review only when Queen is running and no human or Steward owns her attention.
    ///
    /// # Errors
    /// Returns an error when durable authority or Queen session state cannot be read or updated.
    pub fn claim_queen_automation(
        &self,
        now: i64,
    ) -> Result<Option<QueenAutomationDelivery>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        expire_stale_run(&transaction, now)?;
        resume_run_delivered_to_an_ended_queen_session(&transaction, now)?;
        let candidate = transaction.query_row(
            "SELECT automation.run_id, session.session_id, automation.trigger, automation.actionable_count
             FROM queen_automation automation
             JOIN worker_profiles queen ON queen.role = 'queen'
             JOIN worker_sessions session ON session.worker_id = queen.id AND session.ended_at IS NULL
             WHERE automation.id = 1 AND automation.state = 'queued' AND automation.attempts < ?1
               AND NOT EXISTS (SELECT 1 FROM worker_engagements engagement WHERE engagement.worker_id = queen.id AND engagement.expires_at > ?2)
               AND NOT EXISTS (SELECT 1 FROM local_federation_steward_takeover_leases lease WHERE lease.state = 'active' AND lease.expires_at > ?2)
             ORDER BY session.started_at DESC LIMIT 1",
            params![MAX_AUTOMATION_ATTEMPTS, now],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)),
        ).optional()?;
        let Some((run_id, session_id, trigger, count)) = candidate else {
            transaction.commit()?;
            return Ok(None);
        };
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = 'delivering', attempts = attempts + 1,
                 attempted_at = ?2, delivery_session_id = ?3, updated_at = ?2
             WHERE id = 1 AND run_id = ?1 AND state = 'queued'",
            params![run_id, now, session_id],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::IntegrityFailure(
                "Queen automation claim lost atomic ownership".into(),
            ));
        }
        let presence = operator_presence_from_connection(&transaction, now)?.mode;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(Some(QueenAutomationDelivery {
            run_id,
            session_id: session_id.parse().map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid Queen automation session".into())
            })?,
            trigger: QueenAutomationTrigger::from_str(&trigger).map_err(|()| {
                TaskStoreError::IntegrityFailure("invalid Queen automation trigger".into())
            })?,
            actionable_count: usize::try_from(count).unwrap_or_default(),
            presence,
        }))
    }

    /// Records that the exact claimed prompt reached Queen and is now running.
    ///
    /// # Errors
    /// Returns an error when the delivery marker cannot be updated atomically.
    pub fn complete_queen_automation_delivery(
        &self,
        run_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_delivery(run_id, now, None)
    }

    /// Records a bounded retryable or uncertain prompt-delivery failure.
    ///
    /// # Errors
    /// Returns an error when the delivery marker cannot be updated atomically.
    pub fn fail_queen_automation_delivery(
        &self,
        run_id: &str,
        now: i64,
        failure: QueenAutomationFailure,
    ) -> Result<bool, TaskStoreError> {
        self.finish_delivery(run_id, now, Some(failure))
    }

    /// Returns a claimed Queen prompt to its durable queue without consuming
    /// an attempt when the provider is waiting for operator input.
    ///
    /// # Errors
    /// Returns an error when the exact delivery marker cannot be updated.
    pub fn defer_queen_automation_delivery(
        &self,
        run_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE queen_automation
             SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE id = 1 AND run_id = ?1 AND state = 'delivering'",
            params![run_id, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn finish_delivery(
        &self,
        run_id: &str,
        now: i64,
        failure: Option<QueenAutomationFailure>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let next = match failure {
            None => "running",
            Some(QueenAutomationFailure::Uncertain) => "uncertain",
            Some(QueenAutomationFailure::Retryable) => {
                let attempts: i64 = transaction.query_row("SELECT attempts FROM queen_automation WHERE id = 1 AND run_id = ?1 AND state = 'delivering'", [run_id], |row| row.get(0))?;
                if attempts >= MAX_AUTOMATION_ATTEMPTS {
                    "uncertain"
                } else {
                    "queued"
                }
            }
        };
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = ?2, delivered_at = CASE WHEN ?2 = 'running' THEN ?3 ELSE delivered_at END,
                 updated_at = ?3 WHERE id = 1 AND run_id = ?1 AND state = 'delivering'",
            params![run_id, next, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Closes the exact automation turn from Queen's authenticated MCP identity.
    ///
    /// # Errors
    /// Returns an error when the exact running marker cannot be read or updated.
    pub fn finish_queen_automation_run(
        &self,
        run_id: &str,
        outcome: QueenAutomationOutcome,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = 'completed', outcome = ?2, finished_at = ?3,
                 delivered_fingerprint = pending_fingerprint, pending_fingerprint = NULL, updated_at = ?3
             WHERE id = 1 AND run_id = ?1 AND state = 'running'",
            params![run_id, outcome.to_string(), now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Applies the presence ceiling only while an unattended automation marker is active.
    ///
    /// # Errors
    /// Returns an error when automation, presence, or autonomy policy state is unavailable.
    pub fn queen_automation_permits(
        &self,
        action: QueenActionClass,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let running: bool = connection.query_row(
            "SELECT state = 'running' FROM queen_automation WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        if !running {
            return Ok(true);
        }
        let policy = queen_autonomy_policy_from_connection(&connection)?;
        let presence = operator_presence_from_connection(&connection, now)?.mode;
        Ok(policy.permits(presence, action, false))
    }

    /// Converts API-interrupted delivery or execution into explicit uncertainty without replay.
    ///
    /// # Errors
    /// Returns an error when the durable marker cannot be recovered.
    pub fn recover_inflight_queen_automation(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE queen_automation SET state = 'uncertain', updated_at = unixepoch()
             WHERE state IN ('delivering', 'running')",
            [],
        )?)
    }
}

fn queue_run(
    transaction: &rusqlite::Transaction<'_>,
    fingerprint: &str,
    trigger: QueenAutomationTrigger,
    now: i64,
) -> Result<(), TaskStoreError> {
    let count = actionable_count(transaction)?;
    transaction.execute(
        "UPDATE queen_automation SET state = 'queued', run_id = ?1, trigger = ?2,
             pending_fingerprint = ?3, actionable_count = ?4, attempts = 0,
             requested_at = ?5, attempted_at = NULL, delivered_at = NULL,
             finished_at = NULL, outcome = NULL, updated_at = ?5 WHERE id = 1",
        params![
            Uuid::now_v7().to_string(),
            trigger.to_string(),
            fingerprint,
            count,
            now
        ],
    )?;
    insert_control_room_event(transaction, ControlRoomEventKind::WorkersChanged)?;
    Ok(())
}

fn actionable_count(connection: &rusqlite::Connection) -> Result<i64, rusqlite::Error> {
    let task_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM tasks WHERE removed_at IS NULL AND (state IN ('blocked','review') OR (state = 'ready' AND assigned_worker_id IS NULL))",
        [], |row| row.get(0),
    )?;
    let coordination_attention_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM coordinator_actions action
         JOIN tasks task ON task.id = action.task_id
         JOIN worker_sessions session ON session.session_id = action.session_id
         WHERE action.kind IN ('stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention')
           AND action.state = 'completed'
           AND task.assigned_worker_id = action.worker_id
           AND task.updated_at = action.evidence_revision AND session.worker_id = action.worker_id
           AND (
               (action.kind = 'stale_owned_work_attention'
                   AND task.state = 'active' AND session.ended_at IS NULL)
               OR (action.kind = 'owned_work_worker_exited_attention'
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
                   AND task.state = 'ready' AND session.ended_at IS NULL
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
           )",
        [],
        |row| row.get(0),
    )?;
    Ok(task_count + coordination_attention_count)
}

fn actionable_fingerprint(
    connection: &rusqlite::Connection,
) -> Result<(String, usize), TaskStoreError> {
    let count = actionable_count(connection)?;
    let mut statement = connection.prepare(
        "SELECT task.id, task.state, COALESCE(MAX(activity.sequence), 0)
         FROM tasks task LEFT JOIN task_activity activity ON activity.task_id = task.id
         WHERE task.removed_at IS NULL AND (task.state IN ('blocked','review') OR (task.state = 'ready' AND task.assigned_worker_id IS NULL))
         GROUP BY task.id, task.state ORDER BY task.id LIMIT ?1",
    )?;
    let mut rows = statement
        .query_map([MAX_FINGERPRINT_TASKS], |row| {
            Ok(format!(
                "{}:{}:{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut attention_statement = connection.prepare(
        "SELECT action.kind, action.id, task.id, action.evidence_revision
         FROM coordinator_actions action
         JOIN tasks task ON task.id = action.task_id
         JOIN worker_sessions session ON session.session_id = action.session_id
         WHERE action.kind IN ('stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention')
           AND action.state = 'completed'
           AND task.assigned_worker_id = action.worker_id
           AND task.updated_at = action.evidence_revision AND session.worker_id = action.worker_id
           AND (
               (action.kind = 'stale_owned_work_attention'
                   AND task.state = 'active' AND session.ended_at IS NULL)
               OR (action.kind = 'owned_work_worker_exited_attention'
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
                   AND task.state = 'ready' AND session.ended_at IS NULL
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
           )
         ORDER BY action.id LIMIT ?1",
    )?;
    rows.extend(
        attention_statement
            .query_map([MAX_FINGERPRINT_TASKS], |row| {
                Ok(format!(
                    "attention:{}:{}:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok((
        format!("{}|{}", count, rows.join("|")),
        usize::try_from(count).unwrap_or_default(),
    ))
}

/// Resumes a review whose delivery was written to a Queen terminal that has
/// since ended.
///
/// An uncertain run is normally an operator judgment: Swarm could not confirm
/// the review reached Queen, and replaying it blindly could double a briefing.
/// The exact session it was written to having ended removes the ambiguity,
/// because that terminal no longer exists and cannot be read from. Queen is
/// never told the run id in that case, so she cannot finish the run herself and
/// the review would otherwise wait for an operator forever.
///
/// This is deliberately keyed on session identity rather than on comparing an
/// attempt time to a session start. Identity is exact, and the lifecycle
/// already validates the exact session elsewhere for the same reason.
fn resume_run_delivered_to_an_ended_queen_session(
    transaction: &rusqlite::Transaction<'_>,
    now: i64,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE queen_automation
         SET state = 'queued', attempts = 0, attempted_at = NULL,
             delivery_session_id = NULL, delivered_at = NULL, finished_at = NULL,
             outcome = NULL, updated_at = ?1
         WHERE id = 1 AND state = 'uncertain' AND run_id IS NOT NULL
           AND delivery_session_id IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM worker_sessions session
               WHERE session.session_id = queen_automation.delivery_session_id
                 AND session.ended_at IS NULL
           )",
        [now],
    )?;
    if changed > 0 {
        insert_control_room_event(transaction, ControlRoomEventKind::WorkersChanged)?;
    }
    Ok(())
}

fn expire_stale_run(
    transaction: &rusqlite::Transaction<'_>,
    now: i64,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE queen_automation SET state = 'uncertain', updated_at = ?1
         WHERE state = 'running' AND delivered_at IS NOT NULL AND delivered_at + ?2 <= ?1",
        params![now, RUN_TIMEOUT_SECONDS],
    )?;
    if changed > 0 {
        insert_control_room_event(transaction, ControlRoomEventKind::WorkersChanged)?;
    }
    Ok(())
}

fn waiting_reason(
    connection: &rusqlite::Connection,
    state: QueenAutomationState,
    now: i64,
) -> Result<Option<String>, TaskStoreError> {
    if state != QueenAutomationState::Queued {
        return Ok(None);
    }
    let engaged: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles queen JOIN worker_engagements engagement ON engagement.worker_id = queen.id WHERE queen.role = 'queen' AND engagement.expires_at > ?1)",
        [now], |row| row.get(0),
    )?;
    if engaged {
        return Ok(Some("Waiting while you are working with Queen".into()));
    }
    let takeover: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM local_federation_steward_takeover_leases WHERE state = 'active' AND expires_at > ?1)",
        [now], |row| row.get(0),
    )?;
    if takeover {
        return Ok(Some("Paused during Steward takeover".into()));
    }
    let running: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles queen JOIN worker_sessions session ON session.worker_id = queen.id WHERE queen.role = 'queen' AND session.ended_at IS NULL)",
        [], |row| row.get(0),
    )?;
    Ok((!running).then(|| "Waiting for Queen to wake".into()))
}

fn parse_outcome(value: &str) -> Result<QueenAutomationOutcome, rusqlite::Error> {
    match value {
        "completed" => Ok(QueenAutomationOutcome::Completed),
        "needs_operator" => Ok(QueenAutomationOutcome::NeedsOperator),
        "no_action" => Ok(QueenAutomationOutcome::NoAction),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub(super) fn migrate_queen_conductor(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_automation (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0,1)),
             state TEXT NOT NULL DEFAULT 'idle' CHECK (state IN ('idle','queued','delivering','running','completed','uncertain')),
             run_id TEXT,
             trigger TEXT CHECK (trigger IS NULL OR trigger IN ('actionable_work','manual')),
             delivered_fingerprint TEXT NOT NULL DEFAULT '',
             pending_fingerprint TEXT,
             actionable_count INTEGER NOT NULL DEFAULT 0,
             attempts INTEGER NOT NULL DEFAULT 0,
             requested_at INTEGER,
             attempted_at INTEGER,
             delivered_at INTEGER,
             finished_at INTEGER,
             outcome TEXT CHECK (outcome IS NULL OR outcome IN ('completed','needs_operator','no_action')),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT OR IGNORE INTO queen_automation (id) VALUES (1);
         PRAGMA user_version = 61;"
    )?;
    let has_delivery_session = transaction
        .prepare("PRAGMA table_info(queen_automation)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|column| column == "delivery_session_id");
    if !has_delivery_session {
        transaction
            .execute_batch("ALTER TABLE queen_automation ADD COLUMN delivery_session_id TEXT;")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskActivityActor, TaskPriority, TaskState};

    #[test]
    fn opt_in_queues_changed_work_and_defers_while_operator_is_engaged() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        let task = store
            .create_task_with_details("Route this", "", TaskPriority::Normal, "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let status = store.set_queen_automation_enabled(true, 100).unwrap();
        assert_eq!(status.state, QueenAutomationState::Queued);
        let device = swarm_domain::PresenceDeviceId::new();
        store
            .renew_worker_engagement(session, Some(device), 100, 300)
            .unwrap();
        assert!(store.claim_queen_automation(101).unwrap().is_none());
        store.release_worker_engagement(session, device).unwrap();
    }

    #[test]
    fn acknowledged_run_enforces_presence_policy_until_queen_finishes_exact_marker() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();
        assert!(
            store
                .complete_queen_automation_delivery(&delivery.run_id, 12)
                .unwrap()
        );
        assert!(
            store
                .queen_automation_permits(QueenActionClass::Coordinate, 13)
                .unwrap()
        );
        assert!(
            !store
                .queen_automation_permits(QueenActionClass::ModifyWorkspace, 13)
                .unwrap()
        );
        assert!(
            store
                .finish_queen_automation_run(&delivery.run_id, QueenAutomationOutcome::NoAction, 14)
                .unwrap()
        );
        assert!(
            store
                .queen_automation_permits(QueenActionClass::ExternalSideEffect, 15)
                .unwrap()
        );
    }

    #[test]
    fn api_interrupted_delivery_becomes_uncertain_without_replay() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(10).unwrap();
        store.claim_queen_automation(11).unwrap().unwrap();
        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
        assert_eq!(
            store.queen_automation_status(12).unwrap().state,
            QueenAutomationState::Uncertain
        );
        assert!(store.claim_queen_automation(12).unwrap().is_none());
    }

    #[test]
    fn a_review_written_to_an_ended_queen_terminal_resumes_without_an_operator() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        let requested = store.request_queen_automation_run(10).unwrap();
        let run_id = requested.run_id.unwrap();
        store.claim_queen_automation(11).unwrap().unwrap();
        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
        assert_eq!(
            store.queen_automation_status(12).unwrap().state,
            QueenAutomationState::Uncertain
        );

        // The terminal the review was written to is gone, so the delivery
        // provably was not read and Queen was never told the run id.
        store.release_worker_session(session).unwrap();
        let resumed = store.queen_automation_status(13).unwrap();

        assert_eq!(resumed.state, QueenAutomationState::Queued);
        assert_eq!(resumed.run_id.as_deref(), Some(run_id.as_str()));
    }

    #[test]
    fn api_restart_recovers_a_running_review_for_operator_retry() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let requested = store.request_queen_automation_run(10).unwrap();
        let original_run_id = requested.run_id.unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();
        assert!(
            store
                .complete_queen_automation_delivery(&delivery.run_id, 12)
                .unwrap()
        );

        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
        let interrupted = store.queen_automation_status(13).unwrap();
        assert_eq!(interrupted.state, QueenAutomationState::Uncertain);
        assert_eq!(
            interrupted.run_id.as_deref(),
            Some(original_run_id.as_str())
        );

        let resumed = store.request_queen_automation_run(14).unwrap();
        assert_eq!(resumed.state, QueenAutomationState::Queued);
        assert_eq!(resumed.run_id.as_deref(), Some(original_run_id.as_str()));
    }

    #[test]
    fn operator_retry_preserves_interrupted_run_identity_until_queen_finishes() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let requested = store.request_queen_automation_run(10).unwrap();
        let original_run_id = requested.run_id.unwrap();
        let claimed = store.claim_queen_automation(11).unwrap().unwrap();
        assert_eq!(claimed.run_id, original_run_id);
        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);

        let resumed = store.request_queen_automation_run(12).unwrap();
        assert_eq!(resumed.state, QueenAutomationState::Queued);
        assert_eq!(resumed.run_id.as_deref(), Some(original_run_id.as_str()));
        assert_eq!(resumed.attempts, 0);

        let retried = store.claim_queen_automation(13).unwrap().unwrap();
        assert_eq!(retried.run_id, original_run_id);
        assert!(
            store
                .complete_queen_automation_delivery(&original_run_id, 14)
                .unwrap()
        );
        assert!(
            store
                .finish_queen_automation_run(
                    &original_run_id,
                    QueenAutomationOutcome::Completed,
                    15,
                )
                .unwrap()
        );
        assert_eq!(
            store.queen_automation_status(16).unwrap().state,
            QueenAutomationState::Completed
        );
    }

    #[test]
    fn current_stale_attention_enters_the_bounded_queen_review_fingerprint() {
        let store = TaskStore::in_memory().unwrap();
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
            .create_task("Finish the release", "/workspace/clover")
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
                "UPDATE tasks SET updated_at = 100 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
        let candidate = store
            .stale_owned_work_candidates(1_000, 600)
            .unwrap()
            .pop()
            .unwrap();
        store
            .record_stale_owned_work_attention(&candidate, 1_000, 600)
            .unwrap();

        let status = store.set_queen_automation_enabled(true, 1_001).unwrap();
        assert_eq!(status.actionable_count, 1);
        assert_eq!(status.state, QueenAutomationState::Queued);

        store
            .transition_task_with_note(task.id, TaskState::Review, "Ready")
            .unwrap();
        assert!(store.current_coordinator_attention().unwrap().is_empty());
        let status = store.queen_automation_status(1_002).unwrap();
        assert_eq!(status.actionable_count, 1);
    }

    #[test]
    fn delivered_ready_work_not_started_enters_and_leaves_the_queen_review_fingerprint() {
        let store = TaskStore::in_memory().unwrap();
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
            .create_task("Start the delivered task", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        let dispatch = store.claim_task_dispatches(100).unwrap().remove(0);
        store
            .complete_task_dispatch(&dispatch.assignment_id, 101)
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at = 90 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
        let candidate = store
            .assigned_ready_work_not_started_candidates(401, 300)
            .unwrap()
            .pop()
            .unwrap();
        store
            .record_assigned_ready_work_not_started_attention(&candidate, 401, 300)
            .unwrap();

        let status = store.set_queen_automation_enabled(true, 402).unwrap();
        assert_eq!(status.actionable_count, 1);
        assert_eq!(status.state, QueenAutomationState::Queued);

        store.transition_task(task.id, TaskState::Active).unwrap();
        assert!(store.current_coordinator_attention().unwrap().is_empty());
        assert_eq!(
            store.queen_automation_status(403).unwrap().actionable_count,
            0
        );
    }

    #[test]
    fn exited_worker_owned_work_enters_and_leaves_the_queen_review_fingerprint() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Aster",
                ProviderKind::ClaudeCode,
                "/workspace/aster",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Recover interrupted work", "/workspace/aster")
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
                "UPDATE tasks SET updated_at = 100 WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
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
            .record_exited_worker_owned_work_attention(&candidate, 700, 300)
            .unwrap();

        let status = store.set_queen_automation_enabled(true, 701).unwrap();
        assert_eq!(status.actionable_count, 1);
        assert_eq!(status.state, QueenAutomationState::Queued);

        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        assert!(store.current_coordinator_attention().unwrap().is_empty());
        assert_eq!(
            store.queen_automation_status(702).unwrap().actionable_count,
            0
        );
    }
}
