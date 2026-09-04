//! Authored text is independent of provider consumption and decision resolution.
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    OperatorId, OperatorSubmissionId, WorkerId, WorkerSessionId, valid_operator_submission,
};

use crate::{OperatorStatementError, TaskStore, TaskStoreError};

#[derive(serde::Serialize)]
pub struct OperatorSubmissionIndexEntry {
    pub id: OperatorSubmissionId,
    pub session_id: WorkerSessionId,
    pub recorded_at: i64,
    pub text_bytes: usize,
}

/// Authorship evidence only. No Debug implementation for private message text.
#[derive(serde::Serialize)]
pub struct AuthoredOperatorSubmission {
    pub id: OperatorSubmissionId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub operator_id: OperatorId,
    pub text: String,
    pub recorded_at: i64,
}

pub(super) fn migrate(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::OPERATOR_SUBMISSIONS_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch(
        "CREATE TABLE operator_submissions (
        id TEXT PRIMARY KEY,
        worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
        session_id TEXT NOT NULL REFERENCES worker_sessions(session_id),
        operator_id TEXT NOT NULL,
        text TEXT NOT NULL CHECK(length(CAST(text AS BLOB)) BETWEEN 1 AND 65536),
        recorded_at INTEGER NOT NULL CHECK(recorded_at >= 0)
    ); CREATE INDEX operator_submissions_time ON operator_submissions(recorded_at, id);",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::OPERATOR_SUBMISSIONS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Bounded, content-free discovery for one worker in this Hive.
    ///
    /// # Errors
    /// Returns persistence or invalid-time errors; unavailable is not empty.
    pub fn operator_submission_index(
        &self,
        worker: WorkerId,
        now: i64,
    ) -> Result<(Vec<OperatorSubmissionIndexEntry>, bool), TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid submission read time".into(),
            ));
        }
        let connection = self.connection()?;
        let mut query = connection.prepare("SELECT s.id,s.session_id,s.recorded_at,length(CAST(s.text AS BLOB))
            FROM operator_submissions s JOIN worker_profiles w ON w.id=s.worker_id
            JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1
            WHERE s.worker_id=?1 AND s.recorded_at>=?2 ORDER BY s.recorded_at DESC,s.id DESC LIMIT 11")?;
        let mut entries = query
            .query_map(
                params![worker.to_string(), now.saturating_sub(90 * 86400)],
                |row| {
                    Ok(OperatorSubmissionIndexEntry {
                        id: row
                            .get::<_, String>(0)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        session_id: row
                            .get::<_, String>(1)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        recorded_at: row.get(2)?,
                        text_bytes: row.get(3)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let more = entries.len() > 10;
        entries.truncate(10);
        Ok((entries, more))
    }

    /// Reads one retained authored source by exact ID, not a semantic match.
    ///
    /// # Errors
    /// Returns storage or integrity errors instead of a false missing result.
    pub fn authored_operator_submission(
        &self,
        id: OperatorSubmissionId,
        now: i64,
    ) -> Result<Option<AuthoredOperatorSubmission>, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid submission read time".into(),
            ));
        }
        let connection = self.connection()?;
        let source = connection
            .query_row(
                "SELECT s.worker_id,s.session_id,s.operator_id,s.text,s.recorded_at
            FROM operator_submissions s JOIN worker_profiles w ON w.id=s.worker_id
            JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1
            WHERE s.id=?1 AND s.recorded_at>=?2",
                params![id.to_string(), now.saturating_sub(90 * 86400)],
                |row| {
                    Ok(AuthoredOperatorSubmission {
                        id,
                        worker_id: row
                            .get::<_, String>(0)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        session_id: row
                            .get::<_, String>(1)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        operator_id: row
                            .get::<_, String>(2)?
                            .parse()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        text: row.get(3)?,
                        recorded_at: row.get(4)?,
                    })
                },
            )
            .optional()?;
        if source
            .as_ref()
            .is_some_and(|source| !valid_operator_submission(&source.text))
        {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid authored submission".into(),
            ));
        }
        Ok(source)
    }

    /// Records a complete authored submission after explicit operator authentication.
    /// This does not deliver text, confirm consumption, or resolve any decision.
    /// Returns false on an exact retry; source content can never be overwritten.
    ///
    /// # Errors
    /// Rejects invalid text/session, conflicting identity, full storage or SQL errors.
    pub fn record_operator_submission(
        &self,
        id: OperatorSubmissionId,
        session: WorkerSessionId,
        text: &str,
        now: i64,
    ) -> Result<bool, OperatorStatementError> {
        if now < 0 || !valid_operator_submission(text) {
            return Err(OperatorStatementError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let previous: Option<bool> = tx
            .query_row(
                "SELECT session_id=?2 AND text=?3 FROM operator_submissions WHERE id=?1",
                params![id.to_string(), session.to_string(), text],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(same) = previous {
            return if same {
                Ok(false)
            } else {
                Err(OperatorStatementError::Conflict)
            };
        }
        let binding: Option<(String, String)> = tx
            .query_row(
                "SELECT w.id, h.operator_id FROM worker_sessions s
             JOIN worker_profiles w ON w.id=s.worker_id
             JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1
             JOIN hives h ON h.id=l.hive_id
             WHERE s.session_id=?1 AND s.ended_at IS NULL",
                [session.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (worker, operator) = binding.ok_or(OperatorStatementError::Invalid)?;
        // No decision references these source-only records. Future linking must
        // pin referenced evidence before adding any dependent resolution path.
        tx.execute(
            "DELETE FROM operator_submissions WHERE recorded_at < ?1",
            [now.saturating_sub(90 * 86400)],
        )?;
        let (count, bytes): (i64, i64) = tx.query_row("SELECT count(*), coalesce(sum(length(CAST(text AS BLOB))),0) FROM operator_submissions",
            [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if count >= 4096
            || bytes.saturating_add(i64::try_from(text.len()).unwrap_or(i64::MAX))
                > 16 * 1024 * 1024
        {
            return Err(OperatorStatementError::Capacity);
        }
        tx.execute("INSERT INTO operator_submissions(id,worker_id,session_id,operator_id,text,recorded_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![id.to_string(), worker, session.to_string(), operator, text, now])?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_text_is_immutable_and_independent_of_decisions_and_delivery() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let id = OperatorSubmissionId::new();
        assert!(
            store
                .record_operator_submission(id, session, " Exact\n🐝 ", 100)
                .unwrap()
        );
        assert!(
            !store
                .record_operator_submission(id, session, " Exact\n🐝 ", 101)
                .unwrap()
        );
        assert!(matches!(
            store.record_operator_submission(id, session, "Different", 102),
            Err(OperatorStatementError::Conflict)
        ));
        assert!(store.list_decision_requests().unwrap().is_empty());
        assert!(store.claim_decision_deliveries(102).unwrap().is_empty());
        assert!(
            store
                .record_operator_submission(
                    OperatorSubmissionId::new(),
                    WorkerSessionId::new(),
                    "text",
                    102
                )
                .is_err()
        );
        assert!(
            store
                .record_operator_submission(OperatorSubmissionId::new(), session, " ", 102)
                .is_err()
        );
        assert!(
            store
                .record_operator_submission(
                    OperatorSubmissionId::new(),
                    session,
                    &"x".repeat(65537),
                    102
                )
                .is_err()
        );
    }

    #[test]
    fn capacity_is_explicit_and_expired_unreferenced_sources_make_room() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .record_operator_submission(OperatorSubmissionId::new(), session, "text", 0)
            .unwrap();
        store.connection().unwrap().execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095)
            INSERT INTO operator_submissions SELECT 'fixture-'||n.x,s.worker_id,s.session_id,s.operator_id,s.text,s.recorded_at
            FROM n CROSS JOIN (SELECT * FROM operator_submissions LIMIT 1) s;").unwrap();
        let id = OperatorSubmissionId::new();
        assert!(matches!(
            store.record_operator_submission(id, session, "new", 1),
            Err(OperatorStatementError::Capacity)
        ));
        assert!(
            store
                .record_operator_submission(id, session, "new", 90 * 86400 + 1)
                .unwrap()
        );
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM operator_submissions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn discovery_is_bounded_content_free_and_exact_reads_expire() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let mut last = OperatorSubmissionId::new();
        for now in 0..12 {
            last = OperatorSubmissionId::new();
            store
                .record_operator_submission(last, session, "Private exact text", now)
                .unwrap();
        }
        let (index, more) = store.operator_submission_index(worker.id, 12).unwrap();
        assert!(more);
        assert_eq!(index.len(), 10);
        assert_eq!(index[0].id, last);
        assert!(!serde_json::to_string(&index).unwrap().contains("Private"));
        assert!(
            store
                .operator_submission_index(WorkerId::new(), 12)
                .unwrap()
                .0
                .is_empty()
        );
        let source = store
            .authored_operator_submission(last, 12)
            .unwrap()
            .unwrap();
        assert_eq!(source.text, "Private exact text");
        assert_eq!(source.session_id, session);
        assert!(
            store
                .authored_operator_submission(OperatorSubmissionId::new(), 12)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .authored_operator_submission(last, 90 * 86400 + 12)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .operator_submission_index(worker.id, 90 * 86400 + 12)
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn schema_131_upgrade_and_restart_preserve_authored_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("submissions.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE operator_submissions; PRAGMA user_version = 131;")
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            crate::CURRENT_SCHEMA_VERSION
        );
        let id = OperatorSubmissionId::new();
        store
            .record_operator_submission(id, session, " Exact ", 100)
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert!(
            !store
                .record_operator_submission(id, session, " Exact ", 101)
                .unwrap()
        );
    }
}
