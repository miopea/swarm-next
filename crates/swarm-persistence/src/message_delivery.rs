//! Durable ownership of task-message delivery, separate from message history.
use crate::{TaskMessageDispatch, TaskStore, TaskStoreError};
use rusqlite::params;
use swarm_domain::TaskId;

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
    pub claim_id: String,
    pub session_id: String,
    pub updated_at: i64,
    pub superseded: bool,
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
        "CREATE TABLE task_message_deliveries (
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
         CREATE INDEX task_message_deliveries_pending ON task_message_deliveries(state, message_id);
         INSERT INTO task_message_deliveries(message_id, state, session_id, updated_at)
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

impl TaskStore {
    /// Bounded metadata for Queen's reconciliation queue; message text stays in history.
    ///
    /// # Errors
    /// Returns database failures, never an empty healthy result on a failed read.
    pub fn task_message_attention(&self) -> Result<TaskMessageAttentionPage, TaskStoreError> {
        let connection = self.connection()?;
        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM task_message_deliveries WHERE state IN ('uncertain','rejected')",
            [],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(
            "SELECT d.message_id, m.task_id, t.title, d.state, d.claim_id, d.session_id,
                d.updated_at, d.superseded FROM task_message_deliveries d
             JOIN task_messages m ON m.id = d.message_id JOIN tasks t ON t.id = m.task_id
             WHERE d.state IN ('uncertain','rejected') ORDER BY d.updated_at, d.message_id LIMIT 64",
        )?;
        let items = statement
            .query_map([], |row| {
                Ok(TaskMessageAttention {
                    message_id: row.get(0)?,
                    task_id: row.get(1)?,
                    task_title: row.get(2)?,
                    state: row.get(3)?,
                    claim_id: row.get(4)?,
                    session_id: row.get(5)?,
                    updated_at: row.get(6)?,
                    superseded: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TaskMessageAttentionPage {
            items,
            total: usize::try_from(total).unwrap_or(usize::MAX),
        })
    }

    /// Claims a bounded batch before submission. No other claim can own these rows.
    ///
    /// # Errors
    /// Returns database or stored-identity failures without a partial claim.
    pub fn claim_task_messages(&self, now: i64) -> Result<Vec<ClaimedTaskMessage>, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let messages = Self::read_pending_task_message_dispatches(&tx)?;
        let mut claims = Vec::with_capacity(messages.len());
        for message in messages {
            let claim_id = uuid::Uuid::now_v7().to_string();
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

    /// Startup recovery never guesses whether an interrupted write reached a PTY.
    ///
    /// # Errors
    /// Returns database errors; no age-based retry is performed.
    pub fn recover_task_message_claims(&self, now: i64) -> Result<usize, TaskStoreError> {
        Ok(self.connection()?.execute(
            "UPDATE task_message_deliveries SET state = 'uncertain', updated_at = ?1
             WHERE state = 'dispatching'",
            [now],
        )?)
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
        let attention = store.task_message_attention().unwrap();
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
        assert_eq!(store.task_message_attention().unwrap().total, 0);
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
}
