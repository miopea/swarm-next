//! Content-free observations of exact review returns, never inferred productivity.
use rusqlite::{Transaction, params};

const MAX_RETAINED: i64 = 4_096;
const RETENTION_SECONDS: i64 = 30 * 86_400;

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS review_return_history (
        request_id TEXT PRIMARY KEY CHECK(length(request_id) BETWEEN 1 AND 128),
        returned_on_build TEXT CHECK(returned_on_build IS NULL OR length(returned_on_build) BETWEEN 1 AND 128),
        returned_at INTEGER NOT NULL,
        answered_at INTEGER
    ); CREATE INDEX IF NOT EXISTS review_return_history_time ON review_return_history(returned_at,request_id);")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::REVIEW_RETURN_HISTORY_SCHEMA_VERSION,
    )
}

fn prune(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM review_return_history WHERE returned_at < ?1",
        [now.saturating_sub(RETENTION_SECONDS)],
    )?;
    tx.execute("DELETE FROM review_return_history WHERE request_id IN (
        SELECT request_id FROM review_return_history ORDER BY returned_at DESC,request_id DESC LIMIT -1 OFFSET ?1)", [MAX_RETAINED])?;
    Ok(())
}

pub(super) fn record_return(
    tx: &Transaction<'_>,
    request: &str,
    build: Option<&str>,
    now: i64,
) -> rusqlite::Result<()> {
    let build = build.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte))
    });
    tx.execute("INSERT INTO review_return_history(request_id,returned_on_build,returned_at) VALUES (?1,?2,?3)", params![request,build,now])?;
    prune(tx, now)
}

/// Called only after the existing exact reply identity and active assignee checks.
pub(super) fn record_answer(tx: &Transaction<'_>, request: &str, now: i64) -> rusqlite::Result<()> {
    tx.execute("UPDATE review_return_history SET answered_at=?2 WHERE request_id=?1 AND answered_at IS NULL", params![request,now])?;
    // Expired history is not an error and never changes authoritative answer delivery.
    Ok(())
}

pub(super) fn read(
    tx: &Transaction<'_>,
    now: i64,
    limit: u32,
) -> rusqlite::Result<swarm_domain::ReviewReturnHistory> {
    prune(tx, now)?;
    let retained_count = tx.query_row("SELECT COUNT(*) FROM review_return_history", [], |row| {
        row.get(0)
    })?;
    let mut query = tx.prepare("SELECT request_id,returned_on_build,returned_at,answered_at FROM review_return_history ORDER BY returned_at DESC,request_id DESC LIMIT ?1")?;
    let records = query
        .query_map([limit.clamp(1, 100)], |row| {
            Ok(swarm_domain::ReviewReturnEvidence {
                request_id: row.get(0)?,
                returned_on_build: row.get(1)?,
                returned_at: row.get(2)?,
                answered_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(swarm_domain::ReviewReturnHistory {
        retained_count,
        max_retained: 4096,
        retention_days: 30,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStore;
    use swarm_domain::{ProviderKind, TaskState, WorkerSessionId};

    fn fixture() -> (TaskStore, swarm_domain::TaskId, swarm_domain::WorkerId) {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Fixture", ProviderKind::ClaudeCode, "/fixture", false, 1)
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store
            .create_task("Fictional verification", "/fixture")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        (store, task.id, worker.id)
    }

    #[test]
    fn distinct_returns_survive_marker_replacement_and_exact_answer_retry() {
        let (store, task, worker) = fixture();
        let first = store
            .return_review_to_worker_on_build(task, "First question", 100, Some("dev-a"))
            .unwrap();
        let second = store
            .return_review_to_worker_on_build(task, "Second question", 101, Some("dev-b"))
            .unwrap();
        assert!(
            store
                .message_queen_from_worker(task, worker, "Old answer", Some(&first.id), 102)
                .is_err()
        );
        store
            .message_queen_from_worker(task, worker, "Answer", Some(&second.id), 103)
            .unwrap();
        store
            .message_queen_from_worker(task, worker, "Answer", Some(&second.id), 104)
            .unwrap();
        assert!(
            store
                .message_queen_from_worker(task, worker, "Conflicting", Some(&second.id), 105)
                .is_err()
        );
        let history = store.queen_run_history(106, 100).unwrap().review_returns;
        assert_eq!(history.retained_count, 2);
        assert_eq!(
            history.records[0].returned_on_build.as_deref(),
            Some("dev-b")
        );
        assert_eq!(history.records[0].answered_at, Some(103));
        assert_eq!(history.records[1].answered_at, None);
        assert_eq!(history.records[1].request_id, first.id);
    }

    #[test]
    fn failed_return_and_answer_history_writes_roll_back_then_recover() {
        let (store, task, worker) = fixture();
        store.connection().unwrap().execute_batch("CREATE TRIGGER refuse_return BEFORE INSERT ON review_return_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(
            store
                .return_review_to_worker(task, "Question", 100)
                .is_err()
        );
        assert!(store.returned_review_request(task).unwrap().is_none());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_return")
            .unwrap();
        let request = store
            .return_review_to_worker(task, "Question", 101)
            .unwrap();
        store.connection().unwrap().execute_batch("CREATE TRIGGER refuse_answer BEFORE UPDATE ON review_return_history BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(
            store
                .message_queen_from_worker(task, worker, "Answer", Some(&request.id), 102)
                .is_err()
        );
        assert_eq!(
            store.returned_review_request(task).unwrap().unwrap().status,
            "awaiting_answer"
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_answer")
            .unwrap();
        store
            .message_queen_from_worker(task, worker, "Answer", Some(&request.id), 103)
            .unwrap();
        assert_eq!(
            store
                .queen_run_history(104, 100)
                .unwrap()
                .review_returns
                .records[0]
                .answered_at,
            Some(103)
        );
    }

    #[test]
    fn bounded_history_expiry_does_not_block_a_late_authoritative_answer() {
        let (store, task, worker) = fixture();
        let request = store
            .return_review_to_worker(task, "Question", 100)
            .unwrap();
        assert_eq!(
            store
                .queen_run_history(RETENTION_SECONDS + 101, 100)
                .unwrap()
                .review_returns
                .retained_count,
            0
        );
        store
            .message_queen_from_worker(
                task,
                worker,
                "Late answer",
                Some(&request.id),
                RETENTION_SECONDS + 102,
            )
            .unwrap();
        assert_eq!(
            store.returned_review_request(task).unwrap().unwrap().status,
            "answered"
        );
        let mut connection = store.connection().unwrap();
        let tx = connection.transaction().unwrap();
        for index in 0..4100 {
            tx.execute(
                "INSERT INTO review_return_history VALUES(?1,NULL,?2,NULL)",
                params![format!("fixture-{index}"), index],
            )
            .unwrap();
        }
        let history = read(&tx, 4100, u32::MAX).unwrap();
        assert_eq!(history.retained_count, 4096);
        assert_eq!(history.records.len(), 100);
        assert_eq!(history.records[0].returned_at, 4099);
    }

    #[test]
    fn upgrade_does_not_invent_historical_returns() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hive.db");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE review_return_history; PRAGMA user_version=155;")
                .unwrap();
        }
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store
                .queen_run_history(100, 100)
                .unwrap()
                .review_returns
                .retained_count,
            0
        );
    }
}
