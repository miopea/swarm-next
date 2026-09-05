use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, PresenceMode, QueenActionClass, QueenAutomationOutcome,
    QueenAutomationState, QueenAutomationStatus, QueenAutomationTrigger, WorkerId, WorkerSessionId,
};
use uuid::Uuid;

use super::{
    QUEEN_DELIVERY_SESSION_SCHEMA_VERSION, TaskStore, TaskStoreError,
    events::insert_control_room_event,
};
use crate::{
    orchestration::queen_autonomy_policy_from_connection,
    presence::operator_presence_from_connection,
};

const MAX_AUTOMATION_ATTEMPTS: i64 = 3;
/// How long a verdict on an unchanged board stands before Queen looks again.
///
/// Bounds the cost of re-reading: at worst four runs an hour, against a Hive
/// that otherwise stops until a human notices. Only applies while there is
/// actionable work, so a genuinely empty board stays quiet.
const RECHECK_UNCHANGED_BOARD_SECONDS: i64 = 15 * 60;
const MAX_FINGERPRINT_TASKS: i64 = 256;
const MAX_FINGERPRINT_MESSAGE_DELIVERIES: i64 = 64;
const RUN_TIMEOUT_SECONDS: i64 = 60 * 60;
/// How long an unsettleable uncertain run blocks automation before it is
/// abandoned in favour of a fresh one.
///
/// Long enough that settling and resuming — both of which are exact — get their
/// chance first, and short enough that a night does not die on one restart.
const ABANDON_UNSETTLED_UNCERTAIN_SECONDS: i64 = 30 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueenAutomationDelivery {
    pub run_id: String,
    pub session_id: WorkerSessionId,
    /// Queen herself. Carried so that a refusal recorded against this delivery
    /// can name the worker the operator has to go and look at — a held item
    /// with no worker on it renders as a sentence with nothing to open.
    pub worker_id: WorkerId,
    pub trigger: QueenAutomationTrigger,
    pub actionable_count: usize,
    pub presence: PresenceMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueenAutomationFailure {
    Retryable,
    Uncertain,
}

/// Why a finish call did not close the run, so the caller is told which of
/// those it was rather than that no run exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueenAutomationFinish {
    Closed,
    /// The run is the current one, but its marker is in a state the finish does
    /// not cover — already completed, or still queued or delivering.
    WrongState {
        state: String,
    },
    /// A different run is current. The caller is holding an id from an earlier
    /// turn, which is the shape a stale prompt in scrollback produces.
    DifferentRun {
        current: String,
    },
    /// No run has ever been recorded.
    NoRun,
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
        abandon_unsettled_uncertain_run(&transaction, now)?;
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
        abandon_unsettled_uncertain_run(&transaction, now)?;
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
        abandon_unsettled_uncertain_run(&transaction, now)?;
        let (fingerprint, count) = actionable_fingerprint(&transaction)?;
        let (enabled, state, delivered, finished_at): (bool, String, String, Option<i64>) =
            transaction.query_row(
                "SELECT enabled, state, delivered_fingerprint, finished_at
                 FROM queen_automation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        // "I already looked at this" expires.
        //
        // The fingerprint gate stops Queen re-reading an unchanged board on
        // every tick, which is right. What it also did was make a no_action
        // verdict permanent: the board only changes when somebody acts on it,
        // and Queen is who acts, so a Hive holding work she declined once sat
        // still indefinitely. Measured 2026-08-24: she ran at 01:30, returned
        // no_action, and was still idle at 01:49 with 22 tasks ready, 3 in
        // review and several workers doing nothing.
        //
        // Judgement is not a pure function of the board. Workers finish, a
        // review becomes answerable, her own tools change — none of which move
        // the fingerprint. So an unchanged board is worth another look after a
        // while, and only while there is something actionable to look at.
        let looked_recently = finished_at
            .is_some_and(|finished| now.saturating_sub(finished) < RECHECK_UNCHANGED_BOARD_SECONDS);
        let queued = enabled
            && count > 0
            && !matches!(
                state.as_str(),
                "queued" | "delivering" | "running" | "uncertain"
            )
            && (fingerprint != delivered || !looked_recently);
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
        abandon_unsettled_uncertain_run(&transaction, now)?;
        let candidate = transaction.query_row(
            "SELECT automation.run_id, session.session_id, automation.trigger, automation.actionable_count, queen.id
             FROM queen_automation automation
             JOIN worker_profiles queen ON queen.role = 'queen'
             JOIN worker_sessions session ON session.worker_id = queen.id AND session.ended_at IS NULL
             WHERE automation.id = 1 AND automation.state = 'queued' AND automation.attempts < ?1
               AND NOT EXISTS (SELECT 1 FROM worker_engagements engagement WHERE engagement.worker_id = queen.id AND engagement.expires_at > ?2)
               AND NOT EXISTS (SELECT 1 FROM local_federation_steward_takeover_leases lease WHERE lease.state = 'active' AND lease.expires_at > ?2)
             ORDER BY session.started_at DESC LIMIT 1",
            params![MAX_AUTOMATION_ATTEMPTS, now],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, String>(4)?)),
        ).optional()?;
        let Some((run_id, session_id, trigger, queued_count, queen_id)) = candidate else {
            transaction.commit()?;
            return Ok(None);
        };
        // The board as it is now, not as it was when the run was queued.
        //
        // A run waits queued until Queen is running and nobody else owns her
        // attention, which can be a long time: the run that prompted this was
        // requested at 21:14 and delivered at 15:20 the next day. The prompt
        // carried the count from queue time, so it told her to review four
        // records when one had been completed hours earlier and another was an
        // attention false positive that had since been fixed. She was sent to
        // look at yesterday's queue.
        let count = actionable_count(&transaction)?;
        let (fingerprint, _) = actionable_fingerprint(&transaction)?;
        // A manual run is the operator asking, and an empty board is not a
        // reason to refuse them: they may want her to look at something Swarm
        // does not count as actionable. Only a run this Hive queued by itself
        // is abandoned when what triggered it is gone.
        if count == 0 && trigger != QueenAutomationTrigger::Manual.to_string() {
            // Everything actionable was handled while this waited. Waking Queen
            // to review nothing is the "she says she is buzzing and her terminal
            // is idle" complaint, so the run closes itself instead. Recorded as
            // a real outcome rather than discarded: it did finish, and the
            // fingerprint it finishes on is the empty board it actually found.
            transaction.execute(
                "UPDATE queen_automation
                    SET state = 'completed', outcome = 'no_action', finished_at = ?2,
                        delivered_fingerprint = ?3, pending_fingerprint = NULL,
                        actionable_count = 0, updated_at = ?2
                  WHERE id = 1 AND run_id = ?1 AND state = 'queued'",
                params![run_id, now, fingerprint],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
            transaction.commit()?;
            return Ok(None);
        }
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = 'delivering', attempts = attempts + 1,
                 attempted_at = ?2, delivery_session_id = ?3,
                 actionable_count = ?4, pending_fingerprint = ?5, updated_at = ?2
             WHERE id = 1 AND run_id = ?1 AND state = 'queued'",
            params![run_id, now, session_id, count, fingerprint],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::IntegrityFailure(
                "Queen automation claim lost atomic ownership".into(),
            ));
        }
        let presence = operator_presence_from_connection(&transaction, now)?.mode;
        // Internal delivery ownership does not mean Queen accepted the work.
        // Still publish a changed board count while a run was waiting.
        if count != queued_count {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(Some(QueenAutomationDelivery {
            run_id,
            session_id: session_id.parse().map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid Queen automation session".into())
            })?,
            worker_id: queen_id
                .parse()
                .map_err(|_| TaskStoreError::IntegrityFailure("invalid Queen worker id".into()))?,
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

    /// The run and session of an uncertain delivery whose terminal is still
    /// live, so the delivery can be checked rather than waited on.
    ///
    /// Returns nothing when no run is uncertain, or when the session it was
    /// written to has ended — that case is already resumed on its own, because
    /// a terminal that no longer exists cannot be read.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or holds an invalid ID.
    pub fn uncertain_queen_delivery(
        &self,
    ) -> Result<Option<(String, WorkerSessionId)>, TaskStoreError> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT a.run_id, a.delivery_session_id
                 FROM queen_automation a
                 JOIN worker_sessions s ON s.session_id = a.delivery_session_id
                     AND s.ended_at IS NULL
                 WHERE a.id = 1 AND a.state = 'uncertain' AND a.run_id IS NOT NULL",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        row.map(|(run_id, session)| {
            WorkerSessionId::from_str(&session)
                .map(|session| (run_id, session))
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
        })
        .transpose()
    }

    /// Resolves an uncertain run whose delivery is now known to have landed.
    ///
    /// Uncertainty here means Swarm could not *confirm* the review reached
    /// Queen, not that it failed. The prompt carries the run id, so finding it
    /// in the terminal it was written to settles the question: Queen has it and
    /// can finish the run herself.
    ///
    /// Keyed on the exact run and the exact session it was delivered to, so a
    /// marker from an older run in the same scrollback cannot resolve a newer
    /// one.
    ///
    /// # Errors
    /// Returns an error when the delivery marker cannot be updated atomically.
    pub fn confirm_queen_automation_delivered(
        &self,
        run_id: &str,
        session_id: WorkerSessionId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE queen_automation
             SET state = 'running', delivered_at = COALESCE(delivered_at, ?3), updated_at = ?3
             WHERE id = 1 AND run_id = ?1 AND state = 'uncertain'
               AND delivery_session_id = ?2",
            params![run_id, session_id.to_string(), now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
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
        // The same review remains pending; the refusal owner publishes any
        // changed blocker. Rechecking it must not refresh the whole Hive.
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
    ) -> Result<QueenAutomationFinish, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // "I need the operator" is a claim about something they can act on, so
        // it only holds while something is actually waiting for them. Queen
        // reported it with no pending request of her own and the control room
        // then said she had "filed a request and stopped" when she had filed
        // nothing — a card with nothing behind it, which came back on every run
        // and could not be resolved by opening her.
        //
        // Downgraded rather than refused: the run really did finish, and
        // leaving it stuck in 'running' to punish a bad outcome would be worse
        // than recording what actually happened.
        let outcome = match outcome {
            QueenAutomationOutcome::NeedsOperator => {
                let waiting: bool = transaction.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM decision_requests decision
                         JOIN worker_profiles worker ON worker.id = decision.requesting_worker_id
                         WHERE decision.state = 'pending' AND worker.role = 'queen'
                     )",
                    [],
                    |row| row.get(0),
                )?;
                if waiting {
                    QueenAutomationOutcome::NeedsOperator
                } else {
                    QueenAutomationOutcome::NoAction
                }
            }
            other => other,
        };
        // Accepted from `uncertain` as well as `running`, and that is the
        // point rather than a loosening.
        //
        // recover_inflight_queen_automation marks a run uncertain whenever the
        // API restarts, which now happens on every app update. Queen's session
        // survives that — the terminal host is a separate service — so she
        // finishes the review she was actually given and the finish was
        // refused because the state had moved underneath her. The run could
        // then never be closed, and its stale fingerprint re-triggered it.
        //
        // Her report is the evidence the delivery landed. Nothing else Swarm
        // can observe settles the uncertainty as well as the recipient saying
        // what it did with the work, and the run_id is a token she only holds
        // because it was delivered to her.
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = 'completed', outcome = ?2, finished_at = ?3,
                 delivered_fingerprint = pending_fingerprint, pending_fingerprint = NULL, updated_at = ?3
             WHERE id = 1 AND run_id = ?1 AND state IN ('running', 'uncertain')",
            params![run_id, outcome.to_string(), now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
            transaction.commit()?;
            return Ok(QueenAutomationFinish::Closed);
        }
        // Why it was refused, rather than a flat denial that the run exists.
        //
        // "No matching active Queen automation run" was false on its face: the
        // run existed and was simply in a state the update did not cover.
        // Queen spent three calls and a database read to learn that, and the
        // answer was in the row the whole time.
        let marker: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT state, run_id FROM queen_automation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        transaction.commit()?;
        Ok(match marker {
            Some((state, Some(current))) if current == run_id => {
                QueenAutomationFinish::WrongState { state }
            }
            Some((_, Some(current))) => QueenAutomationFinish::DifferentRun { current },
            Some((_, None)) | None => QueenAutomationFinish::NoRun,
        })
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
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE queen_automation SET state = 'uncertain', updated_at = unixepoch()
             WHERE state IN ('delivering', 'running')",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
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

/// Work Queen has something to do about, as one definition.
///
/// Blocked and review work needs her judgment; unassigned ready work needs
/// routing. So does ready work assigned to a worker that is not running —
/// which nothing counted, so nothing noticed. A wake is queued once, at
/// assignment, and never again; when one came back uncertain the work sat
/// assigned to a sleeping worker indefinitely, invisible to every detector,
/// because they all begin from a live session that does not exist.
/// What is worth waking Queen for.
///
/// Drafts are in this set. A draft is by definition un-triaged — it is work
/// nobody has decided about yet — and Queen deciding is the whole point of her
/// review. Leaving them out meant a board could fill with them and still read
/// as empty: on 2026-08-23 the Hive sat idle for eight hours holding 22 drafts,
/// the oldest five days old, because the only three actionable records had been
/// reviewed once and nothing had changed since.
const ACTIONABLE_TASKS: &str = "task.removed_at IS NULL
             AND (
                 task.state IN ('draft','blocked','review')
                 OR (task.state = 'active' AND EXISTS (
                     SELECT 1 FROM task_prerequisites p LEFT JOIN tasks upstream ON upstream.id = p.prerequisite_id
                     WHERE p.task_id = task.id AND (upstream.id IS NULL OR upstream.removed_at IS NOT NULL OR upstream.state != 'completed')
                 ))
                 OR (task.state = 'ready' AND task.assigned_worker_id IS NULL)
                 OR (task.state = 'ready'
                     AND task.assigned_worker_id IS NOT NULL
                     AND NOT EXISTS (
                         SELECT 1 FROM worker_sessions live
                         WHERE live.worker_id = task.assigned_worker_id
                           AND live.ended_at IS NULL
                     ))
             )";

fn actionable_count(connection: &rusqlite::Connection) -> Result<i64, rusqlite::Error> {
    let task_count: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM tasks task WHERE {ACTIONABLE_TASKS}"),
        [],
        |row| row.get(0),
    )?;
    let coordination_attention_count: i64 = connection.query_row(
        &format!(
            "SELECT COUNT(*) {}",
            crate::coordinator::LIVE_ATTENTION_SOURCE
        ),
        [],
        |row| row.get(0),
    )?;
    let message_attention_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM task_message_deliveries WHERE state IN ('uncertain','rejected')",
        [],
        |row| row.get(0),
    )?;
    Ok(task_count + coordination_attention_count + message_attention_count)
}

fn actionable_fingerprint(
    connection: &rusqlite::Connection,
) -> Result<(String, usize), TaskStoreError> {
    let count = actionable_count(connection)?;
    let mut statement = connection.prepare(&format!(
        "SELECT task.id, task.state, COALESCE(MAX(activity.sequence), 0)
         FROM tasks task LEFT JOIN task_activity activity ON activity.task_id = task.id
         WHERE {ACTIONABLE_TASKS}
         GROUP BY task.id, task.state ORDER BY task.id LIMIT ?1"
    ))?;
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
    let mut attention_statement = connection.prepare(&format!(
        "SELECT action.kind, action.id, task.id, action.evidence_revision
         {source}
         ORDER BY action.id LIMIT ?1",
        source = crate::coordinator::LIVE_ATTENTION_SOURCE,
    ))?;
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
    let mut messages = connection.prepare(
        "SELECT message_id, claim_id, state, superseded FROM task_message_deliveries
         WHERE state IN ('uncertain','rejected') ORDER BY updated_at, message_id LIMIT ?1",
    )?;
    rows.extend(
        messages
            .query_map([MAX_FINGERPRINT_MESSAGE_DELIVERIES], |row| {
                Ok(format!(
                    "message:{}:{}:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?,
    );
    rows.push(prerequisite_fingerprint(connection)?);
    Ok((
        format!("{}|{}", count, rows.join("|")),
        usize::try_from(count).unwrap_or_default(),
    ))
}

/// Upstream changes can unblock work without changing the blocked task's own
/// activity. Hash only bounded identity/state/owner facts, not task content.
fn prerequisite_fingerprint(connection: &rusqlite::Connection) -> Result<String, TaskStoreError> {
    use sha2::{Digest, Sha256};
    let mut query = connection.prepare(
        "SELECT p.task_id, p.prerequisite_id, COALESCE(upstream.state, 'missing'),
                upstream.removed_at IS NOT NULL, COALESCE(upstream.assigned_worker_id, '')
         FROM task_prerequisites p JOIN tasks source ON source.id = p.task_id
         LEFT JOIN tasks upstream ON upstream.id = p.prerequisite_id
         WHERE source.removed_at IS NULL AND source.state NOT IN ('completed','abandoned')
         ORDER BY p.task_id, p.prerequisite_id LIMIT ?1",
    )?;
    let mut rows = query.query([i64::try_from(swarm_domain::MAX_HIVE_PREREQUISITES + 1)
        .map_err(|_| swarm_domain::TaskPrerequisiteError::Capacity)?])?;
    let mut count = 0;
    let mut digest = Sha256::new();
    while let Some(row) = rows.next()? {
        count += 1;
        if count > swarm_domain::MAX_HIVE_PREREQUISITES {
            return Err(swarm_domain::TaskPrerequisiteError::Capacity.into());
        }
        for column in [0, 1, 2, 4] {
            digest.update(row.get::<_, String>(column)?.as_bytes());
            digest.update([0]);
        }
        digest.update([u8::from(row.get::<_, bool>(3)?)]);
    }
    Ok(format!("prerequisites:{:x}", digest.finalize()))
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

/// Gives up on an uncertain run nothing could settle, so automation resumes.
///
/// Uncertain is the state Swarm enters when it cannot confirm a review reached
/// Queen, and both exits from it are exact: the delivery session ended, or the
/// run marker is still on Queen's screen. Neither is guaranteed to arrive. The
/// API going down mid-run marks every run uncertain, an app reload does not end
/// Queen's terminal — the terminal host is a separate service — and the marker
/// scrolls out of the visible window within minutes of a busy session. That
/// combination is ordinary, not exotic.
///
/// What it cost: `observe_queen_automation` will not queue while a run is
/// uncertain, so one unsettleable run stopped every automatic review from then
/// on. The board kept filling and Queen was never asked to look at it again.
/// The control room showed a state, nothing raised an alarm, and the only exit
/// was an operator pressing the button — which is precisely the dependency the
/// automation exists to remove, and precisely what is absent overnight.
///
/// Abandoning rather than replaying is what makes this safe. The uncertain
/// state exists to stop a review being DOUBLED, and that risk lives in reusing
/// the run id: Queen holding the original could finish a run somebody else had
/// already re-delivered. Here the run is dropped instead, and the next
/// observation queues a new one with a new id and a fresh prompt. A Queen still
/// working the old run can still close it — `finish_queen_automation_run`
/// accepts `uncertain` — and at worst reads one duplicated review request,
/// which costs a turn. Sitting still until morning costs the night.
///
/// Age is measured from the run's own timestamps, not from `updated_at`.
/// `updated_at` is written with the database's clock while everything that
/// decides here takes an injected `now`, so a comparison against it is between
/// two different clocks and, in a test, never true.
fn abandon_unsettled_uncertain_run(
    transaction: &rusqlite::Transaction<'_>,
    now: i64,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE queen_automation
         SET state = 'idle', run_id = NULL, trigger = NULL, pending_fingerprint = NULL,
             delivery_session_id = NULL, attempts = 0, attempted_at = NULL,
             delivered_at = NULL, finished_at = NULL, outcome = NULL, updated_at = ?1
         WHERE id = 1 AND state = 'uncertain'
           AND COALESCE(delivered_at, requested_at, updated_at) + ?2 <= ?1",
        params![now, ABANDON_UNSETTLED_UNCERTAIN_SECONDS],
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
    let session: Option<String> = connection
        .query_row(
            "SELECT session.session_id FROM worker_profiles queen
         JOIN worker_sessions session ON session.worker_id = queen.id
         WHERE queen.role = 'queen' AND session.ended_at IS NULL
         ORDER BY session.started_at DESC, session.session_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(session) = session else {
        return Ok(Some("Waiting for Queen to wake".into()));
    };
    let session = WorkerSessionId::from_str(&session).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(
        crate::workers::coordination_is_cooling_down_from_connection(connection, session, now)?
            .then(|| "Pacing Queen's next review after a recent delivery".into()),
    )
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

/// Records which terminal each review was written to.
///
/// A separate forward step rather than an edit to the migration that created
/// the table. Every installed database has already passed that version, so a
/// column added there reaches new databases only — which is how this column
/// came to be queried in production before it existed.
pub(super) fn migrate_queen_delivery_session(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let has_delivery_session: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('queen_automation') WHERE name = 'delivery_session_id')",
        [],
        |row| row.get(0),
    )?;
    if !has_delivery_session {
        transaction
            .execute_batch("ALTER TABLE queen_automation ADD COLUMN delivery_session_id TEXT;")?;
    }
    transaction.pragma_update(None, "user_version", QUEEN_DELIVERY_SESSION_SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskActivityActor, TaskPriority, TaskState};

    #[test]
    fn queued_queen_review_explains_pacing_and_clears_at_the_shared_boundary() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(100).unwrap();
        assert_eq!(
            store.queen_automation_status(100).unwrap().waiting_reason,
            None
        );
        store.record_coordination_delivery(session, 100).unwrap();
        let boundary = 100 + crate::COORDINATION_DELIVERY_COOLDOWN_SECONDS;
        assert_eq!(
            store
                .queen_automation_status(boundary - 1)
                .unwrap()
                .waiting_reason
                .as_deref(),
            Some("Pacing Queen's next review after a recent delivery")
        );
        assert!(
            store
                .coordination_is_cooling_down(session, boundary - 1)
                .unwrap()
        );
        assert_eq!(
            store
                .queen_automation_status(boundary)
                .unwrap()
                .waiting_reason,
            None
        );
        assert!(
            !store
                .coordination_is_cooling_down(session, boundary)
                .unwrap()
        );
        store
            .renew_worker_engagement(session, None, 101, 60)
            .unwrap();
        assert_eq!(
            store
                .queen_automation_status(102)
                .unwrap()
                .waiting_reason
                .as_deref(),
            Some("Waiting while you are working with Queen")
        );
    }

    #[test]
    fn delivery_exception_alone_requests_queen_review_and_resolution_goes_quiet() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store.create_task("Ongoing work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.set_queen_automation_enabled(true, 100).unwrap();
        assert_eq!(
            store.queen_automation_status(100).unwrap().actionable_count,
            0
        );
        let message = store
            .send_task_message(
                task.id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(worker.id),
                "Which SHA?",
                100,
            )
            .unwrap();
        let claim = store.claim_task_messages(100).unwrap().remove(0);
        store
            .finish_task_message(&claim, crate::TaskMessageResult::Uncertain, 100)
            .unwrap();
        assert_eq!(
            store.queen_automation_status(100).unwrap().actionable_count,
            1
        );
        assert!(store.observe_queen_automation(100).unwrap());
        assert!(
            !store.observe_queen_automation(100).unwrap(),
            "one run, not a run per observation"
        );
        let delivery = store.claim_queen_automation(100).unwrap().unwrap();
        assert_eq!(delivery.actionable_count, 1);
        store
            .complete_queen_automation_delivery(&delivery.run_id, 100)
            .unwrap();
        store
            .reconcile_task_message(&message.id, &claim.claim_id, false, "Read and handled", 100)
            .unwrap();
        store
            .finish_queen_automation_run(&delivery.run_id, QueenAutomationOutcome::Completed, 100)
            .unwrap();
        assert_eq!(
            store.queen_automation_status(100).unwrap().actionable_count,
            0
        );
        assert!(!store.observe_queen_automation(100).unwrap());
        assert!(
            !store
                .observe_queen_automation(100 + RECHECK_UNCHANGED_BOARD_SECONDS)
                .unwrap()
        );
    }

    #[test]
    fn delivery_fingerprint_tracks_claim_identity_not_time_or_message_body() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store.create_task("Work", "/workspace").unwrap();
        let message = store
            .send_task_message(
                task.id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(worker.id),
                "private message content",
                100,
            )
            .unwrap();
        let claim = store.claim_task_messages(100).unwrap().remove(0);
        store
            .finish_task_message(&claim, crate::TaskMessageResult::Rejected, 100)
            .unwrap();
        let first = actionable_fingerprint(&store.connection().unwrap()).unwrap();
        assert!(!first.0.contains("private message content"));
        store
            .reconcile_task_message(&message.id, &claim.claim_id, true, "Retry explicitly", 100)
            .unwrap();
        let retry = store.claim_task_messages(100).unwrap().remove(0);
        store
            .finish_task_message(&retry, crate::TaskMessageResult::Rejected, 100)
            .unwrap();
        let next = actionable_fingerprint(&store.connection().unwrap()).unwrap();
        assert_eq!(first.1, next.1);
        assert_ne!(
            first.0, next.0,
            "same time and count, different failed attempt"
        );
    }

    #[test]
    fn prerequisite_completion_changes_review_fingerprint_without_rewriting_blocked_work() {
        let store = TaskStore::in_memory().unwrap();
        let a = store.create_task("Consumer", "/consumer").unwrap();
        let b = store.create_task("Contract", "/contract").unwrap();
        store.transition_task(a.id, TaskState::Ready).unwrap();
        store.transition_task(a.id, TaskState::Blocked).unwrap();
        store.transition_task(b.id, TaskState::Ready).unwrap();
        store.transition_task(b.id, TaskState::Active).unwrap();
        store
            .add_task_prerequisite(
                a.id,
                b.id,
                "Private contract reason",
                &swarm_domain::TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        let before = actionable_fingerprint(&store.connection().unwrap()).unwrap();
        assert_eq!(
            before,
            actionable_fingerprint(&store.connection().unwrap()).unwrap()
        );
        assert!(!before.0.contains("Private contract reason"));
        store.transition_task(b.id, TaskState::Review).unwrap();
        store.transition_task(b.id, TaskState::Completed).unwrap();
        let after = actionable_fingerprint(&store.connection().unwrap()).unwrap();
        assert_eq!(before.1, after.1);
        assert_ne!(before.0, after.0);
        assert_eq!(store.get_task(a.id).unwrap().updated_at, 10);
        assert_eq!(store.get_task(a.id).unwrap().state, TaskState::Blocked);
    }

    #[test]
    fn held_review_retries_are_quiet_but_delivery_and_recovery_publish() {
        for recover in [false, true] {
            let store = TaskStore::in_memory().unwrap();
            let queen = store.ensure_queen("/workspace/queen").unwrap();
            store
                .bind_worker_session(queen.id, WorkerSessionId::new())
                .unwrap();
            store.request_queen_automation_run(100).unwrap();
            let cursor = store.list_control_room_events(0).unwrap().next_cursor;
            for now in 101..111 {
                let delivery = store.claim_queen_automation(now).unwrap().unwrap();
                assert!(
                    store
                        .defer_queen_automation_delivery(&delivery.run_id, now)
                        .unwrap()
                );
            }
            let delivery = store.claim_queen_automation(111).unwrap().unwrap();
            assert!(
                store
                    .list_control_room_events(cursor)
                    .unwrap()
                    .events
                    .is_empty()
            );
            if recover {
                assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
                assert_eq!(store.recover_inflight_queen_automation().unwrap(), 0);
            } else {
                assert!(
                    store
                        .complete_queen_automation_delivery(&delivery.run_id, 112)
                        .unwrap()
                );
            }
            let page = store.list_control_room_events(cursor).unwrap();
            assert_eq!(page.events.len(), 1);
            assert_eq!(page.events[0].kind, ControlRoomEventKind::WorkersChanged);
        }
    }

    #[test]
    fn review_hold_tracks_its_run_not_the_queens_general_activity() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(100).unwrap();
        let first = store.claim_queen_automation(101).unwrap().unwrap();
        let subject = format!("queen-run:{}", first.run_id);
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                "queen-review",
                Some(queen.id),
                Some(session),
                "legacy",
                102,
            )
            .unwrap();
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &subject,
                Some(queen.id),
                Some(session),
                "first run",
                102,
            )
            .unwrap();
        store
            .defer_queen_automation_delivery(&first.run_id, 103)
            .unwrap();
        assert_eq!(
            store.standing_coordinator_refusals(1_000, 0).unwrap().len(),
            1
        );
        store.claim_queen_automation(104).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&first.run_id, 105)
            .unwrap();
        assert!(
            store
                .standing_coordinator_refusals(1_000, 0)
                .unwrap()
                .is_empty()
        );
        store
            .finish_queen_automation_run(&first.run_id, QueenAutomationOutcome::Completed, 106)
            .unwrap();
        store.request_queen_automation_run(107).unwrap();
        let next = store.claim_queen_automation(108).unwrap().unwrap();
        let next_subject = format!("queen-run:{}", next.run_id);
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
                &next_subject,
                Some(queen.id),
                Some(session),
                "next run",
                109,
            )
            .unwrap();
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &subject,
                Some(queen.id),
                Some(session),
                "late old",
                110,
            )
            .unwrap();
        store
            .clear_coordinator_refusal(crate::REFUSAL_DELIVERY_HELD, &subject, 111)
            .unwrap();
        let holds = store.standing_coordinator_refusals(1_000, 0).unwrap();
        assert_eq!(holds.len(), 1);
        assert_eq!(holds[0].subject, next_subject);
    }

    /// "She has been idle for the last 30 minutes when we have workers with
    /// work they could start on and reviews that need to be done."
    ///
    /// Measured: Queen ran, returned `no_action`, and was still idle nineteen
    /// minutes later with 22 tasks ready, 3 in review and workers doing
    /// nothing. The board had not changed, so the fingerprint gate never woke
    /// her — and the board only changes when somebody acts on it, which is her.
    #[test]
    fn a_verdict_on_an_unchanged_board_expires() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store
            .create_task_with_details(
                "Waiting on judgement",
                "",
                TaskPriority::Normal,
                "/workspace",
            )
            .unwrap();
        store.set_queen_automation_enabled(true, 100).unwrap();

        // She looks, and decides there is nothing to do.
        let delivery = store.claim_queen_automation(101).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, 102)
            .unwrap();
        store
            .finish_queen_automation_run(&delivery.run_id, QueenAutomationOutcome::NoAction, 110)
            .unwrap();

        // Nothing about the board changed, so she is not woken straight away.
        assert!(!store.observe_queen_automation(200).unwrap());

        // But the verdict does not stand forever.
        assert!(
            store
                .observe_queen_automation(110 + RECHECK_UNCHANGED_BOARD_SECONDS + 1)
                .unwrap(),
            "an unchanged board is worth another look once the verdict is stale"
        );
    }

    /// The gate still earns its keep: an empty board stays quiet however long
    /// it has been, so this cannot become a timer that wakes her for nothing.
    #[test]
    fn an_empty_board_never_wakes_her_however_stale_the_verdict() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.set_queen_automation_enabled(true, 100).unwrap();

        assert!(
            !store
                .observe_queen_automation(100 + RECHECK_UNCHANGED_BOARD_SECONDS * 10)
                .unwrap()
        );
    }

    /// "The queen sat idle all night." She had 22 drafts in front of her, the
    /// oldest five days old, and was woken for none of them — a draft was not
    /// actionable, so a board full of un-triaged work read as an empty board.
    ///
    /// Deciding about a draft is the review. If it does not wake her, nothing
    /// does: no one else promotes a draft, and the fingerprint gate means an
    /// unchanged board never asks twice.
    /// Queen was sent to review "4 actionable records" that were not four and
    /// were not actionable: the run was requested at 21:14 and delivered at
    /// 15:20 the next day, and the prompt carried the count from queue time.
    /// One of the four had been completed hours earlier.
    #[test]
    fn the_prompt_counts_the_board_as_it_is_at_delivery_not_as_it_was_when_queued() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let first = store
            .create_task_with_details("Triage me", "", TaskPriority::Normal, "/workspace")
            .unwrap();
        store
            .create_task_with_details("And me", "", TaskPriority::Normal, "/workspace")
            .unwrap();
        let status = store.set_queen_automation_enabled(true, 100).unwrap();
        assert_eq!(status.state, QueenAutomationState::Queued);

        // Time passes before Queen is free to take it, and one of them is dealt
        // with meanwhile.
        store
            .remove_task_as(
                first.id,
                &swarm_domain::TaskActivityActor::worker(queen.id),
                "handled already",
            )
            .unwrap();

        let delivery = store.claim_queen_automation(200).unwrap().unwrap();
        assert_eq!(
            delivery.actionable_count, 1,
            "the prompt must name the board Queen will actually find"
        );
    }

    /// The same staleness taken to its end: everything the run was queued for
    /// was handled while it waited. Waking Queen to review nothing is the "she
    /// says she is buzzing and her terminal is idle" complaint.
    #[test]
    fn a_self_triggered_run_whose_work_was_all_handled_closes_itself_rather_than_waking_her() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let task = store
            .create_task_with_details("Triage me", "", TaskPriority::Normal, "/workspace")
            .unwrap();
        assert_eq!(
            store.set_queen_automation_enabled(true, 100).unwrap().state,
            QueenAutomationState::Queued
        );
        store
            .remove_task_as(
                task.id,
                &swarm_domain::TaskActivityActor::worker(queen.id),
                "handled already",
            )
            .unwrap();

        assert!(
            store.claim_queen_automation(200).unwrap().is_none(),
            "a run with nothing left to review must not be delivered"
        );
        let status = store.queen_automation_status(201).unwrap();
        assert_eq!(
            status.state,
            QueenAutomationState::Completed,
            "and it must close itself rather than sit queued forever"
        );

        // An operator asking directly is a different thing. An empty board is
        // not a reason to refuse them: they may want her to look at something
        // Swarm does not count as actionable.
        store.request_queen_automation_run(300).unwrap();
        assert!(
            store.claim_queen_automation(301).unwrap().is_some(),
            "a manual request reaches Queen whatever the board looks like"
        );
    }

    #[test]
    fn a_draft_is_work_queen_has_not_decided_about_yet_so_it_wakes_her() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store
            .create_task_with_details("Triage me", "", TaskPriority::Normal, "/workspace")
            .unwrap();

        let status = store.set_queen_automation_enabled(true, 100).unwrap();

        assert_eq!(
            status.state,
            QueenAutomationState::Queued,
            "a draft nobody has decided about has to reach Queen"
        );
        let delivery = store.claim_queen_automation(101).unwrap().unwrap();
        assert_eq!(delivery.actionable_count, 1);
    }

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
        assert_eq!(
            store
                .finish_queen_automation_run(&delivery.run_id, QueenAutomationOutcome::NoAction, 14)
                .unwrap(),
            QueenAutomationFinish::Closed
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

    /// The third trapdoor. `recover_inflight_queen_automation` marks a run
    /// uncertain whenever the API restarts — which now happens on every app
    /// update — while Queen's session survives it, because the terminal host is
    /// a separate service. She finishes the review she was actually given and
    /// the finish used to be refused, leaving the run permanently unclosable
    /// and its stale fingerprint re-triggering it.
    #[test]
    fn a_run_whose_marker_went_uncertain_can_still_be_closed_by_queen() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(10).unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();

        // The API restarts underneath the run.
        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
        assert_eq!(
            store.queen_automation_status(12).unwrap().state,
            QueenAutomationState::Uncertain
        );

        // Queen, whose terminal never stopped, reports what she did.
        assert_eq!(
            store
                .finish_queen_automation_run(&delivery.run_id, QueenAutomationOutcome::NoAction, 13)
                .unwrap(),
            QueenAutomationFinish::Closed,
            "Queen's own report is the evidence the delivery landed"
        );
        assert_eq!(
            store.queen_automation_status(14).unwrap().state,
            QueenAutomationState::Completed
        );
    }

    /// The trapdoor: one unsettleable run stopped automation for good.
    ///
    /// Uncertain has two exits and neither is guaranteed. The session ending is
    /// exact but an app reload does not end Queen's terminal, because the
    /// terminal host is a separate service. Reading the marker off her screen is
    /// exact but it scrolls away. Meanwhile `observe_queen_automation` refuses
    /// to queue while a run is uncertain — so the ordinary case of a reload
    /// during a run left Queen never asked to look at the board again, with the
    /// board still filling and nothing raising an alarm.
    #[test]
    fn an_uncertain_run_nothing_could_settle_stops_blocking_automation() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        // Her terminal never ends, which is what a reload actually looks like.
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
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
            .create_task("Something to look at", "/workspace/petal")
            .unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        // Enabling observes the board, which already has work on it, so a run
        // is queued the ordinary way rather than by hand.
        store.set_queen_automation_enabled(true, 10).unwrap();
        store.claim_queen_automation(11).unwrap().unwrap();

        // The API restarts underneath the run, and nothing can settle it.
        assert_eq!(store.recover_inflight_queen_automation().unwrap(), 1);
        assert_eq!(
            store.queen_automation_status(12).unwrap().state,
            QueenAutomationState::Uncertain
        );
        assert!(
            !store.observe_queen_automation(13).unwrap(),
            "while it is genuinely fresh, uncertain must still hold automation back"
        );

        // Half an hour later it is abandoned rather than believed, and the
        // board — which still has work on it — gets looked at again.
        let later = 13 + ABANDON_UNSETTLED_UNCERTAIN_SECONDS;
        assert!(
            store.observe_queen_automation(later).unwrap(),
            "an unsettleable run must not block every future review"
        );
        let status = store.queen_automation_status(later).unwrap();
        assert_eq!(status.state, QueenAutomationState::Queued);
        assert_eq!(
            status.trigger,
            Some(QueenAutomationTrigger::ActionableWork),
            "the replacement is a fresh run, not a replay of the one that was lost"
        );
    }

    /// A run nobody was given cannot be closed by knowing its shape. The `run_id`
    /// is the token, and Queen only holds it because it was delivered to her.
    #[test]
    fn a_run_id_that_was_never_delivered_closes_nothing() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(10).unwrap();

        // And it says which of the reasons it was, rather than claiming no run
        // exists when one plainly does.
        let refused = store
            .finish_queen_automation_run("not-a-run", QueenAutomationOutcome::NoAction, 11)
            .unwrap();
        assert!(
            matches!(refused, QueenAutomationFinish::DifferentRun { .. }),
            "an id from an older turn must be told which run is current: {refused:?}"
        );
    }

    #[test]
    fn an_uncertain_review_on_a_live_terminal_can_be_settled_by_reading_it() {
        // Observed 2026-08-19: a review went uncertain at 22:59 and was still
        // parked ninety minutes later. The session it was written to never
        // ended, so the resume-on-ended-session rule correctly did not fire,
        // and nothing else could move it. Uncertain means Swarm could not
        // confirm the review arrived — not that it failed — and the terminal it
        // was written to can be read.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let claimed = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .fail_queen_automation_delivery(&claimed.run_id, 12, QueenAutomationFailure::Uncertain)
            .unwrap();

        // The live session is offered for checking, with the run it belongs to.
        let pending = store.uncertain_queen_delivery().unwrap();
        assert_eq!(pending, Some((claimed.run_id.clone(), session)));

        assert!(
            store
                .confirm_queen_automation_delivered(&claimed.run_id, session, 13)
                .unwrap()
        );
        let status = store.queen_automation_status(14).unwrap();
        assert_eq!(status.state, QueenAutomationState::Running);
        assert_eq!(status.delivered_at, Some(13));
    }

    #[test]
    fn settling_is_keyed_on_the_exact_run_and_session() {
        // A marker from an older run sitting in the same scrollback must not
        // resolve a newer one, and a different terminal proves nothing about
        // this delivery.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let claimed = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .fail_queen_automation_delivery(&claimed.run_id, 12, QueenAutomationFailure::Uncertain)
            .unwrap();

        assert!(
            !store
                .confirm_queen_automation_delivered("some-older-run", session, 13)
                .unwrap()
        );
        assert!(
            !store
                .confirm_queen_automation_delivered(&claimed.run_id, WorkerSessionId::new(), 13)
                .unwrap()
        );
        assert_eq!(
            store.queen_automation_status(14).unwrap().state,
            QueenAutomationState::Uncertain
        );
    }

    #[test]
    fn a_review_on_an_ended_terminal_is_not_offered_for_reading() {
        // That case resumes on its own, because a terminal that no longer
        // exists cannot be read from.
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let claimed = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .fail_queen_automation_delivery(&claimed.run_id, 12, QueenAutomationFailure::Uncertain)
            .unwrap();
        store.release_worker_session(session).unwrap();

        assert_eq!(store.uncertain_queen_delivery().unwrap(), None);
    }

    #[test]
    fn an_already_migrated_database_still_gains_the_delivery_session_column() {
        // The column was first added inside the migration that creates this
        // table, which every installed database had already passed. Fresh
        // databases gained it and installed ones did not, so it was queried in
        // production before it existed and every Queen automation read failed
        // until a forward step was added. Only a step reaches both.
        let store = TaskStore::in_memory().unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "ALTER TABLE queen_automation DROP COLUMN delivery_session_id;
                 PRAGMA user_version = 72;",
            )
            .unwrap();
        // Exercise this forward step directly: relabeling today's complete
        // schema as v72 is not a v72 fixture and replays unrelated migrations
        // against tables that already exist.
        {
            let mut connection = store.connection().unwrap();
            let transaction = connection.transaction().unwrap();
            migrate_queen_delivery_session(&transaction).unwrap();
            migrate_queen_delivery_session(&transaction).unwrap();
            let version: i64 = transaction
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, QUEEN_DELIVERY_SESSION_SCHEMA_VERSION);
            transaction.commit().unwrap();
        }

        let has_column: bool = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('queen_automation') WHERE name = 'delivery_session_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_column, "an installed database must gain the column too");
        // And the query that failed in production now works.
        assert!(store.queen_automation_status(10).is_ok());
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
        assert_eq!(
            store
                .finish_queen_automation_run(
                    &original_run_id,
                    QueenAutomationOutcome::Completed,
                    15,
                )
                .unwrap(),
            QueenAutomationFinish::Closed
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
            .record_stale_owned_work_attention(
                &candidate,
                1_000,
                600,
                crate::BackgroundWorkReading::NoneVisible,
            )
            .unwrap();

        let status = store.set_queen_automation_enabled(true, 1_001).unwrap();
        assert_eq!(status.actionable_count, 1);
        assert_eq!(status.state, QueenAutomationState::Queued);

        store
            .transition_task_with_note(task.id, TaskState::Review, "Ready")
            .unwrap();
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
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
        let dispatch = store
            .claim_task_dispatches(100, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        store
            .complete_task_dispatch(&dispatch.assignment_id, dispatch.generation, 101)
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
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
        assert!(store.current_coordinator_attention(0).unwrap().is_empty());
        assert_eq!(
            store.queen_automation_status(702).unwrap().actionable_count,
            0
        );
    }

    /// The operator kept seeing "Queen needs you", opened her, and found
    /// nothing. Her own panel said "0 worker cases needing judgment" while the
    /// card said she had filed a request and stopped. Re-running reproduced it.
    ///
    /// The claim was never checked against anything. "I need the operator" is a
    /// statement about something they can act on, so it only holds while
    /// something is actually waiting for them.
    #[test]
    fn queen_cannot_report_needing_the_operator_with_nothing_waiting() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, 12)
            .unwrap();

        assert_eq!(
            store
                .finish_queen_automation_run(
                    &delivery.run_id,
                    QueenAutomationOutcome::NeedsOperator,
                    13,
                )
                .unwrap(),
            QueenAutomationFinish::Closed
        );

        // The run finished — it really did happen — but it does not leave a
        // request behind that nobody can find.
        let status = store.queen_automation_status(14).unwrap();
        assert_eq!(status.state, QueenAutomationState::Completed);
        assert_eq!(status.outcome, Some(QueenAutomationOutcome::NoAction));
    }

    /// And when something genuinely is waiting, the claim stands.
    #[test]
    fn queen_reports_needing_the_operator_when_her_request_is_pending() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: None,
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Which repository owns this?",
                summary: "Two repositories both look like the owner of this work.",
                reason: "The change touches both.",
                risk: "",
                evidence: "",
                suggested_action: "Route to the platform repository",
                allowed_actions: &["Route to the platform repository".to_owned()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        store.request_queen_automation_run(10).unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, 12)
            .unwrap();

        store
            .finish_queen_automation_run(
                &delivery.run_id,
                QueenAutomationOutcome::NeedsOperator,
                13,
            )
            .unwrap();

        let status = store.queen_automation_status(14).unwrap();
        assert_eq!(status.outcome, Some(QueenAutomationOutcome::NeedsOperator));
    }

    /// The operator: "Public Website created a bunch of tasks and the queen
    /// never triggered to look. Doing it manually now."
    ///
    /// A worker filing work records an attention record Queen reads when she
    /// reviews — but what decides she should review at all is a separate
    /// fingerprint, and that carried its own copy of the predicate without the
    /// new kind. So the record existed, Queen could have read it, and nothing
    /// woke her. All three questions now read one definition.
    #[test]
    fn work_a_worker_files_is_enough_to_wake_queen() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, session).unwrap();
        let worker = store
            .create_worker(
                "Public Website",
                ProviderKind::ClaudeCode,
                "/workspace/public-web",
                false,
                1,
            )
            .unwrap();
        let worker_session = WorkerSessionId::new();
        store
            .bind_worker_session(worker.id, worker_session)
            .unwrap();
        store.set_queen_automation_enabled(true, 9).unwrap();
        // Nothing outstanding: Queen has no reason to run.
        assert!(!store.observe_queen_automation(10).unwrap());

        let filed = store
            .create_task_with_details_as(
                "Sanitise the module HTML",
                "",
                TaskPriority::Normal,
                "/workspace/public-web",
                &TaskActivityActor::worker(worker.id),
            )
            .unwrap();
        store
            .record_worker_filed_draft_attention(filed.id, worker.id, worker_session)
            .unwrap();

        // Now she has a reason, and the trigger sees it.
        assert!(
            store.observe_queen_automation(11).unwrap(),
            "a worker filing work should wake Queen"
        );
    }

    /// Real Truth had Ready work assigned, its only session had ended a week
    /// earlier, and its wake had been attempted once and come back uncertain.
    /// A wake is queued at assignment and never again, and every detector
    /// begins from a live session — so nothing retried and nothing noticed.
    /// The work simply sat there, and Queen was not even told there was
    /// anything to look at.
    #[test]
    fn ready_work_whose_worker_never_woke_is_something_queen_should_see() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let queen_session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, queen_session).unwrap();
        let worker = store
            .create_worker(
                "Real Truth",
                ProviderKind::ClaudeCode,
                "/workspace/real-truth",
                false,
                1,
            )
            .unwrap();
        store.set_queen_automation_enabled(true, 9).unwrap();

        // Assigned while it was running, as assignment requires.
        let worker_session = WorkerSessionId::new();
        store
            .bind_worker_session(worker.id, worker_session)
            .unwrap();
        let task = store
            .create_task("Bring the staging slot current", "/workspace/real-truth")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, worker_session).unwrap();
        // Then it went away, and its wake came back uncertain.
        store.release_worker_session(worker_session).unwrap();

        // The worker is not running, so this work is going nowhere on its own.
        assert!(
            store.observe_queen_automation(11).unwrap(),
            "stranded ready work should be something Queen reviews"
        );
    }

    /// The same work, once its worker is actually running, is that worker's to
    /// get on with rather than something for Queen to re-examine.
    #[test]
    fn ready_work_with_a_running_worker_is_not_queens_to_chase() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let worker_session = WorkerSessionId::new();
        store
            .bind_worker_session(worker.id, worker_session)
            .unwrap();
        store.set_queen_automation_enabled(true, 9).unwrap();

        let task = store
            .create_task("Carry on with this", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, worker_session).unwrap();

        assert!(!store.observe_queen_automation(11).unwrap());
    }
}
