//! Durable ownership of task-message delivery, separate from message history.
use crate::{TaskMessageDispatch, TaskStore, TaskStoreError};
use rusqlite::params;
use std::str::FromStr;

use swarm_domain::{TaskId, WorkerId};

pub const TASK_MESSAGE_BATCH_LIMIT: usize = 16;
pub const TASK_MESSAGE_QUEUE_LIMIT: usize = 4_096;

#[derive(Clone, Debug)]
pub struct ClaimedTaskMessage {
    pub message: TaskMessageDispatch,
    pub claim_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TaskMessageAttention {
    pub message_id: String,
    pub task_id: String,
    pub task_title: String,
    pub state: String,
    /// Absent until a delivery pass claims the row.
    ///
    /// ⚠️ OPTIONAL BECAUSE 'queued' IS NOW IN SCOPE. The old query returned only
    /// claimed rows, so these were always present and typed as though they
    /// always would be. The first queued row this surface saw failed to read
    /// with `InvalidColumnType` — a shape the type had quietly promised could not
    /// happen.
    pub claim_id: Option<String>,
    pub session_id: Option<String>,
    pub updated_at: i64,
    pub superseded: bool,
    /// How long this has been waiting. The number a reader actually acts on.
    pub waiting_seconds: i64,
    /// Delivery attempts so far. One row here reached 1,065 before anybody
    /// noticed, which is why the count is shown rather than inferred from age.
    pub attempts: i64,
    /// Whether the recipient is running at all.
    ///
    /// ⚠️ THE FIELD THAT CHANGES WHAT TO DO. "Waiting on Platform" and "waiting
    /// on a Platform that is asleep" need different actions, and the second was
    /// indistinguishable from the first for 141 hours.
    pub recipient_running: bool,
    /// The worker it is addressed to, by name; `None` for Queen's office.
    pub recipient_name: Option<String>,
    /// The recipient's other Active task this waits behind (ADR 0067). A held
    /// message is not a failed one: it arrives once that task leaves Active,
    /// and retrying it changes nothing.
    pub held_behind: Option<crate::HeldBehindTask>,
}

#[derive(Debug, serde::Serialize)]
pub struct TaskMessageAttentionPage {
    pub items: Vec<TaskMessageAttention>,
    pub total: usize,
}

/// Only `Deferred` establishes that it is safe to retry automatically.
#[derive(Clone, Copy, Debug)]
pub enum TaskMessageResult {
    Delivered,
    Deferred,
    Uncertain,
    Rejected,
}

impl TaskMessageResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Deferred => "queued",
            Self::Uncertain => "uncertain",
            Self::Rejected => "rejected",
        }
    }
}

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::MESSAGE_DELIVERY_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_message_deliveries (
            message_id TEXT PRIMARY KEY REFERENCES task_messages(id) ON DELETE CASCADE,
            state TEXT NOT NULL CHECK(state IN
              ('queued','dispatching','delivered','uncertain','rejected','cancelled','resolved')),
            claim_id TEXT,
            session_id TEXT,
            updated_at INTEGER NOT NULL,
            resolution_reason TEXT,
            superseded INTEGER NOT NULL DEFAULT 0 CHECK(superseded IN (0,1)),
            attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
            CHECK(state != 'dispatching' OR (claim_id IS NOT NULL AND session_id IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS task_message_deliveries_pending ON task_message_deliveries(state, message_id);
         INSERT OR IGNORE INTO task_message_deliveries(message_id, state, session_id, updated_at)
           SELECT id, CASE WHEN delivered_at IS NULL THEN 'queued' ELSE 'delivered' END,
                  delivered_session_id, COALESCE(delivered_at, created_at) FROM task_messages;",
    )?;
    tx.pragma_update(None, "user_version", crate::MESSAGE_DELIVERY_SCHEMA_VERSION)
}

/// Supersession cannot recall an in-flight write. Only queued bytes are cancelled.
pub(super) fn cancel_queued_review(
    tx: &rusqlite::Transaction<'_>,
    task: TaskId,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE task_message_deliveries SET state = CASE WHEN state = 'queued' THEN 'cancelled' ELSE state END,
             superseded = 1, resolution_reason = 'review request superseded'
         WHERE state IN ('queued','dispatching','uncertain','rejected') AND message_id = (
           SELECT request_message_id FROM task_returned_reviews WHERE task_id = ?1)",
        [task.to_string()],
    )?;
    Ok(())
}

/// Cancels queued messages to a worker that can never usefully arrive: the
/// recipient was removed, or the task they are about was completed, abandoned
/// or removed.
///
/// ⚠️ FOUR MESSAGES TO THE REMOVED "MY RCG" WORKER, ON TASKS CLOSED IN
/// SEPTEMBER, WERE STILL QUEUED ON 2026-10-18, and Queues showed them as work to
/// reconcile for six weeks. Only queued rows are cancelled; a claim already in
/// flight keeps its own outcome. The message itself stays in history. Messages
/// TO QUEEN are left alone on purpose: a worker's late report on finished work,
/// such as a regression found after review, is exactly what she must still see.
fn cancel_undeliverable_worker_messages(
    tx: &rusqlite::Transaction<'_>,
    now: i64,
) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE task_message_deliveries SET state = 'cancelled', updated_at = ?1,
             resolution_reason = CASE
                 WHEN EXISTS(SELECT 1 FROM task_messages m
                     LEFT JOIN worker_profiles w ON w.id = m.recipient_worker_id
                     WHERE m.id = task_message_deliveries.message_id
                       AND (w.id IS NULL OR w.archived_at IS NOT NULL))
                 THEN 'its recipient was removed'
                 ELSE 'its task closed before it arrived' END
         WHERE state = 'queued' AND message_id IN (
             SELECT m.id FROM task_messages m
             LEFT JOIN worker_profiles w ON w.id = m.recipient_worker_id
             LEFT JOIN tasks t ON t.id = m.task_id
             WHERE m.recipient = 'worker' AND m.delivered_at IS NULL
               AND (w.id IS NULL OR w.archived_at IS NOT NULL
                 OR t.id IS NULL OR t.removed_at IS NOT NULL
                 OR t.state IN ('completed','abandoned')))",
        [now],
    )
}

/// A message to the task's worker, about that task, that has not arrived and
/// still could (ADR 0150). The task is aliased `task`.
///
/// ⚠️ AUTOMATIC SETTLEMENT CANCELLED THESE UNREAD. Closing a task makes its
/// queued messages to the worker undeliverable, and the no-deployment sweep
/// closes nothing-built work within a minute of Review. From 2026-10-20 to
/// 2026-11-04, 5 of its 117 settlements cancelled a message Queen had queued to
/// the worker; one was her go on a pull request, 72 seconds old. Pending is the
/// set the queue limit counts. `uncertain` and `rejected` wait for Queen to
/// reconcile them, and her attention list shows both, so nothing waits unowned.
macro_rules! message_owed_to_assignee {
    () => {
        "EXISTS(SELECT 1 FROM task_messages owed
             JOIN task_message_deliveries owed_delivery ON owed_delivery.message_id = owed.id
             WHERE owed.task_id = task.id AND owed.recipient = 'worker'
               AND owed.recipient_worker_id = task.assigned_worker_id
               AND owed_delivery.state IN ('queued','dispatching','uncertain','rejected'))"
    };
}
pub(crate) use message_owed_to_assignee;

/// Fences automatic settlement against a message still on its way to the
/// task's worker, inside the completion transaction, so a message queued after
/// the sweep selected the task still holds it.
pub(super) fn ensure_no_message_owed_to_assignee(
    tx: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<(), TaskStoreError> {
    let owed: bool = tx.query_row(
        concat!(
            "SELECT EXISTS(SELECT 1 FROM tasks task WHERE task.id = ?1 AND ",
            message_owed_to_assignee!(),
            ")"
        ),
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if owed {
        return Err(TaskStoreError::MessageOwedToAssignee);
    }
    Ok(())
}

impl TaskStore {
    /// Bounded metadata for Queen's reconciliation queue; message text stays in history.
    ///
    /// # Errors
    /// Returns database failures, never an empty healthy result on a failed read.
    pub fn task_message_attention(
        &self,
        now: i64,
        waited_seconds: i64,
    ) -> Result<TaskMessageAttentionPage, TaskStoreError> {
        let connection = self.connection()?;
        // ⚠️ THIS QUERY USED TO READ ONLY 'uncertain' AND 'rejected', AND THOSE
        // TWO STATES HAVE NEVER OCCURRED HERE — zero rows out of 1,701 in this
        // database's whole history. `uncertain` is written by exactly one
        // function, crash recovery for rows left mid-write, so the surface Queen
        // was told catches stuck deliveries was scoped to a case that has not
        // happened. A message retried 1,065 times over 141 hours sat in
        // 'queued', where this could not see it, and she checked a dozen times
        // and read {items:[],total:0} every time. It was empty the way a search
        // for a colour finds no shapes.
        //
        // So a message that has WAITED TOO LONG is an exception too, whatever
        // state it is in.
        let filter = "(d.state IN ('uncertain','rejected')
             OR (d.state IN ('queued','dispatching') AND d.superseded = 0
                 AND m.delivered_at IS NULL AND m.created_at + ?2 <= ?1))";
        let total: i64 = connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM task_message_deliveries d
                 JOIN task_messages m ON m.id = d.message_id WHERE {filter}"
            ),
            params![now, waited_seconds],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(&format!(
            "SELECT d.message_id, m.task_id, t.title, d.state, d.claim_id, d.session_id,
                d.updated_at, d.superseded, ?1 - m.created_at AS waiting, d.attempts,
                EXISTS(SELECT 1 FROM worker_sessions s
                    WHERE s.ended_at IS NULL
                      AND ((m.recipient = 'worker' AND s.worker_id = m.recipient_worker_id)
                        OR (m.recipient = 'queen' AND s.worker_id IN
                            (SELECT id FROM worker_profiles WHERE role = 'queen')))),
                (SELECT name FROM worker_profiles WHERE m.recipient = 'worker'
                    AND id = m.recipient_worker_id),
                {held_id}, {held_title}
             FROM task_message_deliveries d
             JOIN task_messages m ON m.id = d.message_id JOIN tasks t ON t.id = m.task_id
             WHERE {filter} ORDER BY waiting DESC, d.message_id LIMIT 64",
            held_id = crate::messages::held_behind_active_task!("id"),
            held_title = crate::messages::held_behind_active_task!("title"),
        ))?;
        let items = statement
            .query_map(params![now, waited_seconds], |row| {
                Ok(TaskMessageAttention {
                    message_id: row.get(0)?,
                    task_id: row.get(1)?,
                    task_title: row.get(2)?,
                    state: row.get(3)?,
                    claim_id: row.get(4)?,
                    session_id: row.get(5)?,
                    updated_at: row.get(6)?,
                    superseded: row.get(7)?,
                    waiting_seconds: row.get(8)?,
                    attempts: row.get(9)?,
                    recipient_running: row.get(10)?,
                    recipient_name: row.get(11)?,
                    held_behind: match (row.get(12)?, row.get(13)?) {
                        (Some(task_id), Some(title)) => {
                            Some(crate::HeldBehindTask { task_id, title })
                        }
                        _ => None,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TaskMessageAttentionPage {
            items,
            total: usize::try_from(total).unwrap_or(usize::MAX),
        })
    }

    /// Workers that are asleep while something addressed to them waits.
    ///
    /// ⚠️ THE FIVE-DAY SILENCE THIS EXISTS TO END. Delivery requires the
    /// recipient to have a LIVE SESSION, so a message to a sleeping worker has
    /// nowhere to go and simply waits. Measured on this Hive: two messages to
    /// Platform waited 141 HOURS across 1,065 delivery attempts each and were
    /// then cancelled as superseded — nobody read them, and nothing anywhere
    /// said they were waiting on a worker that was not running. Two more to
    /// D365 Solutions waited 34 and 37 hours.
    ///
    /// ⚠️ AND IT MUST NOT BE GATED ON autostart. Every worker that produced
    /// those waits has autostart = 0. `autostart` answers "should this worker
    /// always be running"; this answers "something is addressed to it" — a
    /// different question, and gating one on the other would have left exactly
    /// the affected population unfixed.
    ///
    /// Bounded by age so an ordinary send to a running worker is never a reason
    /// to start anything: a message only counts once it has waited longer than
    /// a delivery to a live session would ever take.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn workers_owed_a_waiting_delivery(
        &self,
        now: i64,
        waited_seconds: i64,
    ) -> Result<Vec<WorkerId>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT w.id FROM worker_profiles w
             JOIN task_messages m ON m.recipient_worker_id = w.id
             JOIN task_message_deliveries d ON d.message_id = m.id
             WHERE w.archived_at IS NULL
               AND m.delivered_at IS NULL
               AND d.state IN ('queued', 'dispatching')
               AND d.superseded = 0
               AND m.created_at + ?2 <= ?1
               AND NOT EXISTS(SELECT 1 FROM worker_sessions s
                   WHERE s.worker_id = w.id AND s.ended_at IS NULL)",
        )?;
        let rows =
            statement.query_map(params![now, waited_seconds], |row| row.get::<_, String>(0))?;
        let mut owed = Vec::new();
        for row in rows {
            if let Ok(worker) = WorkerId::from_str(&row?) {
                owed.push(worker);
            }
        }
        Ok(owed)
    }

    /// Claims a bounded batch before submission. No other claim can own these rows.
    ///
    /// # Errors
    /// Returns database or stored-identity failures without a partial claim.
    pub fn claim_task_messages(&self, now: i64) -> Result<Vec<ClaimedTaskMessage>, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        cancel_undeliverable_worker_messages(&tx, now)?;
        let messages = Self::read_pending_task_message_dispatches(&tx)?;
        let mut claims = Vec::with_capacity(messages.len());
        // ONE CLAIM PER TERMINAL, because one terminal gets one write. The claim
        // then names that write, so a write whose arrival could not be confirmed
        // can be found again as a whole: every message it carried, and only
        // those (ADR 0173). Minted per message, five messages pasted together
        // were five unrelated claims, and nothing could say which of them were
        // sitting unsent in the prompt.
        //
        // ⚠️ A RESEND IS THE WRITE IT REPEATS, NOTHING ADDED. Its marker is the
        // same head message, so it finds a late copy of the original unsent in
        // the prompt and presses Enter on it rather than writing again. That
        // Enter delivers what the original carried. A newer message riding in
        // the same batch would be recorded delivered without ever being
        // written, so it waits for the next write instead.
        let mut write_for_terminal: std::collections::HashMap<
            swarm_domain::WorkerSessionId,
            (String, Option<String>),
        > = std::collections::HashMap::new();
        for message in messages {
            let (claim_id, repeats) = write_for_terminal
                .entry(message.session_id)
                .or_insert_with(|| {
                    (
                        uuid::Uuid::now_v7().to_string(),
                        message.repeats_write.clone(),
                    )
                })
                .clone();
            if repeats != message.repeats_write {
                continue;
            }
            let changed = tx.execute(
                "UPDATE task_message_deliveries SET state = 'dispatching', claim_id = ?2,
                    session_id = ?3, updated_at = ?4, attempts = attempts + 1
                 WHERE message_id = ?1 AND state = 'queued'",
                params![
                    message.message_id,
                    claim_id,
                    message.session_id.to_string(),
                    now
                ],
            )?;
            if changed == 1 {
                claims.push(ClaimedTaskMessage { message, claim_id });
            }
        }
        tx.commit()?;
        Ok(claims)
    }

    /// Rechecks durable claim ownership and task/engagement holds before submission.
    /// This is an observation, not a reservation across an asynchronous PTY write.
    ///
    /// # Errors
    /// Returns database failures without granting permission to write.
    pub fn task_message_claim_can_submit(
        &self,
        claim: &ClaimedTaskMessage,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.query_row(
            concat!(
                "SELECT EXISTS(
                SELECT 1 FROM task_message_deliveries d
                JOIN task_messages m ON m.id = d.message_id
                JOIN tasks task ON task.id = m.task_id AND task.removed_at IS NULL
                JOIN worker_sessions s ON s.session_id = d.session_id AND s.ended_at IS NULL
                JOIN worker_profiles recipient ON recipient.id = s.worker_id
                  AND recipient.archived_at IS NULL
                WHERE d.message_id = ?1 AND d.claim_id = ?2 AND d.session_id = ?3
                  AND d.state = 'dispatching' AND d.superseded = 0
                  AND ((m.recipient = 'queen' AND recipient.role = 'queen')
                    OR (m.recipient = 'worker' AND m.recipient_worker_id = recipient.id))
                  AND NOT EXISTS(SELECT 1 FROM worker_engagements e
                    WHERE e.worker_id = recipient.id AND e.expires_at > ?4)
                  AND (recipient.role = 'queen' OR ",
                crate::messages::handed_off_by_recipient!(),
                " OR NOT EXISTS(
                    SELECT 1 FROM tasks ongoing WHERE ongoing.assigned_worker_id = recipient.id
                      AND ongoing.state = 'active' AND ongoing.removed_at IS NULL
                      AND ongoing.id != m.task_id)))"
            ),
            params![
                claim.message.message_id,
                claim.claim_id,
                claim.message.session_id.to_string(),
                now
            ],
            |row| row.get(0),
        )?)
    }

    /// Completes only the exact in-flight claim. False means a stale observation.
    ///
    /// # Errors
    /// Returns persistence errors; delivery evidence and state commit together.
    pub fn finish_task_message(
        &self,
        claim: &ClaimedTaskMessage,
        result: TaskMessageResult,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE task_message_deliveries SET state = CASE
                WHEN ?4 = 'queued' AND superseded = 1 THEN 'cancelled'
                ELSE ?4 END, updated_at = ?5
             WHERE message_id = ?1 AND claim_id = ?2 AND session_id = ?3 AND state = 'dispatching'",
            params![
                claim.message.message_id,
                claim.claim_id,
                claim.message.session_id.to_string(),
                result.as_str(),
                now
            ],
        )?;
        if changed == 1 && matches!(result, TaskMessageResult::Delivered) {
            tx.execute(
                "UPDATE task_messages SET delivered_at = ?2, delivered_session_id = ?3
                 WHERE id = ?1 AND delivered_at IS NULL",
                params![
                    claim.message.message_id,
                    now,
                    claim.message.session_id.to_string()
                ],
            )?;
        }
        if changed == 1 && !matches!(result, TaskMessageResult::Deferred) {
            crate::insert_control_room_event(
                &tx,
                swarm_domain::ControlRoomEventKind::TasksChanged,
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Recovery never guesses whether an interrupted write reached a PTY.
    /// Call only at startup or while holding the exclusive delivery-owner lock,
    /// before a new pass submits anything. Never recover a live owner's claim.
    ///
    /// # Errors
    /// Returns database errors; no age-based retry is performed.
    pub fn recover_task_message_claims(&self, now: i64) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE task_message_deliveries SET state = 'uncertain', updated_at = ?1
             WHERE state = 'dispatching'",
            [now],
        )?;
        if changed > 0 {
            crate::insert_control_room_event(
                &tx,
                swarm_domain::ControlRoomEventKind::TasksChanged,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Explicit reconciliation after Queen inspects durable message content.
    /// `retry` accepts possible duplicate delivery; false closes without claiming receipt.
    ///
    /// # Errors
    /// Refuses empty/oversized reasons and returns database failures.
    pub fn reconcile_task_message(
        &self,
        message_id: &str,
        observed_claim_id: &str,
        retry: bool,
        reason: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let reason = reason.trim();
        if reason.is_empty() || reason.len() > 1_000 {
            return Err(TaskStoreError::InvalidTaskMessage { max: 1_000 });
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE task_message_deliveries SET state = ?3, resolution_reason = ?4, updated_at = ?5
             WHERE message_id = ?1 AND claim_id = ?2 AND state IN ('uncertain','rejected')
               AND (?3 != 'queued' OR superseded = 0)",
            params![
                message_id,
                observed_claim_id,
                if retry { "queued" } else { "resolved" },
                reason,
                now
            ],
        )?;
        if changed == 1 {
            crate::insert_control_room_event(
                &tx,
                swarm_domain::ControlRoomEventKind::TasksChanged,
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Writes of task messages whose arrival delivery could not confirm, on
    /// terminals that are still running (ADR 0173).
    ///
    /// One entry per write: its claim names it, and it lists every message the
    /// write carried in the order they were written, so the first is the marker
    /// the recipient's prompt shows. A write to a session that has since ended
    /// is not here. Whatever it left in that prompt ended with it, and Queen
    /// still reconciles it from her attention list. Nor is a write to a
    /// terminal the operator is engaged in, exactly as a delivery to it waits:
    /// what follows Swarm's text in that prompt may be theirs, half typed.
    ///
    /// # Errors
    /// Returns database failures, never an empty healthy result on a failed read.
    pub fn unconfirmed_task_message_writes(
        &self,
        now: i64,
    ) -> Result<Vec<UnconfirmedTaskMessageWrite>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.session_id, d.claim_id, d.message_id, d.updated_at,
                    COALESCE(d.resolution_reason = ?1, 0)
             FROM task_message_deliveries d
             JOIN task_messages m ON m.id = d.message_id
             JOIN worker_sessions s ON s.session_id = d.session_id AND s.ended_at IS NULL
             WHERE d.state = 'uncertain' AND d.claim_id IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM worker_engagements e
                 WHERE e.worker_id = s.worker_id AND e.expires_at > ?3)
             ORDER BY d.session_id, d.claim_id, m.created_at, m.id
             LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                REDELIVERED_AFTER_AN_UNCONFIRMED_WRITE,
                i64::try_from(UNCONFIRMED_WRITE_ROW_LIMIT).unwrap_or(i64::MAX),
                now
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )?;
        let mut writes: Vec<UnconfirmedTaskMessageWrite> = Vec::new();
        for row in rows {
            let (session, claim_id, message_id, given_up_at, redelivered) = row?;
            let session_id = swarm_domain::WorkerSessionId::from_str(&session)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            match writes.last_mut() {
                Some(write) if write.session_id == session_id && write.claim_id == claim_id => {
                    write.message_ids.push(message_id);
                    write.given_up_at = write.given_up_at.max(given_up_at);
                    write.was_redelivery |= redelivered;
                }
                _ => writes.push(UnconfirmedTaskMessageWrite {
                    session_id,
                    claim_id,
                    message_ids: vec![message_id],
                    given_up_at,
                    was_redelivery: redelivered,
                }),
            }
        }
        Ok(writes)
    }

    /// Settles one unconfirmed write from what its recipient's prompt showed.
    ///
    /// `Delivered` is for a write Swarm has just seen submitted. `Redeliver`
    /// queues the write's messages again, once: a message that was already a
    /// second attempt stays uncertain for Queen to reconcile, and a superseded
    /// one is never sent again. Returns how many messages changed.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn settle_unconfirmed_task_message_write(
        &self,
        write: &UnconfirmedTaskMessageWrite,
        settlement: UnconfirmedWriteSettlement,
        now: i64,
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let session = write.session_id.to_string();
        let changed = match settlement {
            UnconfirmedWriteSettlement::Delivered => {
                tx.execute(
                    "UPDATE task_messages SET delivered_at = ?3, delivered_session_id = ?1
                     WHERE delivered_at IS NULL AND id IN (
                         SELECT message_id FROM task_message_deliveries
                         WHERE session_id = ?1 AND claim_id = ?2 AND state = 'uncertain')",
                    params![session, write.claim_id, now],
                )?;
                tx.execute(
                    "UPDATE task_message_deliveries SET state = 'delivered', updated_at = ?3
                     WHERE session_id = ?1 AND claim_id = ?2 AND state = 'uncertain'",
                    params![session, write.claim_id, now],
                )?
            }
            UnconfirmedWriteSettlement::Redeliver => tx.execute(
                "UPDATE task_message_deliveries SET state = 'queued', updated_at = ?3,
                     resolution_reason = ?4
                 WHERE session_id = ?1 AND claim_id = ?2 AND state = 'uncertain'
                   AND superseded = 0 AND COALESCE(resolution_reason, '') <> ?4",
                params![
                    session,
                    write.claim_id,
                    now,
                    REDELIVERED_AFTER_AN_UNCONFIRMED_WRITE
                ],
            )?,
        };
        if changed > 0 {
            crate::insert_control_room_event(
                &tx,
                swarm_domain::ControlRoomEventKind::TasksChanged,
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
}

/// Recorded on a message Swarm sends a second time because the first write
/// could not be confirmed. It marks the message as possibly repeated for its
/// reader, and it is why a message is only ever sent again once (ADR 0173).
pub const REDELIVERED_AFTER_AN_UNCONFIRMED_WRITE: &str = "redelivered after an unconfirmed write";

/// How many unconfirmed messages one delivery pass looks at.
const UNCONFIRMED_WRITE_ROW_LIMIT: usize = 64;

/// One write of task messages whose arrival delivery could not confirm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnconfirmedTaskMessageWrite {
    pub session_id: swarm_domain::WorkerSessionId,
    pub claim_id: String,
    /// In the order they were written; the first is the write's marker.
    pub message_ids: Vec<String>,
    /// When delivery stopped waiting to see it.
    pub given_up_at: i64,
    /// Whether this write was already a second attempt.
    pub was_redelivery: bool,
}

/// What the recipient's prompt established about an unconfirmed write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnconfirmedWriteSettlement {
    /// Swarm found the write unsent in the prompt and submitted it.
    Delivered,
    /// The prompt is empty and has drawn since, so the write is not waiting
    /// there. Sending it again may repeat it, and the reader is told so.
    Redeliver,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageEnd;
    use swarm_domain::{ProviderKind, TaskState, WorkerId, WorkerSessionId};

    fn fixture() -> (TaskStore, TaskId, WorkerId) {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store.create_task("Review", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        (store, task.id, worker.id)
    }

    fn send(store: &TaskStore, task: TaskId, worker: WorkerId) -> String {
        store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "Which SHA?",
                10,
            )
            .unwrap()
            .id
    }

    /// Four messages to the removed My RCG worker, on tasks closed in
    /// September, sat queued for six weeks and showed in Queues as live work.
    #[test]
    fn a_message_that_can_never_arrive_is_cancelled_and_kept_in_history() {
        let (store, review, worker) = fixture();
        let to_worker = send(&store, review, worker);
        let to_queen = store
            .send_task_message(
                review,
                MessageEnd::worker(worker),
                MessageEnd::queen(),
                "Found a regression after review",
                10,
            )
            .unwrap()
            .id;
        let leaver = store
            .create_worker("Fern", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        let handed_on = store.create_task("Handed on", "/workspace").unwrap();
        store
            .transition_task(handed_on.id, TaskState::Ready)
            .unwrap();
        store
            .assign_task_to_worker(handed_on.id, leaver.id)
            .unwrap();
        let to_leaver = send(&store, handed_on.id, leaver.id);

        store.transition_task(review, TaskState::Completed).unwrap();
        store.assign_task_to_worker(handed_on.id, worker).unwrap();
        store.archive_worker_profile(leaver.id).unwrap();
        assert!(store.claim_task_messages(20).unwrap().is_empty());

        let delivery = |id: &str| {
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT state, resolution_reason FROM task_message_deliveries
                     WHERE message_id = ?1",
                    [id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .unwrap()
        };
        assert_eq!(
            delivery(&to_worker),
            (
                "cancelled".into(),
                Some("its task closed before it arrived".into())
            )
        );
        assert_eq!(
            delivery(&to_leaver),
            ("cancelled".into(), Some("its recipient was removed".into()))
        );
        assert_eq!(
            delivery(&to_queen).0,
            "queued",
            "a late report to Queen on finished work still reaches her"
        );
        assert_eq!(
            store.task_messages(review).unwrap().len(),
            2,
            "history stays"
        );
        let waiting: Vec<_> = store
            .task_message_attention(10_000, 0)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.message_id)
            .collect();
        assert_eq!(waiting, [to_queen]);
    }

    /// ⚠️ A HELD MESSAGE SAYS WHAT HOLDS IT, AND ONLY A HELD ONE DOES. Queen's
    /// messages to RCG Networks on its blocked Phase E and its review task
    /// waited a day behind its Active Phase A2b (ADR 0067), and nothing said
    /// why. `held_behind` names that task on exactly the messages the
    /// dispatcher will not claim, so the two cannot disagree. Since ADR 0149
    /// the review task's message is not held at all: it answers work the
    /// worker handed off.
    #[test]
    fn a_queued_message_names_the_active_task_it_waits_behind() {
        let (store, review, worker) = fixture();
        let blocked = store.create_task("Phase E", "/workspace").unwrap();
        store.transition_task(blocked.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(blocked.id, worker).unwrap();
        store
            .transition_task(blocked.id, TaskState::Active)
            .unwrap();
        store
            .transition_task(blocked.id, TaskState::Blocked)
            .unwrap();
        let current = store.create_task("Phase A2b", "/workspace").unwrap();
        store.transition_task(current.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(current.id, worker).unwrap();
        store
            .transition_task(current.id, TaskState::Active)
            .unwrap();

        let on_review = send(&store, review, worker);
        let on_blocked = send(&store, blocked.id, worker);
        let on_current = send(&store, current.id, worker);
        let holder = crate::HeldBehindTask {
            task_id: current.id.to_string(),
            title: "Phase A2b".to_owned(),
        };
        // Read BEFORE the claim: a claimed message is no longer queued, so
        // afterwards nothing would be named whatever the rule said.
        let named: Vec<_> = [review, blocked.id, current.id]
            .into_iter()
            .map(|task| store.task_messages(task).unwrap()[0].held_behind.clone())
            .collect();
        assert_eq!(named, [None, Some(holder.clone()), None]);
        let claims = store.claim_task_messages(11).unwrap();
        let mut claimed: Vec<_> = claims
            .iter()
            .map(|c| c.message.message_id.clone())
            .collect();
        claimed.sort();
        let mut unnamed = vec![on_review, on_current];
        unnamed.sort();
        assert_eq!(claimed, unnamed, "exactly the messages nothing names");
        for claim in &claims {
            store
                .finish_task_message(claim, TaskMessageResult::Delivered, 12)
                .unwrap();
        }

        let attention = store.task_message_attention(100, 0).unwrap();
        assert_eq!(attention.items.len(), 1, "{attention:?}");
        for item in &attention.items {
            assert_eq!(item.held_behind, Some(holder.clone()));
            assert_eq!(item.recipient_name.as_deref(), Some("Petal"));
        }

        store
            .transition_task(current.id, TaskState::Review)
            .unwrap();
        assert_eq!(
            store.task_messages(blocked.id).unwrap()[0].held_behind,
            None
        );
        let released: Vec<_> = store
            .claim_task_messages(13)
            .unwrap()
            .into_iter()
            .map(|c| c.message.message_id)
            .collect();
        assert_eq!(released, [on_blocked], "it goes once A2b leaves Active");
    }

    /// ⚠️ THE ANSWER TO HANDED-OFF WORK DOES NOT WAIT BEHIND OTHER WORK (ADR
    /// 0149). Queen's go for a pull request, on a task in Awaiting release,
    /// waited 3 h 39 min, 85% of it behind its worker's other Active task, and
    /// was cancelled unread when the task closed. The worker read the go in its task history,
    /// which its auto mode rightly does not take as Queen's word, and asked the
    /// operator for the step instead.
    ///
    /// A message about the worker's own task in Review or Awaiting release is
    /// claimed, and passes the recheck before submission, while other work is
    /// Active. One about its Blocked work still waits, and the operator's
    /// engagement still holds everything.
    #[test]
    fn the_answer_to_handed_off_work_does_not_wait_behind_other_active_work() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap()
            .id;
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker, session).unwrap();
        let task_through = |title: &str, states: &[TaskState]| {
            let task = store.create_task(title, "/workspace").unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store.assign_task_to_worker(task.id, worker).unwrap();
            for state in states {
                store.transition_task(task.id, *state).unwrap();
            }
            task.id
        };
        let review = task_through("In review", &[TaskState::Active, TaskState::Review]);
        let awaiting = task_through(
            "Awaiting release",
            &[
                TaskState::Active,
                TaskState::Review,
                TaskState::AwaitingRelease,
            ],
        );
        let blocked = task_through("Blocked", &[TaskState::Active, TaskState::Blocked]);
        let current = task_through("Current work", &[]);

        // The recheck before submission: claimed while nothing else was
        // Active, then other work starts before the terminal is written.
        let on_review = send(&store, review, worker);
        let on_blocked = send(&store, blocked, worker);
        let claims = store.claim_task_messages(11).unwrap();
        assert_eq!(claims.len(), 2);
        store.transition_task(current, TaskState::Active).unwrap();
        for claim in &claims {
            assert_eq!(
                store.task_message_claim_can_submit(claim, 12).unwrap(),
                claim.message.message_id == on_review,
                "only the answer to handed-off work is still submitted"
            );
            store
                .finish_task_message(claim, TaskMessageResult::Deferred, 12)
                .unwrap();
        }

        // Selection, with the other work Active throughout.
        let on_awaiting = send(&store, awaiting, worker);
        let held: Vec<_> = [review, awaiting, blocked]
            .into_iter()
            .map(|task| store.task_messages(task).unwrap()[0].held_behind.clone())
            .collect();
        let holder = crate::HeldBehindTask {
            task_id: current.to_string(),
            title: "Current work".to_owned(),
        };
        assert_eq!(held, [None, None, Some(holder)]);
        let claims = store.claim_task_messages(13).unwrap();
        let mut claimed: Vec<_> = claims
            .iter()
            .map(|c| c.message.message_id.clone())
            .collect();
        claimed.sort();
        let mut expected = vec![on_review.clone(), on_awaiting];
        expected.sort();
        assert_eq!(claimed, expected, "the Blocked task's message still waits");
        assert!(!claimed.contains(&on_blocked));

        // The operator's engagement holds even the answer to handed-off work.
        assert!(
            store
                .renew_worker_engagement(session, None, 14, 60)
                .unwrap()
        );
        for claim in &claims {
            assert!(!store.task_message_claim_can_submit(claim, 15).unwrap());
        }
    }

    /// ⚠️ A MESSAGE TO A SLEEPING WORKER HAS NOWHERE TO GO, and nothing used to
    /// say so. Two of these waited 141 hours on this Hive across 1,065 delivery
    /// attempts each, and were cancelled unread. The recipient was not busy; it
    /// was not running.
    #[test]
    fn a_sleeping_worker_with_a_waiting_message_is_owed_a_return() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        // Running: nothing to wake, whatever the message's age.
        assert!(
            store
                .workers_owed_a_waiting_delivery(10_000, 600)
                .unwrap()
                .is_empty(),
            "a running worker is reached by ordinary delivery"
        );

        let session = store
            .get_worker_profile(worker)
            .unwrap()
            .active_session_id
            .unwrap();
        store.release_worker_session(session).unwrap();

        assert_eq!(
            store.workers_owed_a_waiting_delivery(10_000, 600).unwrap(),
            vec![worker],
            "asleep with something waiting is exactly the case nothing reported"
        );
    }

    /// ⚠️ AND IT MUST NOT WAIT ON autostart. Every worker that produced a
    /// multi-day wait here has autostart = 0 — the fixture's worker does too,
    /// created with `false`. Gating the wake on that flag would have left the
    /// entire affected population unfixed while looking like a fix.
    #[test]
    fn a_worker_that_does_not_autostart_is_still_woken_for_what_is_waiting() {
        let (store, task, worker) = fixture();
        let profile = store.get_worker_profile(worker).unwrap();
        assert!(
            !profile.autostart,
            "the fixture models the affected workers"
        );
        send(&store, task, worker);
        let session = store
            .get_worker_profile(worker)
            .unwrap()
            .active_session_id
            .unwrap();
        store.release_worker_session(session).unwrap();

        assert_eq!(
            store.workers_owed_a_waiting_delivery(10_000, 600).unwrap(),
            vec![worker]
        );
    }

    /// An ordinary send must never be a reason to start anything: the message
    /// has to have waited longer than delivery to a live session ever takes.
    #[test]
    fn a_message_sent_moments_ago_wakes_nobody() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        let session = store
            .get_worker_profile(worker)
            .unwrap()
            .active_session_id
            .unwrap();
        store.release_worker_session(session).unwrap();

        assert!(
            store
                .workers_owed_a_waiting_delivery(60, 600)
                .unwrap()
                .is_empty(),
            "sent at 10, read at 60, threshold 600 — far too soon to wake anyone"
        );
    }

    /// ⚠️ THE CHECK QUEEN RAN A DOZEN TIMES AND GOT NOTHING FROM. A message
    /// waiting past the threshold is an exception whatever state it is in — it
    /// used to be invisible unless it reached 'uncertain' or 'rejected', two
    /// states this database has never once written.
    #[test]
    fn a_message_waiting_too_long_is_an_exception_even_while_queued() {
        let (store, task, worker) = fixture();
        let message = send(&store, task, worker);

        // Fresh: nothing to report, or every ordinary send becomes noise.
        assert_eq!(
            store.task_message_attention(60, 600).unwrap().total,
            0,
            "sent at 10 and read at 60 is not a stuck message"
        );

        let page = store.task_message_attention(10_000, 600).unwrap();
        assert_eq!(page.total, 1, "waited 9,990 seconds and said nothing");
        let item = &page.items[0];
        assert_eq!(item.message_id, message);
        assert_eq!(item.state, "queued");
        assert_eq!(item.waiting_seconds, 9_990);
    }

    /// ⚠️ "WAITING ON PLATFORM" AND "WAITING ON A PLATFORM THAT IS ASLEEP" NEED
    /// DIFFERENT ACTIONS. The second is what produced 141 hours of silence, and
    /// it was indistinguishable from the first.
    #[test]
    fn the_exception_says_whether_the_recipient_is_even_running() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);

        let running = store.task_message_attention(10_000, 600).unwrap();
        assert!(
            running.items[0].recipient_running,
            "a live recipient is merely busy: {:?}",
            running.items[0]
        );

        let session = store
            .get_worker_profile(worker)
            .unwrap()
            .active_session_id
            .unwrap();
        store.release_worker_session(session).unwrap();

        let asleep = store.task_message_attention(10_000, 600).unwrap();
        assert!(
            !asleep.items[0].recipient_running,
            "a stopped recipient has nowhere to receive: {:?}",
            asleep.items[0]
        );
    }

    #[test]
    fn exclusive_bounded_claims_and_exact_session_fence() {
        let (store, task, worker) = fixture();
        for _ in 0..=TASK_MESSAGE_BATCH_LIMIT {
            send(&store, task, worker);
        }
        let first = store.claim_task_messages(20).unwrap();
        assert_eq!(first.len(), TASK_MESSAGE_BATCH_LIMIT);
        assert!(
            store.claim_task_messages(21).unwrap().is_empty(),
            "one in-flight batch per terminal"
        );
        assert!(store.claim_task_messages(22).unwrap().is_empty());
        let mut wrong = first[0].clone();
        wrong.message.session_id = WorkerSessionId::new();
        assert!(
            !store
                .finish_task_message(&wrong, TaskMessageResult::Delivered, 23)
                .unwrap()
        );
        wrong = first[0].clone();
        wrong.claim_id = "another claim".into();
        assert!(
            !store
                .finish_task_message(&wrong, TaskMessageResult::Delivered, 23)
                .unwrap()
        );
        assert!(
            store
                .finish_task_message(&first[0], TaskMessageResult::Delivered, 24)
                .unwrap()
        );
        assert!(
            !store
                .finish_task_message(&first[0], TaskMessageResult::Deferred, 25)
                .unwrap()
        );
        let history = store.task_messages(task).unwrap();
        assert_eq!(history[0].delivery_state, "delivered");
        assert_eq!(
            history[0].delivered_session_id,
            Some(first[0].message.session_id)
        );
    }

    #[test]
    fn uncertain_writes_require_explicit_fenced_reconciliation() {
        let (store, task, worker) = fixture();
        let id = send(&store, task, worker);
        let claim = store.claim_task_messages(20).unwrap().remove(0);
        store
            .finish_task_message(&claim, TaskMessageResult::Uncertain, 21)
            .unwrap();
        assert!(store.claim_task_messages(22).unwrap().is_empty());
        let attention = store.task_message_attention(i64::MAX, i64::MAX).unwrap();
        assert_eq!(attention.total, 1);
        assert_eq!(attention.items[0].message_id, id);
        assert!(
            !store
                .reconcile_task_message(&id, "wrong", true, "Need delivery", 23)
                .unwrap()
        );
        assert!(
            store
                .reconcile_task_message(
                    &id,
                    &claim.claim_id,
                    true,
                    "Checked; duplicate acceptable",
                    24
                )
                .unwrap()
        );
        let retry = store.claim_task_messages(25).unwrap().remove(0);
        assert_ne!(claim.claim_id, retry.claim_id);
        assert!(
            !store
                .finish_task_message(&claim, TaskMessageResult::Delivered, 26)
                .unwrap()
        );
        store
            .finish_task_message(&retry, TaskMessageResult::Rejected, 27)
            .unwrap();
        assert!(
            store
                .reconcile_task_message(
                    &id,
                    &retry.claim_id,
                    false,
                    "Retrieved durable content; handled",
                    28
                )
                .unwrap()
        );
        assert_eq!(
            store
                .task_message_attention(i64::MAX, i64::MAX)
                .unwrap()
                .total,
            0
        );
        let history = store.task_messages(task).unwrap();
        assert_eq!(history[0].delivery_state, "resolved");
        assert!(history[0].delivered_at.is_none());
    }

    #[test]
    fn startup_recovers_claims_without_replaying_and_deferral_can_retry() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        let claim = store.claim_task_messages(20).unwrap().remove(0);
        store
            .finish_task_message(&claim, TaskMessageResult::Deferred, 21)
            .unwrap();
        let retry = store.claim_task_messages(22).unwrap().remove(0);
        assert_ne!(claim.claim_id, retry.claim_id);
        assert_eq!(store.recover_task_message_claims(23).unwrap(), 1);
        assert_eq!(store.recover_task_message_claims(24).unwrap(), 0);
        assert!(store.claim_task_messages(25).unwrap().is_empty());
        assert!(
            !store
                .finish_task_message(&retry, TaskMessageResult::Delivered, 26)
                .unwrap()
        );
        assert_eq!(
            store.task_messages(task).unwrap()[0].delivery_state,
            "uncertain"
        );
    }

    #[test]
    fn supersession_cancels_queued_review_but_preserves_ordinary_messages() {
        let (store, task, worker) = fixture();
        let old = store
            .return_review_to_worker(task, "First question", 10)
            .unwrap();
        let ordinary = send(&store, task, worker);
        let new = store
            .return_review_to_worker(task, "Replacement question", 11)
            .unwrap();
        let pending = store.pending_task_message_dispatches().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().any(|d| d.message_id == ordinary));
        assert!(pending.iter().any(|d| d.message_id == new.id));
        let history = store.task_messages(task).unwrap();
        assert_eq!(
            history
                .iter()
                .find(|m| m.id == old.id)
                .unwrap()
                .delivery_state,
            "cancelled"
        );
        store.transition_task(task, TaskState::Active).unwrap();
        store.transition_task(task, TaskState::Review).unwrap();
        assert_eq!(store.pending_task_message_dispatches().unwrap().len(), 1);
    }

    #[test]
    fn in_flight_supersession_cannot_recall_bytes_or_be_retried() {
        for result in [
            TaskMessageResult::Deferred,
            TaskMessageResult::Uncertain,
            TaskMessageResult::Delivered,
        ] {
            let (store, task, _) = fixture();
            store
                .return_review_to_worker(task, "First question", 10)
                .unwrap();
            let claim = store.claim_task_messages(20).unwrap().remove(0);
            store.transition_task(task, TaskState::Active).unwrap();
            store.finish_task_message(&claim, result, 21).unwrap();
            assert!(store.pending_task_message_dispatches().unwrap().is_empty());
            assert!(
                !store
                    .reconcile_task_message(
                        &claim.message.message_id,
                        &claim.claim_id,
                        true,
                        "Try again",
                        22
                    )
                    .unwrap()
            );
            let message = store.task_messages(task).unwrap().remove(0);
            let expected = match result {
                TaskMessageResult::Deferred => "cancelled",
                TaskMessageResult::Delivered => "delivered",
                _ => "uncertain",
            };
            assert_eq!(message.delivery_state, expected);
        }
    }

    #[test]
    fn outbox_insert_failure_rolls_back_message_and_review_change() {
        let (store, task, worker) = fixture();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_message_outbox BEFORE INSERT ON task_message_deliveries
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .send_task_message(
                    task,
                    MessageEnd::queen(),
                    MessageEnd::worker(worker),
                    "hello",
                    10
                )
                .is_err()
        );
        assert!(
            store
                .return_review_to_worker(task, "Which SHA?", 11)
                .is_err()
        );
        assert!(store.task_messages(task).unwrap().is_empty());
        assert!(store.returned_review_request(task).unwrap().is_none());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_message_outbox")
            .unwrap();
        send(&store, task, worker);
        assert_eq!(store.claim_task_messages(20).unwrap().len(), 1);
    }

    #[test]
    fn answer_from_history_cancels_the_question_before_terminal_delivery() {
        let (store, task, worker) = fixture();
        let request = store
            .return_review_to_worker(task, "Which SHA?", 10)
            .unwrap();
        store
            .message_queen_from_worker(task, worker, "abc123", Some(&request.id), 11)
            .unwrap();
        assert!(store.undelivered_task_messages(worker).unwrap().is_empty());
        let history = store.task_messages(task).unwrap();
        assert_eq!(history[0].delivery_state, "cancelled");
        assert!(history[0].delivered_at.is_none());
        assert_eq!(
            store.returned_review_request(task).unwrap().unwrap().status,
            "answered"
        );
    }

    #[test]
    fn queue_overflow_refuses_without_losing_older_work() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        store.connection().unwrap().execute_batch(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x < 4095)
             INSERT INTO task_messages(id,task_id,sender,recipient,recipient_worker_id,body,created_at)
             SELECT 'fixture-' || x, m.task_id,m.sender,m.recipient,m.recipient_worker_id,m.body,m.created_at
             FROM n CROSS JOIN (SELECT * FROM task_messages LIMIT 1) m;
             INSERT INTO task_message_deliveries(message_id,state,updated_at)
             SELECT id,'queued',10 FROM task_messages WHERE id LIKE 'fixture-%';",
        ).unwrap();
        assert!(matches!(
            store.send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "Overflow",
                11
            ),
            Err(TaskStoreError::TaskMessageQueueFull)
        ));
        assert_eq!(
            store.task_messages(task).unwrap().len(),
            TASK_MESSAGE_QUEUE_LIMIT
        );
        assert_eq!(
            store.claim_task_messages(20).unwrap().len(),
            TASK_MESSAGE_BATCH_LIMIT
        );
    }

    #[test]
    fn deferred_oldest_batch_cannot_starve_other_queued_messages() {
        let (store, task, worker) = fixture();
        for _ in 0..TASK_MESSAGE_BATCH_LIMIT {
            send(&store, task, worker);
        }
        let other = store
            .create_worker("Daisy", ProviderKind::ClaudeCode, "/other", false, 1)
            .unwrap();
        store
            .bind_worker_session(other.id, WorkerSessionId::new())
            .unwrap();
        let tail = send(&store, task, other.id);
        let claims = store.claim_task_messages(20).unwrap();
        assert!(claims.iter().any(|claim| claim.message.message_id == tail));
        for claim in claims {
            store
                .finish_task_message(&claim, TaskMessageResult::Deferred, 20)
                .unwrap();
        }
        // Same timestamp: fairness must not rely on sleeping between passes.
        let next = store.claim_task_messages(20).unwrap();
        assert!(next.iter().any(|claim| claim.message.message_id == tail));
        let worker_messages: Vec<_> = next
            .iter()
            .filter(|claim| claim.message.message_id != tail)
            .map(|claim| claim.message.message_id.clone())
            .collect();
        let mut sorted = worker_messages.clone();
        sorted.sort();
        assert_eq!(
            worker_messages, sorted,
            "a recipient's older questions remain first"
        );
    }

    #[test]
    fn migration_preserves_known_receipt_without_inventing_old_attempts() {
        let (store, task, worker) = fixture();
        let delivered = send(&store, task, worker);
        let queued = send(&store, task, worker);
        let session = WorkerSessionId::new();
        store
            .mark_task_message_delivered(&delivered, session, 11)
            .unwrap();
        let mut connection = store.connection().unwrap();
        let tx = connection.transaction().unwrap();
        tx.execute_batch("DROP TABLE task_message_deliveries; PRAGMA user_version = 133;")
            .unwrap();
        migrate(&tx, 133).unwrap();
        tx.commit().unwrap();
        drop(connection);
        let messages = store.task_messages(task).unwrap();
        assert_eq!(
            messages
                .iter()
                .find(|m| m.id == delivered)
                .unwrap()
                .delivery_state,
            "delivered"
        );
        let pending = messages.iter().find(|m| m.id == queued).unwrap();
        assert_eq!(pending.delivery_state, "queued");
        assert!(pending.delivery_claim_id.is_none());
        assert_eq!(store.pending_task_message_dispatches().unwrap().len(), 1);
    }

    /// An empty commit report, which the no-deployment sweep settles on.
    fn nothing_built(store: &TaskStore, task: TaskId) {
        store
            .record_task_commits(
                task,
                "/workspace",
                swarm_domain::CommitRepositoryState::Read,
                &[],
                11,
            )
            .unwrap();
    }

    fn settle(store: &TaskStore, now: i64) -> Vec<TaskId> {
        store
            .settle_reviewed_work_without_deployment_page(now, None)
            .unwrap()
            .closed
    }

    fn delivery_state(store: &TaskStore, task: TaskId, message: &str) -> String {
        store
            .task_messages(task)
            .unwrap()
            .into_iter()
            .find(|m| m.id == message)
            .unwrap()
            .delivery_state
    }

    /// ⚠️ QUEEN'S GO WAS CANCELLED UNREAD BY THE SETTLEMENT IT RACED (ADR
    /// 0150). Work that built nothing settled within a minute of Review, and
    /// closing it cancelled her queued message to its worker. It now waits for
    /// the message to arrive, and the next pass settles it.
    #[test]
    fn settlement_waits_for_a_message_to_the_worker_then_settles() {
        let (store, task, worker) = fixture();
        nothing_built(&store, task);
        let go = send(&store, task, worker);
        assert!(settle(&store, 12).is_empty());
        assert_eq!(delivery_state(&store, task, &go), "queued");
        assert_eq!(
            store.completion_evidence(task).unwrap(),
            crate::CompletionEvidence::None,
            "a held task is not selected, so nothing is claimed for it"
        );
        let claim = store.claim_task_messages(13).unwrap().remove(0);
        assert!(settle(&store, 14).is_empty(), "a write in flight holds it");
        store
            .finish_task_message(&claim, TaskMessageResult::Delivered, 15)
            .unwrap();
        assert_eq!(settle(&store, 16), [task]);
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Completed);
        assert_eq!(delivery_state(&store, task, &go), "delivered");
    }

    /// A write that may or may not have landed holds the task until Queen
    /// reconciles it. A retry still holds; closing it out as resolved releases
    /// the task, as delivery does.
    #[test]
    fn an_unconfirmed_message_holds_settlement_until_queen_reconciles_it() {
        let (store, task, worker) = fixture();
        nothing_built(&store, task);
        let go = send(&store, task, worker);
        let claim = store.claim_task_messages(12).unwrap().remove(0);
        assert_eq!(store.recover_task_message_claims(13).unwrap(), 1);
        assert_eq!(delivery_state(&store, task, &go), "uncertain");
        assert!(settle(&store, 14).is_empty());
        assert!(
            store
                .reconcile_task_message(&go, &claim.claim_id, true, "Retry it", 15)
                .unwrap()
        );
        let retry = store.claim_task_messages(16).unwrap().remove(0);
        store
            .finish_task_message(&retry, TaskMessageResult::Rejected, 17)
            .unwrap();
        assert!(
            settle(&store, 18).is_empty(),
            "a refused write is not final"
        );
        assert!(
            store
                .reconcile_task_message(&go, &retry.claim_id, false, "Handled directly", 19)
                .unwrap()
        );
        assert_eq!(settle(&store, 20), [task]);
    }

    /// Only a message still on its way to THIS task's worker holds it. One
    /// already cancelled, one about the worker's other task, and one to a
    /// worker who does not hold this task do not.
    #[test]
    fn only_a_pending_message_to_this_tasks_worker_holds_settlement() {
        let (store, task, worker) = fixture();
        nothing_built(&store, task);
        let other = store.create_task("Other work", "/workspace").unwrap();
        store.transition_task(other.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(other.id, worker).unwrap();
        let on_other = send(&store, other.id, worker);
        // A review request, cancelled unsent when the worker answered it.
        let request = store
            .return_review_to_worker(task, "Which SHA?", 12)
            .unwrap();
        store
            .message_queen_from_worker(task, worker, "Nine tests passed", Some(&request.id), 13)
            .unwrap();
        assert_eq!(delivery_state(&store, task, &request.id), "cancelled");
        let bystander = store
            .create_worker("Fern", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        let connection = store.connection().unwrap();
        connection
            .execute(
                "INSERT INTO task_messages (id, task_id, sender, recipient, recipient_worker_id, body, created_at)
                 VALUES ('to-bystander', ?1, 'queen', 'worker', ?2, 'Fictional aside', 13)",
                params![task.to_string(), bystander.id.to_string()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_message_deliveries (message_id, state, updated_at)
                 VALUES ('to-bystander', 'queued', 13)",
                [],
            )
            .unwrap();
        drop(connection);

        assert_eq!(settle(&store, 14), [task]);
        assert_eq!(delivery_state(&store, other.id, &on_other), "queued");
    }

    /// The sweep selects first and completes later. A message queued in
    /// between is caught by the completion transaction, which leaves the task
    /// in Review and the message queued; the next pass settles once it lands.
    #[test]
    fn a_message_queued_after_selection_still_holds_the_completion() {
        let (store, task, worker) = fixture();
        nothing_built(&store, task);
        // A selected candidate whose exemption committed before the message.
        store
            .claim_completion_exemption(task, "Nothing built", None, 12)
            .unwrap();
        store
            .approve_completion_exemption(task, "coordinator", "Empty commit report", 12)
            .unwrap();
        let go = send(&store, task, worker);
        assert!(matches!(
            store.transition_task_guarded(
                task,
                TaskState::Completed,
                "Automatic close",
                None,
                &crate::TaskActivityActor::system(),
                true
            ),
            Err(TaskStoreError::MessageOwedToAssignee)
        ));
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
        assert_eq!(delivery_state(&store, task, &go), "queued");
        let claim = store.claim_task_messages(13).unwrap().remove(0);
        store
            .finish_task_message(&claim, TaskMessageResult::Delivered, 14)
            .unwrap();
        assert_eq!(settle(&store, 15), [task]);
    }

    /// The hold is the coordinator's alone. Queen or the operator closing the
    /// task on purpose is not held, and the message is cancelled as before.
    #[test]
    fn an_explicit_completion_is_not_held_by_a_pending_message() {
        let (store, task, worker) = fixture();
        nothing_built(&store, task);
        let go = send(&store, task, worker);
        store.transition_task(task, TaskState::Completed).unwrap();
        assert!(store.claim_task_messages(12).unwrap().is_empty());
        assert_eq!(delivery_state(&store, task, &go), "cancelled");
    }

    fn give_up_on(store: &TaskStore, claims: &[ClaimedTaskMessage], now: i64) {
        for claim in claims {
            assert!(
                store
                    .finish_task_message(claim, TaskMessageResult::Uncertain, now)
                    .unwrap()
            );
        }
    }

    /// One terminal gets one write, so the messages it carried share one claim
    /// and can be found again together. Another terminal's write is its own.
    #[test]
    fn the_messages_one_write_carries_share_one_claim() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        send(&store, task, worker);
        let other = store
            .create_worker("Fern", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        store
            .bind_worker_session(other.id, WorkerSessionId::new())
            .unwrap();
        let theirs = store.create_task("Theirs", "/workspace").unwrap();
        store.transition_task(theirs.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(theirs.id, other.id).unwrap();
        send(&store, theirs.id, other.id);

        let claims = store.claim_task_messages(20).unwrap();
        assert_eq!(claims.len(), 3);
        let (ours, others): (Vec<_>, Vec<_>) = claims
            .iter()
            .partition(|claim| claim.message.recipient_worker_id == worker);
        assert_eq!(ours.len(), 2);
        assert_eq!(ours[0].claim_id, ours[1].claim_id);
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].message.recipient_worker_id, other.id);
        assert_ne!(others[0].claim_id, ours[0].claim_id);
    }

    /// A write delivery could not confirm is found whole, in the order it was
    /// written, while its terminal runs. A terminal that ended took the prompt
    /// with it, so its write is left to Queen's reconciliation.
    #[test]
    fn an_unconfirmed_write_is_found_whole_while_its_terminal_runs() {
        let (store, task, worker) = fixture();
        let first = send(&store, task, worker);
        let second = send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        assert_eq!(claims[0].message.repeats_write, None);
        give_up_on(&store, &claims, 21);

        let writes = store.unconfirmed_task_message_writes(40).unwrap();
        assert_eq!(
            writes,
            [UnconfirmedTaskMessageWrite {
                session_id: claims[0].message.session_id,
                claim_id: claims[0].claim_id.clone(),
                message_ids: vec![first, second],
                given_up_at: 21,
                was_redelivery: false,
            }]
        );

        store
            .renew_worker_engagement(claims[0].message.session_id, None, 30, 60)
            .unwrap();
        assert!(
            store
                .unconfirmed_task_message_writes(40)
                .unwrap()
                .is_empty(),
            "the operator is engaged in that terminal; its prompt may hold their typing"
        );
        assert_eq!(store.unconfirmed_task_message_writes(91).unwrap().len(), 1);

        assert!(
            store
                .release_worker_session_because(claims[0].message.session_id, None)
                .unwrap()
        );
        assert!(
            store
                .unconfirmed_task_message_writes(91)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .task_message_attention(i64::MAX, i64::MAX)
                .unwrap()
                .total,
            2,
            "Queen still sees what the ended terminal may have lost"
        );
    }

    /// Swarm found its own write unsent in the prompt and sent it.
    #[test]
    fn a_write_seen_submitted_is_delivered() {
        let (store, task, worker) = fixture();
        let first = send(&store, task, worker);
        let second = send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        give_up_on(&store, &claims, 21);
        let write = store.unconfirmed_task_message_writes(40).unwrap().remove(0);

        assert_eq!(
            store
                .settle_unconfirmed_task_message_write(
                    &write,
                    UnconfirmedWriteSettlement::Delivered,
                    30
                )
                .unwrap(),
            2
        );
        for message in [&first, &second] {
            let row = store
                .task_messages(task)
                .unwrap()
                .into_iter()
                .find(|m| &m.id == message)
                .unwrap();
            assert_eq!(row.delivery_state, "delivered");
            assert_eq!(row.delivered_at, Some(30));
            assert_eq!(row.delivered_session_id, Some(write.session_id));
        }
        assert!(
            store
                .unconfirmed_task_message_writes(40)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .task_message_attention(i64::MAX, i64::MAX)
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            store
                .settle_unconfirmed_task_message_write(
                    &write,
                    UnconfirmedWriteSettlement::Delivered,
                    31
                )
                .unwrap(),
            0,
            "settling the same write twice changes nothing"
        );
    }

    /// A write that is not waiting in the prompt is sent once more, marked so
    /// its reader knows it may be a repeat. A second unconfirmed attempt stays
    /// with Queen: nothing is sent a third time on Swarm's own judgement.
    #[test]
    fn a_lost_write_is_sent_again_once_and_says_so() {
        let (store, task, worker) = fixture();
        let id = send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        give_up_on(&store, &claims, 21);
        let write = store.unconfirmed_task_message_writes(40).unwrap().remove(0);
        assert_eq!(
            store
                .settle_unconfirmed_task_message_write(
                    &write,
                    UnconfirmedWriteSettlement::Redeliver,
                    30
                )
                .unwrap(),
            1
        );
        assert_eq!(delivery_state(&store, task, &id), "queued");

        let again = store.claim_task_messages(31).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].message.repeats_write, Some(write.claim_id.clone()));
        assert_ne!(again[0].claim_id, write.claim_id);
        give_up_on(&store, &again, 32);

        let second = store.unconfirmed_task_message_writes(40).unwrap().remove(0);
        assert!(second.was_redelivery);
        assert_eq!(
            store
                .settle_unconfirmed_task_message_write(
                    &second,
                    UnconfirmedWriteSettlement::Redeliver,
                    33
                )
                .unwrap(),
            0
        );
        assert_eq!(delivery_state(&store, task, &id), "uncertain");
        let attention = store.task_message_attention(i64::MAX, i64::MAX).unwrap();
        assert_eq!(attention.total, 1);
        assert_eq!(attention.items[0].message_id, id);
    }

    /// A deferral between the two attempts keeps the mark: the reader is told
    /// whenever the message finally arrives.
    #[test]
    fn a_deferred_resend_still_says_it_may_be_a_repeat() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        give_up_on(&store, &claims, 21);
        let write = store.unconfirmed_task_message_writes(40).unwrap().remove(0);
        store
            .settle_unconfirmed_task_message_write(
                &write,
                UnconfirmedWriteSettlement::Redeliver,
                30,
            )
            .unwrap();
        let deferred = store.claim_task_messages(31).unwrap().remove(0);
        store
            .finish_task_message(&deferred, TaskMessageResult::Deferred, 32)
            .unwrap();
        assert_eq!(
            store.claim_task_messages(33).unwrap()[0]
                .message
                .repeats_write,
            Some(deferred.claim_id),
            "still the same write, still said to be a repeat"
        );
    }

    /// A superseded request is never written again, even once.
    #[test]
    fn a_superseded_message_is_not_sent_again() {
        let (store, task, worker) = fixture();
        let kept = send(&store, task, worker);
        let dropped = send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        give_up_on(&store, &claims, 21);
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_message_deliveries SET superseded = 1 WHERE message_id = ?1",
                [&dropped],
            )
            .unwrap();
        let write = store.unconfirmed_task_message_writes(40).unwrap().remove(0);
        assert_eq!(
            store
                .settle_unconfirmed_task_message_write(
                    &write,
                    UnconfirmedWriteSettlement::Redeliver,
                    30
                )
                .unwrap(),
            1
        );
        assert_eq!(delivery_state(&store, task, &kept), "queued");
        assert_eq!(delivery_state(&store, task, &dropped), "uncertain");
    }

    /// A held message raises one card for its terminal, and the card goes
    /// once nothing waits there, whichever way the message left.
    #[test]
    fn a_held_message_card_lasts_while_a_message_waits() {
        let (store, task, worker) = fixture();
        send(&store, task, worker);
        let claim = store.claim_task_messages(20).unwrap().remove(0);
        store
            .finish_task_message(&claim, TaskMessageResult::Deferred, 21)
            .unwrap();
        let subject = format!("message-delivery:{}", claim.message.session_id);
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
                &subject,
                Some(worker),
                Some(claim.message.session_id),
                "A message is waiting",
                22,
            )
            .unwrap();
        let standing = |store: &TaskStore| {
            store
                .standing_coordinator_refusals(23, 0)
                .unwrap()
                .into_iter()
                .filter(|refusal| refusal.subject == subject)
                .count()
        };
        assert_eq!(standing(&store), 1);
        let again = store.claim_task_messages(24).unwrap().remove(0);
        assert_eq!(standing(&store), 1, "still waiting while it is written");
        store
            .finish_task_message(&again, TaskMessageResult::Delivered, 25)
            .unwrap();
        assert_eq!(standing(&store), 0);
    }

    /// A resend carries what the write it repeats carried, nothing added. Its
    /// marker can find a late copy of the original unsent in the prompt, and
    /// the Enter that sends that copy delivers exactly what the copy holds.
    #[test]
    fn a_resend_is_written_alone_as_the_write_it_repeats() {
        let (store, task, worker) = fixture();
        let first = send(&store, task, worker);
        let second = send(&store, task, worker);
        let claims = store.claim_task_messages(20).unwrap();
        give_up_on(&store, &claims, 21);
        let write = store.unconfirmed_task_message_writes(40).unwrap().remove(0);
        store
            .settle_unconfirmed_task_message_write(
                &write,
                UnconfirmedWriteSettlement::Redeliver,
                30,
            )
            .unwrap();
        let newer = store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "And the tag?",
                31,
            )
            .unwrap()
            .id;

        let resend = store.claim_task_messages(32).unwrap();
        assert_eq!(
            resend
                .iter()
                .map(|claim| claim.message.message_id.clone())
                .collect::<Vec<_>>(),
            [first, second],
            "the original's messages in the original's order, so its head marker"
        );
        for claim in &resend {
            store
                .finish_task_message(claim, TaskMessageResult::Delivered, 33)
                .unwrap();
        }
        assert_eq!(delivery_state(&store, task, &newer), "queued");
        let next = store.claim_task_messages(34).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].message.message_id, newer);
        assert_eq!(next[0].message.repeats_write, None);
    }
}
