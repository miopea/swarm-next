//! Durable private native sources, separate from confirmed decision receipts.
use crate::{TaskStore, TaskStoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    MAX_NATIVE_INTERVIEW_SOURCE_BYTES, NativeInterviewEvidence, OperatorId, OperatorSubmissionId,
    WorkerId,
};

const MAX_RECORDS: i64 = 4096;
const MAX_BYTES: i64 = 16 * 1024 * 1024;
const RETENTION: i64 = 90 * 86400;

#[derive(Debug, thiserror::Error)]
pub enum NativeInterviewStoreError {
    #[error("native interview source is not valid for a local worker session")]
    Invalid,
    #[error("native interview identity already carries different evidence")]
    Conflict,
    #[error("native interview storage is full; referenced evidence was preserved")]
    Capacity,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

/// Captured source is not a confirmed operator statement or new permission.
/// This private read is not exposed through diagnostics or agent-write routes.
#[derive(serde::Serialize)]
pub struct StoredNativeInterview {
    pub source: NativeInterviewEvidence,
    pub worker_id: WorkerId,
    pub operator_id: OperatorId,
    pub recorded_at: i64,
}

struct NativeSourceRow {
    payload: String,
    worker: String,
    operator: String,
    recorded_at: i64,
    session: String,
    conversation: String,
    invocation: String,
}

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS native_operator_interviews (
        id TEXT PRIMARY KEY,
        worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
        session_id TEXT NOT NULL REFERENCES worker_sessions(session_id),
        operator_id TEXT NOT NULL,
        conversation TEXT NOT NULL,
        invocation_id TEXT NOT NULL,
        payload TEXT NOT NULL CHECK(length(CAST(payload AS BLOB)) BETWEEN 1 AND 131072),
        recorded_at INTEGER NOT NULL CHECK(recorded_at >= 0),
        decision_id TEXT REFERENCES decision_requests(id),
        UNIQUE(session_id, conversation, invocation_id)
    ); CREATE INDEX IF NOT EXISTS native_operator_interviews_time ON native_operator_interviews(recorded_at, id);")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::NATIVE_OPERATOR_INTERVIEWS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Admit only after the application authenticates the independent engine.
    /// An ended known session may still have retained engine evidence awaiting
    /// API ingestion. Admission does not settle a request or inject an answer.
    /// Exact-ID retries survive API interruption; conflicting invocation IDs fail.
    /// # Errors
    /// Refuses invalid, conflicting, foreign, oversized or unavailable evidence.
    pub fn record_native_interview(
        &self,
        source: &NativeInterviewEvidence,
        now: i64,
    ) -> Result<bool, NativeInterviewStoreError> {
        if now < 0 || !source.is_valid() {
            return Err(NativeInterviewStoreError::Invalid);
        }
        let payload =
            serde_json::to_string(source).map_err(|_| NativeInterviewStoreError::Invalid)?;
        if payload.len() > MAX_NATIVE_INTERVIEW_SOURCE_BYTES {
            return Err(NativeInterviewStoreError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let previous: Option<bool> = tx
            .query_row(
                "SELECT payload=?2 FROM native_operator_interviews WHERE id=?1",
                params![source.id.to_string(), payload],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(same) = previous {
            return if same {
                Ok(false)
            } else {
                Err(NativeInterviewStoreError::Conflict)
            };
        }
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM native_operator_interviews
            WHERE session_id=?1 AND conversation=?2 AND invocation_id=?3)",
            params![
                source.session_id.to_string(),
                source.conversation.to_string(),
                source.tool_use_id
            ],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(NativeInterviewStoreError::Conflict);
        }
        let binding: Option<(String, String)> = tx.query_row(
            "SELECT w.id,h.operator_id FROM worker_sessions s JOIN worker_profiles w ON w.id=s.worker_id
             JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1
             JOIN hives h ON h.id=l.hive_id WHERE s.session_id=?1",
            [source.session_id.to_string()], |row| Ok((row.get(0)?,row.get(1)?))).optional()?;
        let (worker, operator) = binding.ok_or(NativeInterviewStoreError::Invalid)?;
        // A future exact decision binding pins pending evidence. This source-only
        // admission cannot create that link or claim the decision was answered.
        tx.execute("DELETE FROM native_operator_interviews WHERE recorded_at < ?1 AND
            (decision_id IS NULL OR decision_id IN (SELECT id FROM decision_requests WHERE state != 'pending'))",
            [now.saturating_sub(RETENTION)])?;
        let (count, bytes): (i64, i64) = tx.query_row(
            "SELECT count(*),coalesce(sum(length(CAST(payload AS BLOB))),0) FROM native_operator_interviews",
            [], |row| Ok((row.get(0)?,row.get(1)?)))?;
        if count >= MAX_RECORDS
            || bytes.saturating_add(i64::try_from(payload.len()).unwrap_or(i64::MAX)) > MAX_BYTES
        {
            return Err(NativeInterviewStoreError::Capacity);
        }
        tx.execute("INSERT INTO native_operator_interviews(id,worker_id,session_id,operator_id,conversation,invocation_id,payload,recorded_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![source.id.to_string(),worker,source.session_id.to_string(),operator,
            source.conversation.to_string(),source.tool_use_id,payload,now])?;
        tx.commit()?;
        Ok(true)
    }

    /// Read one exact private source. Retention pruning occurs on admission;
    /// pending decision references never expire just because the API restarts.
    /// # Errors
    /// Corruption or unavailable storage is not reported as missing evidence.
    pub fn native_interview(
        &self,
        id: OperatorSubmissionId,
    ) -> Result<Option<StoredNativeInterview>, TaskStoreError> {
        let connection = self.connection()?;
        let row: Option<NativeSourceRow> = connection.query_row(
            "SELECT n.payload,n.worker_id,n.operator_id,n.recorded_at,n.session_id,n.conversation,n.invocation_id
             FROM native_operator_interviews n JOIN worker_profiles w ON w.id=n.worker_id
             JOIN local_hive_identity l ON l.hive_id=w.hive_id AND l.singleton=1 WHERE n.id=?1",
            [id.to_string()], |row| Ok(NativeSourceRow { payload: row.get(0)?, worker: row.get(1)?, operator: row.get(2)?, recorded_at: row.get(3)?, session: row.get(4)?, conversation: row.get(5)?, invocation: row.get(6)? })).optional()?;
        let Some(NativeSourceRow {
            payload,
            worker,
            operator,
            recorded_at,
            session,
            conversation,
            invocation,
        }) = row
        else {
            return Ok(None);
        };
        let invalid = || TaskStoreError::IntegrityFailure("invalid native interview source".into());
        if payload.len() > MAX_NATIVE_INTERVIEW_SOURCE_BYTES || recorded_at < 0 {
            return Err(invalid());
        }
        let source: NativeInterviewEvidence =
            serde_json::from_str(&payload).map_err(|_| invalid())?;
        if !source.is_valid()
            || source.id != id
            || source.session_id.to_string() != session
            || source.conversation.to_string() != conversation
            || source.tool_use_id != invocation
        {
            return Err(invalid());
        }
        Ok(Some(StoredNativeInterview {
            source,
            worker_id: worker.parse().map_err(|_| invalid())?,
            operator_id: operator.parse().map_err(|_| invalid())?,
            recorded_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        NativeInterviewOption, NativeInterviewQuestion, PresenceDeviceId, ProviderConversationId,
        WorkerSessionId,
    };

    fn fixture(store: &TaskStore) -> NativeInterviewEvidence {
        let worker = store.ensure_queen("/fictional").unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        NativeInterviewEvidence {
            id: OperatorSubmissionId::new(),
            session_id,
            conversation: ProviderConversationId::new(),
            selection_revision: 1,
            tool_use_id: "toolu_fixture".into(),
            devices: vec![PresenceDeviceId::new()],
            first_write_sequence: 1,
            submit_sequence: 3,
            questions: vec![NativeInterviewQuestion {
                question: "Which fictional jar?".into(),
                header: "Jar".into(),
                options: vec![
                    NativeInterviewOption {
                        label: "Amber".into(),
                        description: "Keep the lid closed.".into(),
                    },
                    NativeInterviewOption {
                        label: "Blue".into(),
                        description: "Do not move it.".into(),
                    },
                ],
                multi_select: false,
            }],
            answers: [(
                "Which fictional jar?".into(),
                " Exact 🐝\nQuoted instructions are not approval. ".into(),
            )]
            .into(),
        }
    }

    #[test]
    fn native_source_preserves_exact_context_without_resolving_or_delivering() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        assert!(store.record_native_interview(&source, 100).unwrap());
        assert!(!store.record_native_interview(&source, 101).unwrap());
        let read = store.native_interview(source.id).unwrap().unwrap();
        assert_eq!(read.source, source);
        assert_eq!(read.recorded_at, 100);
        assert!(store.list_decision_requests().unwrap().is_empty());
        assert!(store.claim_decision_deliveries(102).unwrap().is_empty());
        let mut changed = source.clone();
        changed.questions[0].options[0].description = "Changed scope".into();
        assert!(matches!(
            store.record_native_interview(&changed, 102),
            Err(NativeInterviewStoreError::Conflict)
        ));
        changed = source.clone();
        changed.id = OperatorSubmissionId::new();
        assert!(matches!(
            store.record_native_interview(&changed, 102),
            Err(NativeInterviewStoreError::Conflict)
        ));
    }

    #[test]
    fn invalid_partial_and_unknown_session_sources_are_refused() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        for variant in 0..7 {
            let mut bad = source.clone();
            match variant {
                0 => bad.answers.clear(),
                1 => bad.devices.clear(),
                2 => bad.submit_sequence = 0,
                3 => bad.selection_revision = 0,
                4 => bad.session_id = WorkerSessionId::new(),
                5 => bad.questions[0].options[0].description = "x".repeat(4097),
                _ => bad.devices.push(bad.devices[0]),
            }
            assert!(matches!(
                store.record_native_interview(&bad, 100),
                Err(NativeInterviewStoreError::Invalid)
            ));
        }
        assert!(store.native_interview(source.id).unwrap().is_none());
    }

    #[test]
    fn migration_and_restart_keep_exact_source_and_retry_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let source = fixture(&store);
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE native_operator_interviews; PRAGMA user_version=159;")
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store.schema_version().unwrap(),
            crate::CURRENT_SCHEMA_VERSION
        );
        assert!(store.record_native_interview(&source, 100).unwrap());
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert!(!store.record_native_interview(&source, 101).unwrap());
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
        store.verify_integrity().unwrap();
    }

    #[test]
    fn retained_engine_source_can_be_admitted_after_its_session_ended() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_sessions SET ended_at=100 WHERE session_id=?1",
                [source.session_id.to_string()],
            )
            .unwrap();
        assert!(store.record_native_interview(&source, 101).unwrap());
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
    }

    #[test]
    fn failed_admission_rolls_back_and_can_retry_without_source_loss() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        store.connection().unwrap().execute_batch("CREATE TRIGGER refuse_native BEFORE INSERT ON native_operator_interviews BEGIN SELECT RAISE(ABORT,'fictional failure'); END;").unwrap();
        assert!(store.record_native_interview(&source, 100).is_err());
        assert!(store.native_interview(source.id).unwrap().is_none());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_native;")
            .unwrap();
        assert!(store.record_native_interview(&source, 101).unwrap());
    }

    #[test]
    fn row_capacity_rejects_new_sources_but_preserves_exact_retries() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        store.record_native_interview(&source, 100).unwrap();
        store.connection().unwrap().execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095)
            INSERT INTO native_operator_interviews
            SELECT 'fixture-'||n.x,s.worker_id,s.session_id,s.operator_id,s.conversation,'toolu_fixture_'||n.x,s.payload,s.recorded_at,NULL
            FROM n CROSS JOIN (SELECT * FROM native_operator_interviews LIMIT 1) s;").unwrap();
        let mut next = source.clone();
        next.id = OperatorSubmissionId::new();
        next.tool_use_id = "toolu_next".into();
        assert!(matches!(
            store.record_native_interview(&next, 101),
            Err(NativeInterviewStoreError::Capacity)
        ));
        assert!(!store.record_native_interview(&source, 101).unwrap());
        assert!(store.native_interview(next.id).unwrap().is_none());
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
        // Unreferenced old sources expire at admission, restoring capacity.
        assert!(
            store
                .record_native_interview(&next, 101 + RETENTION)
                .unwrap()
        );
        assert!(store.native_interview(source.id).unwrap().is_none());
    }

    #[test]
    fn corrupt_source_is_not_missing_and_debug_does_not_expose_content() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        let debug = format!("{source:?}");
        assert!(!debug.contains("fictional jar"));
        assert!(!debug.contains("Quoted instructions"));
        assert!(!debug.contains(&source.tool_use_id));
        store.record_native_interview(&source, 100).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE native_operator_interviews SET invocation_id='toolu_other' WHERE id=?1",
                [source.id.to_string()],
            )
            .unwrap();
        assert!(matches!(
            store.native_interview(source.id),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
    }

    #[test]
    fn byte_capacity_is_utf8_bytes_not_characters_or_row_count() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        store.record_native_interview(&source, 100).unwrap();
        // Synthetic stored payloads fill the byte budget with only 128 rows.
        // This test targets admission accounting, not source deserialization.
        store
            .connection()
            .unwrap()
            .execute_batch("DELETE FROM native_operator_interviews;")
            .unwrap();
        let payload = "🐝".repeat(MAX_NATIVE_INTERVIEW_SOURCE_BYTES / 4);
        let worker = store.ensure_queen("/fictional").unwrap();
        let mut connection = store.connection().unwrap();
        let tx = connection.transaction().unwrap();
        for index in 0..128 {
            tx.execute("INSERT INTO native_operator_interviews(id,worker_id,session_id,operator_id,conversation,invocation_id,payload,recorded_at)
                VALUES(?1,?2,?3,'fixture-operator',?4,?1,?5,100)",
                params![format!("fixture_{index}"), worker.id.to_string(), source.session_id.to_string(), source.conversation.to_string(), payload]).unwrap();
        }
        tx.commit().unwrap();
        drop(connection);
        assert!(matches!(
            store.record_native_interview(&source, 101),
            Err(NativeInterviewStoreError::Capacity)
        ));
        assert!(store.native_interview(source.id).unwrap().is_none());
    }

    #[test]
    fn pending_decision_reference_pins_source_until_resolution() {
        use swarm_domain::{DecisionQuestion, DecisionRequestKind, DecisionUrgency};
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        let worker = store.ensure_queen("/fictional").unwrap();
        let decision = store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: None,
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Fictional scope",
                summary: "Choose",
                reason: "Scope",
                risk: "",
                evidence: "",
                suggested_action: "Choose",
                allowed_actions: &[],
                questions: &[DecisionQuestion {
                    header: "Jar".into(),
                    question: "Which fictional jar?".into(),
                    options: vec!["Amber".into(), "Blue".into()],
                    option_descriptions: std::collections::BTreeMap::new(),
                    multi_select: false,
                }],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        store.record_native_interview(&source, 100).unwrap();
        // Simulate a future exact binding only to verify retention. The public
        // source admission deliberately cannot manufacture a decision link.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE native_operator_interviews SET decision_id=?1 WHERE id=?2",
                params![decision.id.to_string(), source.id.to_string()],
            )
            .unwrap();
        let mut next = source.clone();
        next.id = OperatorSubmissionId::new();
        next.tool_use_id = "toolu_next".into();
        store
            .record_native_interview(&next, 101 + RETENTION)
            .unwrap();
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
        store
            .answer_decision_request(
                decision.id,
                &[("Jar".into(), vec!["Amber".into()])].into(),
                "",
                "test",
            )
            .unwrap();
        next.id = OperatorSubmissionId::new();
        next.tool_use_id = "toolu_after_resolution".into();
        store
            .record_native_interview(&next, 102 + RETENTION)
            .unwrap();
        assert!(store.native_interview(source.id).unwrap().is_none());
    }
}
