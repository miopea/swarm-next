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
                 attempted_at = ?2, updated_at = ?2 WHERE id = 1 AND run_id = ?1 AND state = 'queued'",
            params![run_id, now],
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

    /// Converts crash-interrupted delivery into explicit uncertainty without replay.
    ///
    /// # Errors
    /// Returns an error when the durable marker cannot be recovered.
    pub fn recover_inflight_queen_automation(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute("UPDATE queen_automation SET state = 'uncertain', updated_at = unixepoch() WHERE state = 'delivering'", [])?)
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
        "SELECT COUNT(*) FROM tasks WHERE state IN ('blocked','review') OR (state = 'ready' AND assigned_worker_id IS NULL)",
        [], |row| row.get(0),
    )?;
    let stale_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM coordinator_actions action
         JOIN tasks task ON task.id = action.task_id
         JOIN worker_sessions session ON session.session_id = action.session_id AND session.ended_at IS NULL
         WHERE action.kind = 'stale_owned_work_attention' AND action.state = 'completed'
           AND task.state = 'active' AND task.assigned_worker_id = action.worker_id
           AND task.updated_at = action.evidence_revision AND session.worker_id = action.worker_id",
        [],
        |row| row.get(0),
    )?;
    Ok(task_count + stale_count)
}

fn actionable_fingerprint(
    connection: &rusqlite::Connection,
) -> Result<(String, usize), TaskStoreError> {
    let count = actionable_count(connection)?;
    let mut statement = connection.prepare(
        "SELECT task.id, task.state, COALESCE(MAX(activity.sequence), 0)
         FROM tasks task LEFT JOIN task_activity activity ON activity.task_id = task.id
         WHERE task.state IN ('blocked','review') OR (task.state = 'ready' AND task.assigned_worker_id IS NULL)
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
        "SELECT action.id, task.id, action.evidence_revision
         FROM coordinator_actions action
         JOIN tasks task ON task.id = action.task_id
         JOIN worker_sessions session ON session.session_id = action.session_id AND session.ended_at IS NULL
         WHERE action.kind = 'stale_owned_work_attention' AND action.state = 'completed'
           AND task.state = 'active' AND task.assigned_worker_id = action.worker_id
           AND task.updated_at = action.evidence_revision AND session.worker_id = action.worker_id
         ORDER BY action.id LIMIT ?1",
    )?;
    rows.extend(
        attention_statement
            .query_map([MAX_FINGERPRINT_TASKS], |row| {
                Ok(format!(
                    "stale:{}:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok((
        format!("{}|{}", count, rows.join("|")),
        usize::try_from(count).unwrap_or_default(),
    ))
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
    )
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
    fn crash_interrupted_delivery_becomes_uncertain_without_replay() {
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
}
