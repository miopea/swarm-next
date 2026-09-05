//! Exact request correlation for worker answers, independent of delivery.
use crate::{MessageEnd, TaskMessage, TaskStore, TaskStoreError, insert_control_room_event};
use rusqlite::{OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, TaskId, WorkerId};

/// One current review marker, including whether its reply identity is usable.
#[derive(serde::Serialize)]
pub struct ReturnedReviewRequest {
    pub request_message_id: Option<String>,
    pub request_worker_id: Option<String>,
    pub answer_message_id: Option<String>,
    pub status: &'static str,
}

/// Invalidate only an outstanding request; keep its message as historical evidence.
pub(super) fn invalidate_pending_request(
    tx: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    retained_worker: Option<WorkerId>,
) -> rusqlite::Result<()> {
    let changed = tx.execute(
        "UPDATE task_returned_reviews SET request_worker_id = NULL
         WHERE task_id = ?1 AND answered_at IS NULL AND request_message_id IS NOT NULL
           AND (?2 IS NULL OR request_worker_id != ?2)",
        params![
            task_id.to_string(),
            retained_worker.map(|id| id.to_string())
        ],
    )?;
    if changed > 0 {
        crate::message_delivery::cancel_queued_review(tx, task_id)?;
    }
    Ok(())
}

/// Fixtures that model pre-133 databases must remove these columns as well.
#[cfg(test)]
pub(super) fn remove_schema_for_test(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "DROP TABLE task_message_deliveries;
        ALTER TABLE task_returned_reviews DROP COLUMN request_message_id;
        ALTER TABLE task_returned_reviews DROP COLUMN request_worker_id;
        ALTER TABLE task_returned_reviews DROP COLUMN answer_message_id;",
    )
}

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::REVIEW_ANSWERS_SCHEMA_VERSION {
        return Ok(());
    }
    for (column, definition) in [
        ("request_message_id", "TEXT REFERENCES task_messages(id)"),
        ("request_worker_id", "TEXT REFERENCES worker_profiles(id)"),
        ("answer_message_id", "TEXT REFERENCES task_messages(id)"),
    ] {
        let present: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_returned_reviews')
             WHERE name = ?1)",
            [column],
            |row| row.get(0),
        )?;
        if !present {
            tx.execute_batch(&format!(
                "ALTER TABLE task_returned_reviews ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    tx.pragma_update(None, "user_version", crate::REVIEW_ANSWERS_SCHEMA_VERSION)
}

impl TaskStore {
    /// Reads the exact review request without asking a caller to infer it from
    /// message order, timestamps, or matching text.
    ///
    /// # Errors
    /// Returns persistence failures; no marker is distinct from a failed read.
    pub fn returned_review_request(
        &self,
        task_id: TaskId,
    ) -> Result<Option<ReturnedReviewRequest>, TaskStoreError> {
        self.connection()?
            .query_row(
                "SELECT r.request_message_id, r.request_worker_id, r.answer_message_id,
                    r.answered_at, t.state, t.assigned_worker_id
             FROM task_returned_reviews r JOIN tasks t ON t.id = r.task_id
             WHERE r.task_id = ?1 AND t.removed_at IS NULL",
                [task_id.to_string()],
                |row| {
                    let request_message_id: Option<String> = row.get(0)?;
                    let request_worker_id: Option<String> = row.get(1)?;
                    let answer_message_id = row.get(2)?;
                    let answered_at: Option<i64> = row.get(3)?;
                    let state: String = row.get(4)?;
                    let assigned: Option<String> = row.get(5)?;
                    let status = if request_message_id.is_none() {
                        "legacy_unlinked"
                    } else if request_worker_id.is_none()
                        || state != "review"
                        || request_worker_id != assigned
                    {
                        "superseded"
                    } else if answered_at.is_some() {
                        "answered"
                    } else {
                        "awaiting_answer"
                    };
                    Ok(ReturnedReviewRequest {
                        request_message_id,
                        request_worker_id,
                        answer_message_id,
                        status,
                    })
                },
            )
            .optional()
            .map_err(TaskStoreError::from)
    }

    /// Saves a scoped worker message and explicitly correlated review answer.
    ///
    /// # Errors
    /// Refuses other workers, stale/conflicting reply identities, invalid text,
    /// and persistence failures. No partial answer is committed.
    pub fn message_queen_from_worker(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        body: &str,
        reply_to: Option<&str>,
        now: i64,
    ) -> Result<TaskMessage, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let task: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT state, assigned_worker_id FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, assigned)) = task else {
            return Err(TaskStoreError::InvalidReviewReply);
        };
        if assigned.as_deref() != Some(worker_id.to_string().as_str()) {
            return Err(TaskStoreError::InvalidReviewReply);
        }
        if let Some(request_id) = reply_to {
            let marker: Option<(Option<String>, Option<i64>)> = tx
                .query_row(
                    "SELECT answer_message_id, answered_at FROM task_returned_reviews
                 WHERE task_id = ?1 AND request_message_id = ?2 AND request_worker_id = ?3",
                    params![task_id.to_string(), request_id, worker_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((answer_id, answered_at)) = marker else {
                return Err(TaskStoreError::InvalidReviewReply);
            };
            if let Some(answer_id) = answer_id {
                let sql = format!(
                    "SELECT {} FROM task_messages m WHERE m.id = ?1",
                    crate::messages::MESSAGE_COLUMNS
                );
                let saved = tx.query_row(&sql, [answer_id], crate::messages::message_from_row)?;
                if saved.body != body.trim() {
                    return Err(TaskStoreError::InvalidReviewReply);
                }
                return Ok(saved);
            }
            if state != "review" || answered_at.is_some() {
                return Err(TaskStoreError::InvalidReviewReply);
            }
        }
        if reply_to.is_some() {
            crate::message_delivery::cancel_queued_review(&tx, task_id)?;
        }
        let message = Self::insert_task_message(
            &tx,
            task_id,
            MessageEnd::worker(worker_id),
            MessageEnd::queen(),
            body,
            now,
        )?;
        if reply_to.is_some() {
            tx.execute(
                "UPDATE task_returned_reviews SET answered_at = ?2, answer_message_id = ?3 WHERE task_id = ?1",
                params![task_id.to_string(), now, message.id],
            )?;
        }
        insert_control_room_event(&tx, ControlRoomEventKind::TasksChanged)?;
        tx.commit()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{NextMoveOwner, ProviderKind, TaskState, WorkerSessionId};

    fn fixture() -> (TaskStore, TaskId, WorkerId, String) {
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
        let request = store
            .return_review_to_worker(task.id, "Which SHA?", 100)
            .unwrap();
        (store, task.id, worker.id, request.id)
    }

    #[test]
    fn only_an_explicit_current_answer_transfers_the_move_and_retries_once() {
        let (store, task, worker, request) = fixture();
        let current = store.returned_review_request(task).unwrap().unwrap();
        assert_eq!(
            current.request_message_id.as_deref(),
            Some(request.as_str())
        );
        assert_eq!(current.status, "awaiting_answer");
        let projected = store.get_task(task).unwrap();
        assert_eq!(
            projected.review_request_id.as_deref(),
            Some(request.as_str())
        );
        assert_eq!(projected.review_request.as_deref(), Some("Which SHA?"));
        store
            .message_queen_from_worker(task, worker, "Checking", None, 101)
            .unwrap();
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Worker
        );
        for id in [&request[..8], "missing"] {
            assert!(
                store
                    .message_queen_from_worker(task, worker, "abc123", Some(id), 102)
                    .is_err()
            );
        }
        assert!(
            store
                .message_queen_from_worker(task, WorkerId::new(), "abc123", Some(&request), 102)
                .is_err()
        );
        let answer = store
            .message_queen_from_worker(task, worker, "abc123", Some(&request), 103)
            .unwrap();
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
        assert!(store.get_task(task).unwrap().review_request.is_none());
        assert!(store.get_task(task).unwrap().review_request_id.is_none());
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );
        let retry = store
            .message_queen_from_worker(task, worker, "abc123", Some(&request), 104)
            .unwrap();
        assert_eq!(answer.id, retry.id);
        let answered = store.returned_review_request(task).unwrap().unwrap();
        assert_eq!(answered.status, "answered");
        assert_eq!(
            answered.answer_message_id.as_deref(),
            Some(answer.id.as_str())
        );
        assert!(
            store
                .message_queen_from_worker(task, worker, "different", Some(&request), 104)
                .is_err()
        );
        assert_eq!(store.task_messages(task).unwrap().len(), 3);
        let next = store
            .return_review_to_worker(task, "Which environment?", 105)
            .unwrap();
        assert_eq!(
            store.get_task(task).unwrap().review_request.as_deref(),
            Some("Which environment?")
        );
        assert!(
            store
                .message_queen_from_worker(task, worker, "abc123", Some(&request), 106)
                .is_err()
        );
        store
            .message_queen_from_worker(task, worker, "development", Some(&next.id), 107)
            .unwrap();
    }

    #[test]
    fn answer_event_failure_rolls_back_message_and_ownership() {
        let (store, task, worker, request) = fixture();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER reject_answer_event BEFORE INSERT ON control_room_events
             BEGIN SELECT RAISE(ABORT, 'injected event failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .message_queen_from_worker(task, worker, "abc123", Some(&request), 101)
                .is_err()
        );
        assert_eq!(store.task_messages(task).unwrap().len(), 1);
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Worker
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_answer_event")
            .unwrap();
        store
            .message_queen_from_worker(task, worker, "abc123", Some(&request), 102)
            .unwrap();
        assert_eq!(store.task_messages(task).unwrap().len(), 2);
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );
    }

    #[test]
    fn reassignment_does_not_transfer_the_old_workers_reply_authority() {
        let (store, task, worker, request) = fixture();
        let other = store
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/workspace/other",
                false,
                2,
            )
            .unwrap();
        store.assign_task_to_worker(task, other.id).unwrap();
        assert!(store.get_task(task).unwrap().review_request.is_none());
        assert!(store.get_task(task).unwrap().review_request_id.is_none());
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );
        assert_eq!(
            store.returned_review_request(task).unwrap().unwrap().status,
            "superseded"
        );
        for sender in [worker, other.id] {
            assert!(
                store
                    .message_queen_from_worker(task, sender, "abc123", Some(&request), 101)
                    .is_err()
            );
        }
        assert!(
            store
                .message_queen_from_worker(task, worker, "Old worker", None, 101)
                .is_err()
        );
        store.assign_task_to_worker(task, worker).unwrap();
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );
        assert!(
            store
                .message_queen_from_worker(task, worker, "old answer", Some(&request), 102)
                .is_err()
        );
        assert_eq!(store.task_messages(task).unwrap().len(), 1);
    }

    #[test]
    fn reopened_or_unassigned_work_cannot_resurrect_an_old_request() {
        for reopen in [false, true] {
            let (store, task, worker, request) = fixture();
            // Rebinding the same worker alone preserves their current question.
            store.assign_task_to_worker(task, worker).unwrap();
            assert_eq!(
                store.returned_review_request(task).unwrap().unwrap().status,
                "awaiting_answer"
            );
            if reopen {
                store.transition_task(task, TaskState::Active).unwrap();
                store.transition_task(task, TaskState::Review).unwrap();
            } else {
                store.unassign_task(task).unwrap();
                store.assign_task_to_worker(task, worker).unwrap();
            }
            assert_eq!(
                store.get_task(task).unwrap().next_move_owner,
                NextMoveOwner::Queen
            );
            assert_eq!(
                store.returned_review_request(task).unwrap().unwrap().status,
                "superseded"
            );
            assert!(
                store
                    .message_queen_from_worker(task, worker, "old answer", Some(&request), 101)
                    .is_err()
            );
            let next = store
                .return_review_to_worker(task, "Which SHA now?", 102)
                .unwrap();
            store
                .message_queen_from_worker(task, worker, "new answer", Some(&next.id), 103)
                .unwrap();
        }
    }

    #[test]
    fn migration_leaves_historical_requests_unlinked() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE task_returned_reviews (task_id TEXT PRIMARY KEY,
            request TEXT, returned_at INTEGER, answered_at INTEGER);
            INSERT INTO task_returned_reviews VALUES ('old', 'Which SHA?', 1, NULL);",
            )
            .unwrap();
        let tx = connection.transaction().unwrap();
        migrate(&tx, 132).unwrap();
        tx.commit().unwrap();
        let ids: (Option<String>, Option<String>, Option<String>) = connection.query_row(
            "SELECT request_message_id, request_worker_id, answer_message_id FROM task_returned_reviews",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(ids, (None, None, None));
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            133
        );
    }

    #[test]
    fn unlinked_history_does_not_make_the_current_worker_owe_an_answer() {
        let (store, task, worker, request) = fixture();
        store.connection().unwrap().execute(
            "UPDATE task_returned_reviews SET request_message_id = NULL, request_worker_id = NULL WHERE task_id = ?1",
            [task.to_string()],
        ).unwrap();
        assert_eq!(
            store.returned_review_request(task).unwrap().unwrap().status,
            "legacy_unlinked"
        );
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );
        assert!(
            store
                .message_queen_from_worker(task, worker, "abc123", Some(&request), 101)
                .is_err()
        );
        assert!(
            store
                .returned_review_request(TaskId::new())
                .unwrap()
                .is_none()
        );
    }
}
