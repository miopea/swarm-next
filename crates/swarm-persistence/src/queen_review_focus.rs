//! A bounded delivery reservation, not a review receipt or task transition.
use rusqlite::{OptionalExtension, params};
use swarm_domain::{TaskId, WorkerSessionId, next_queen_review_focus};

use crate::{TaskStore, TaskStoreError};

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_review_focus (
         singleton INTEGER PRIMARY KEY CHECK(singleton=1),
         run_id TEXT NOT NULL CHECK(length(run_id)=36),
         focus TEXT NOT NULL CHECK(length(focus)<=256),
         last_task_id TEXT CHECK(last_task_id IS NULL OR length(last_task_id)=36));",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::QUEEN_REVIEW_FOCUS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Reserve once for the current claimed delivery. Replays never advance.
    /// Candidates are application-authorized attention, not execution authority.
    ///
    /// # Errors
    /// Rejects stale claims, invalid candidates and corrupt durable reservations.
    pub fn reserve_queen_review_focus(
        &self,
        run_id: &str,
        session_id: WorkerSessionId,
        candidates: &[TaskId],
    ) -> Result<Vec<TaskId>, TaskStoreError> {
        let invalid = || TaskStoreError::Sql(rusqlite::Error::InvalidQuery);
        uuid::Uuid::parse_str(run_id).map_err(|_| invalid())?;
        // Validate even a retry's input before consulting its cached reservation.
        next_queen_review_focus(candidates, None).map_err(|_| invalid())?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_automation a
             JOIN worker_sessions s ON s.session_id=a.delivery_session_id
             JOIN worker_profiles w ON w.id=s.worker_id AND w.role='queen'
             WHERE a.id=1 AND a.run_id=?1 AND a.state='delivering'
               AND a.delivery_session_id=?2 AND s.ended_at IS NULL)",
            params![run_id, session_id.to_string()],
            |row| row.get(0),
        )?;
        if !current {
            return Err(invalid());
        }
        let saved = tx
            .query_row(
                "SELECT run_id,focus,last_task_id FROM queen_review_focus WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let mut cursor = None;
        if let Some((saved_run, payload, last)) = saved {
            if payload.len() > 256 {
                return Err(invalid());
            }
            let focus: Vec<TaskId> = serde_json::from_str(&payload).map_err(|_| invalid())?;
            if focus.len() > swarm_domain::QUEEN_REVIEW_FOCUS_SIZE {
                return Err(invalid());
            }
            next_queen_review_focus(&focus, None).map_err(|_| invalid())?;
            cursor = last
                .map(|id| id.parse::<TaskId>())
                .transpose()
                .map_err(|_| invalid())?;
            if focus.last().copied() != cursor {
                return Err(invalid());
            }
            if saved_run == run_id {
                tx.commit()?;
                return Ok(focus);
            }
        }
        let focus = next_queen_review_focus(candidates, cursor).map_err(|_| invalid())?;
        let payload = serde_json::to_string(&focus).map_err(|_| invalid())?;
        tx.execute(
            "INSERT INTO queen_review_focus(singleton,run_id,focus,last_task_id) VALUES(1,?1,?2,?3)
             ON CONFLICT(singleton) DO UPDATE SET run_id=excluded.run_id,
             focus=excluded.focus,last_task_id=excluded.last_task_id",
            params![run_id, payload, focus.last().map(ToString::to_string)],
        )?;
        tx.commit()?;
        Ok(focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_restores_missing_focus_without_changing_existing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let task = store
            .create_task("Migration fixture", "/workspace/demo")
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE queen_review_focus; PRAGMA user_version=150;")
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(store.get_task(task.id).unwrap(), task);
        let run = claim(&store);
        assert_eq!(
            store
                .reserve_queen_review_focus(&run.run_id, run.session_id, &[task.id])
                .unwrap(),
            vec![task.id]
        );
    }

    #[test]
    fn corrupt_reservation_refuses_instead_of_advancing() {
        let store = TaskStore::in_memory().unwrap();
        let run = claim(&store);
        let task = TaskId::new();
        store
            .reserve_queen_review_focus(&run.run_id, run.session_id, &[task])
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE queen_review_focus SET focus='{}' WHERE singleton=1",
                [],
            )
            .unwrap();
        let changes = store.connection().unwrap().total_changes();
        assert!(
            store
                .reserve_queen_review_focus(&run.run_id, run.session_id, &[task])
                .is_err()
        );
        assert_eq!(store.connection().unwrap().total_changes(), changes);
    }

    fn claim(store: &TaskStore) -> crate::QueenAutomationDelivery {
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        if queen.active_session_id.is_none() {
            store
                .bind_worker_session(queen.id, WorkerSessionId::new())
                .unwrap();
        }
        store.request_queen_automation_run(100).unwrap();
        store.claim_queen_automation(100).unwrap().unwrap()
    }

    #[test]
    fn durable_replay_and_stale_session_refusal_leave_cursor_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focus.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let run = claim(&store);
        let candidates = (0..8).map(|_| TaskId::new()).collect::<Vec<_>>();
        let first = store
            .reserve_queen_review_focus(&run.run_id, run.session_id, &candidates)
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        let changes = store.connection().unwrap().total_changes();
        assert_eq!(
            store
                .reserve_queen_review_focus(&run.run_id, run.session_id, &[])
                .unwrap(),
            first
        );
        assert!(
            store
                .reserve_queen_review_focus(&run.run_id, WorkerSessionId::new(), &candidates)
                .is_err()
        );
        assert!(
            store
                .reserve_queen_review_focus(&TaskId::new().to_string(), run.session_id, &candidates)
                .is_err()
        );
        assert_eq!(store.connection().unwrap().total_changes(), changes);
    }

    #[test]
    fn failed_write_rolls_back_and_next_claim_advances_once() {
        let store = TaskStore::in_memory().unwrap();
        let run = claim(&store);
        let candidates = (0..6).map(|_| TaskId::new()).collect::<Vec<_>>();
        store.connection().unwrap().execute_batch("CREATE TRIGGER fail_focus BEFORE INSERT ON queen_review_focus BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(
            store
                .reserve_queen_review_focus(&run.run_id, run.session_id, &candidates)
                .is_err()
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_focus;")
            .unwrap();
        let first = store
            .reserve_queen_review_focus(&run.run_id, run.session_id, &candidates)
            .unwrap();
        // Model a new claimed run without relying on a review timer or model verdict.
        let next = TaskId::new().to_string();
        store
            .connection()
            .unwrap()
            .execute("UPDATE queen_automation SET run_id=?1 WHERE id=1", [&next])
            .unwrap();
        assert!(
            store
                .reserve_queen_review_focus(&run.run_id, run.session_id, &candidates)
                .is_err()
        );
        let second = store
            .reserve_queen_review_focus(&next, run.session_id, &candidates)
            .unwrap();
        assert!(second.iter().all(|id| !first.contains(id)));
        assert_eq!(
            store
                .reserve_queen_review_focus(&next, run.session_id, &candidates)
                .unwrap(),
            second
        );
    }
}
