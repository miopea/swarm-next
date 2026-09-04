//! Private first-party evidence, never automatic diagnostic content.
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    DecisionQuestion, DecisionRequestId, MAX_DECISION_QUESTIONS, OperatorAnswerConsumption,
    OperatorAnswerEvidence, OperatorAnswerTarget, OperatorStatementId, WorkerId, WorkerSessionId,
    confirmed_operator_interview,
};
use thiserror::Error;

use crate::{TaskStore, TaskStoreError};

const MAX_RECORDS: i64 = 4096;
const MAX_BYTES: i64 = 16 * 1024 * 1024;
const RETENTION: i64 = 90 * 86_400;

/// Deliberately has no Debug implementation: answer content is not diagnostics.
#[derive(serde::Serialize)]
pub struct VerifiedOperatorStatement {
    pub id: OperatorStatementId,
    pub decision_id: DecisionRequestId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub operator_id: swarm_domain::OperatorId,
    pub question: DecisionQuestion,
    pub answer: String,
    pub recorded_at: i64,
    pub used_for_resolution: bool,
}

pub(super) fn migrate_resolutions(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::OPERATOR_STATEMENT_RESOLUTIONS_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch("CREATE TABLE operator_statement_resolutions (
        statement_id TEXT PRIMARY KEY REFERENCES operator_statements(id) ON DELETE CASCADE,
        decision_id TEXT NOT NULL REFERENCES decision_requests(id) ON DELETE CASCADE
    ); CREATE INDEX operator_statement_resolution_decision ON operator_statement_resolutions(decision_id);")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::OPERATOR_STATEMENT_RESOLUTIONS_SCHEMA_VERSION,
    )
}

#[derive(Debug, Error)]
pub enum OperatorStatementError {
    #[error("operator answer is not confirmed for a current local decision and session")]
    Invalid,
    #[error("operator statement identity already carries different evidence")]
    Conflict,
    #[error("operator statement storage is full; no unresolved evidence was discarded")]
    Capacity,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub(super) fn migrate(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::OPERATOR_STATEMENTS_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch(
        "CREATE TABLE operator_statements (
            id TEXT PRIMARY KEY,
            decision_id TEXT NOT NULL REFERENCES decision_requests(id) ON DELETE CASCADE,
            worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
            session_id TEXT NOT NULL REFERENCES worker_sessions(session_id),
            operator_id TEXT NOT NULL,
            question TEXT NOT NULL CHECK(length(CAST(question AS BLOB)) <= 16384),
            answer TEXT NOT NULL CHECK(length(CAST(answer AS BLOB)) BETWEEN 1 AND 16384),
            recorded_at INTEGER NOT NULL CHECK(recorded_at >= 0)
        );
        CREATE INDEX operator_statements_decision ON operator_statements(decision_id, recorded_at);"
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::OPERATOR_STATEMENTS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Reads one exact local-Hive first-party receipt, including its scope.
    /// Absence is not proof of approval or rejection. No input is replayed.
    ///
    /// # Errors
    /// Returns storage or integrity errors, never an invented missing record.
    pub fn verified_operator_statement(
        &self,
        id: OperatorStatementId,
    ) -> Result<Option<VerifiedOperatorStatement>, TaskStoreError> {
        let connection = self.connection()?;
        let receipt = connection.query_row(
            "SELECT s.decision_id, s.worker_id, s.session_id, s.operator_id, s.question, s.answer,
                    s.recorded_at, EXISTS(SELECT 1 FROM operator_statement_resolutions r
                        WHERE r.statement_id=s.id AND r.decision_id=s.decision_id)
             FROM operator_statements s JOIN decision_requests d ON d.id=s.decision_id
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1 WHERE s.id=?1",
            [id.to_string()], |row| {
                let parse = |index| -> rusqlite::Result<String> { row.get(index) };
                Ok(VerifiedOperatorStatement {
                    id,
                    decision_id: parse(0)?.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    worker_id: parse(1)?.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: parse(2)?.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    operator_id: parse(3)?.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    question: serde_json::from_str(&parse(4)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    answer: row.get(5)?, recorded_at: row.get(6)?, used_for_resolution: row.get(7)?,
                })
            }).optional()?;
        if let Some(receipt) = &receipt
            && (receipt.answer.trim().is_empty()
                || receipt.answer.len() > swarm_domain::MAX_OPERATOR_ANSWER_BYTES
                || !swarm_domain::valid_decision_questions(std::slice::from_ref(&receipt.question)))
        {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid operator statement".into(),
            ));
        }
        Ok(receipt)
    }

    /// Resolves an interview from a complete set of stored, confirmed receipts.
    /// Revalidates question and session identities in the same transaction as
    /// resolution and delivery bookkeeping. No terminal input is written here.
    /// Returns false for the identical already-consumed receipt set.
    ///
    /// # Errors
    /// Rejects incomplete, mismatched, stale, conflicting or unavailable evidence.
    pub fn resolve_operator_statement_interview(
        &self,
        decision: DecisionRequestId,
        worker: WorkerId,
        session: WorkerSessionId,
        ids: &[OperatorStatementId],
    ) -> Result<bool, OperatorStatementError> {
        if ids.is_empty()
            || ids.len() > MAX_DECISION_QUESTIONS
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
        {
            return Err(OperatorStatementError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let (state, questions): (String, String) = tx
            .query_row(
                "SELECT d.state, d.questions FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id=d.hive_id AND l.singleton=1 WHERE d.id=?1",
                [decision.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(OperatorStatementError::Invalid)?;
        let questions: Vec<DecisionQuestion> =
            serde_json::from_str(&questions).map_err(|_| OperatorStatementError::Invalid)?;
        let mut evidence = Vec::with_capacity(ids.len());
        for id in ids {
            let (question, answer): (String, String) = tx
                .query_row(
                    "SELECT question, answer FROM operator_statements
                 WHERE id=?1 AND decision_id=?2 AND worker_id=?3 AND session_id=?4",
                    params![
                        id.to_string(),
                        decision.to_string(),
                        worker.to_string(),
                        session.to_string()
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or(OperatorStatementError::Invalid)?;
            evidence.push(
                OperatorAnswerEvidence::new(
                    OperatorAnswerTarget {
                        decision_id: decision,
                        worker_id: worker,
                        session_id: session,
                        question: serde_json::from_str(&question)
                            .map_err(|_| OperatorStatementError::Invalid)?,
                    },
                    answer,
                    OperatorAnswerConsumption::Confirmed,
                )
                .ok_or(OperatorStatementError::Invalid)?,
            );
        }
        let answers =
            confirmed_operator_interview(decision, worker, session, &questions, &evidence)
                .ok_or(OperatorStatementError::Invalid)?;
        if state != "pending" {
            let linked: Vec<String> = {
                let mut query = tx.prepare("SELECT statement_id FROM operator_statement_resolutions WHERE decision_id=?1 LIMIT 5")?;
                query
                    .query_map([decision.to_string()], |row| row.get(0))?
                    .collect::<Result<_, _>>()?
            };
            return if linked.len() == ids.len()
                && ids.iter().all(|id| linked.contains(&id.to_string()))
            {
                Ok(false)
            } else {
                Err(OperatorStatementError::Conflict)
            };
        }
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM worker_sessions WHERE session_id=?1 AND worker_id=?2 AND ended_at IS NULL)",
            params![session.to_string(), worker.to_string()], |row| row.get(0))?;
        if !active {
            return Err(OperatorStatementError::Invalid);
        }
        crate::decisions::write_answer_resolution(
            &tx,
            decision,
            &answers,
            "",
            "verified_terminal",
            Some((worker, session)),
        )?;
        for id in ids {
            tx.execute("INSERT INTO operator_statement_resolutions(statement_id, decision_id) VALUES(?1,?2)", params![id.to_string(), decision.to_string()])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Persists caller-authenticated, provider-consumed answer evidence.
    /// This is not an agent-facing write API. The application must authenticate
    /// the human source; the evidence type itself is not an authentication token.
    /// Does not resolve the decision or replay input. Returns false for an exact
    /// retry of the immutable statement ID, including after its session ended.
    ///
    /// # Errors
    /// Rejects unconfirmed input, invalid binding, conflicting IDs, full storage,
    /// and persistence failures. Ordinary terminal input must remain available.
    pub fn record_operator_statement(
        &self,
        id: OperatorStatementId,
        evidence: &OperatorAnswerEvidence,
        now: i64,
    ) -> Result<bool, OperatorStatementError> {
        if now < 0 || evidence.consumption() != OperatorAnswerConsumption::Confirmed {
            return Err(OperatorStatementError::Invalid);
        }
        let target = evidence.target();
        let question =
            serde_json::to_string(&target.question).map_err(|_| OperatorStatementError::Invalid)?;
        if question.len() > 16384 {
            return Err(OperatorStatementError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let previous: Option<bool> = tx
            .query_row(
                "SELECT decision_id = ?2 AND worker_id = ?3 AND session_id = ?4
                    AND question = ?5 AND answer = ?6 FROM operator_statements WHERE id = ?1",
                params![
                    id.to_string(),
                    target.decision_id.to_string(),
                    target.worker_id.to_string(),
                    target.session_id.to_string(),
                    question,
                    evidence.text()
                ],
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
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM decision_requests d
             JOIN local_hive_identity l ON l.hive_id = d.hive_id AND l.singleton = 1
             JOIN worker_profiles w ON w.id = ?2 AND w.hive_id = d.hive_id
             JOIN worker_sessions s ON s.worker_id = w.id AND s.session_id = ?3 AND s.ended_at IS NULL
             WHERE d.id = ?1 AND d.state = 'pending'
               AND EXISTS(SELECT 1 FROM json_each(d.questions) q WHERE
                   json_extract(q.value, '$.header') = ?4
                   AND json(q.value) = json(?5)))",
            params![target.decision_id.to_string(), target.worker_id.to_string(), target.session_id.to_string(),
                target.question.header, question], |row| row.get(0)
        )?;
        if !valid {
            return Err(OperatorStatementError::Invalid);
        }
        // Only closed, aged evidence expires. Open questions remain pinned even
        // past retention; capacity failure never pretends they were answered.
        tx.execute(
            "DELETE FROM operator_statements WHERE recorded_at < ?1
            AND decision_id IN (SELECT id FROM decision_requests WHERE state = 'resolved')",
            [now.saturating_sub(RETENTION)],
        )?;
        let (count, bytes): (i64, i64) = tx.query_row(
            "SELECT count(*), coalesce(sum(length(CAST(question AS BLOB)) + length(CAST(answer AS BLOB))), 0)
             FROM operator_statements", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let added = i64::try_from(question.len() + evidence.text().len())
            .map_err(|_| OperatorStatementError::Capacity)?;
        if count >= MAX_RECORDS || bytes.saturating_add(added) > MAX_BYTES {
            return Err(OperatorStatementError::Capacity);
        }
        tx.execute("INSERT INTO operator_statements(id, decision_id, worker_id, session_id, operator_id, question, answer, recorded_at)
            SELECT ?1, ?2, ?3, ?4, h.operator_id, ?5, ?6, ?7
            FROM local_hive_identity l JOIN hives h ON h.id = l.hive_id WHERE l.singleton = 1",
            params![id.to_string(), target.decision_id.to_string(), target.worker_id.to_string(),
                target.session_id.to_string(), question, evidence.text(), now])?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewDecisionRequest;
    use swarm_domain::{
        DecisionQuestion, DecisionRequestKind, DecisionUrgency, OperatorAnswerTarget,
        WorkerSessionId,
    };

    fn fixture() -> (TaskStore, OperatorAnswerEvidence) {
        fixture_in(TaskStore::in_memory().unwrap())
    }

    fn fixture_in(store: TaskStore) -> (TaskStore, OperatorAnswerEvidence) {
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO worker_sessions(session_id,worker_id) VALUES(?1,?2)",
                params![session.to_string(), worker.id.to_string()],
            )
            .unwrap();
        let question = DecisionQuestion {
            header: "Scope".into(),
            question: "Which scope?".into(),
            options: vec!["Narrow".into(), "Broad".into()],
            multi_select: false,
        };
        let decision = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: None,
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Scope",
                summary: "Choose scope",
                reason: "Need scope",
                risk: "",
                evidence: "",
                suggested_action: "Choose",
                allowed_actions: &[],
                questions: std::slice::from_ref(&question),
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        let evidence = OperatorAnswerEvidence::new(
            OperatorAnswerTarget {
                decision_id: decision.id,
                worker_id: worker.id,
                session_id: session,
                question,
            },
            "Narrow".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        (store, evidence)
    }

    #[test]
    fn confirmed_resolution_clears_attention_without_redelivery() {
        let (store, evidence) = fixture();
        let id = OperatorStatementId::new();
        let target = evidence.target();
        store.record_operator_statement(id, &evidence, 100).unwrap();
        let resolve = || {
            store.resolve_operator_statement_interview(
                target.decision_id,
                target.worker_id,
                target.session_id,
                &[id],
            )
        };
        let receipt = store.verified_operator_statement(id).unwrap().unwrap();
        assert_eq!(receipt.answer, "Narrow");
        assert_eq!(receipt.question, target.question);
        assert_eq!(receipt.session_id, target.session_id);
        assert!(!receipt.used_for_resolution);
        assert!(resolve().unwrap());
        assert!(!resolve().unwrap());
        assert!(
            store
                .verified_operator_statement(id)
                .unwrap()
                .unwrap()
                .used_for_resolution
        );
        assert!(
            store
                .verified_operator_statement(OperatorStatementId::new())
                .unwrap()
                .is_none()
        );
        let decision = store.get_decision_request(target.decision_id).unwrap();
        assert_eq!(decision.state, swarm_domain::DecisionRequestState::Resolved);
        assert_eq!(
            decision.delivery_state,
            Some(swarm_domain::DecisionDeliveryState::Delivered)
        );
        assert_eq!(decision.resolution_answers["Scope"], ["Narrow"]);
        assert!(store.claim_decision_deliveries(101).unwrap().is_empty());
        assert!(
            !store
                .workers_awaiting_operator()
                .unwrap()
                .contains(&target.worker_id)
        );
    }

    #[test]
    fn stale_receipts_leave_attention_pending() {
        let (store, evidence) = fixture();
        let id = OperatorStatementId::new();
        let target = evidence.target();
        store.record_operator_statement(id, &evidence, 100).unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    target.worker_id,
                    target.session_id,
                    &[]
                )
                .is_err()
        );
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    target.worker_id,
                    target.session_id,
                    &[id, id]
                )
                .is_err()
        );
        store
            .connection()
            .unwrap()
            .execute("UPDATE worker_sessions SET ended_at=101", [])
            .unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    target.worker_id,
                    target.session_id,
                    &[id]
                )
                .is_err()
        );
        assert_eq!(
            store
                .get_decision_request(target.decision_id)
                .unwrap()
                .state,
            swarm_domain::DecisionRequestState::Pending
        );
    }

    #[test]
    fn worker_answers_complete_queens_interview_and_queue_only_her_notification() {
        let (store, first) = fixture();
        let queen = first.target().worker_id;
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let mut target = first.target().clone();
        target.worker_id = worker.id;
        target.session_id = session;
        let one = OperatorAnswerEvidence::new(
            target.clone(),
            "Narrow".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        target.question.header = "Timing".into();
        target.question.question = "When?".into();
        let two = OperatorAnswerEvidence::new(
            target.clone(),
            "After lunch".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        let questions =
            serde_json::to_string(&[one.target().question.clone(), two.target().question.clone()])
                .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE decision_requests SET questions=?2 WHERE id=?1",
                params![target.decision_id.to_string(), questions],
            )
            .unwrap();
        let first_id = OperatorStatementId::new();
        let second_id = OperatorStatementId::new();
        store
            .record_operator_statement(first_id, &one, 100)
            .unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    worker.id,
                    session,
                    &[first_id]
                )
                .is_err()
        );
        assert_eq!(
            store
                .get_decision_request(target.decision_id)
                .unwrap()
                .state,
            swarm_domain::DecisionRequestState::Pending
        );
        store
            .record_operator_statement(second_id, &two, 101)
            .unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    worker.id,
                    session,
                    &[second_id, first_id]
                )
                .unwrap()
        );
        assert!(
            !store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    worker.id,
                    session,
                    &[first_id, second_id]
                )
                .unwrap()
        );
        let deliveries = store.claim_decision_deliveries(102).unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].worker_id, queen);
        assert_eq!(deliveries[0].answers["Timing"], ["After lunch"]);
    }

    #[test]
    fn failed_evidence_link_rolls_back_resolution_and_delivery() {
        let (store, evidence) = fixture();
        let id = OperatorStatementId::new();
        let target = evidence.target();
        store.record_operator_statement(id, &evidence, 100).unwrap();
        store.connection().unwrap().execute_batch("CREATE TRIGGER reject_link BEFORE INSERT ON operator_statement_resolutions BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    target.worker_id,
                    target.session_id,
                    &[id]
                )
                .is_err()
        );
        let decision = store.get_decision_request(target.decision_id).unwrap();
        assert_eq!(decision.state, swarm_domain::DecisionRequestState::Pending);
        assert!(decision.delivery_state.is_none());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_link;")
            .unwrap();
        assert!(
            store
                .resolve_operator_statement_interview(
                    target.decision_id,
                    target.worker_id,
                    target.session_id,
                    &[id]
                )
                .unwrap()
        );
    }

    #[test]
    fn immutable_receipt_is_idempotent_and_does_not_resolve_or_deliver() {
        let (store, evidence) = fixture();
        let id = OperatorStatementId::new();
        assert!(store.record_operator_statement(id, &evidence, 100).unwrap());
        assert!(!store.record_operator_statement(id, &evidence, 101).unwrap());
        let changed = OperatorAnswerEvidence::new(
            evidence.target().clone(),
            "Broad".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        assert!(matches!(
            store.record_operator_statement(id, &changed, 102),
            Err(OperatorStatementError::Conflict)
        ));
        let decision = store
            .get_decision_request(evidence.target().decision_id)
            .unwrap();
        assert_eq!(decision.state, swarm_domain::DecisionRequestState::Pending);
        assert!(decision.delivery_state.is_none());
        store
            .connection()
            .unwrap()
            .execute("UPDATE worker_sessions SET ended_at=102", [])
            .unwrap();
        assert!(!store.record_operator_statement(id, &evidence, 103).unwrap());
        assert!(matches!(
            store.record_operator_statement(OperatorStatementId::new(), &evidence, 103),
            Err(OperatorStatementError::Invalid)
        ));
    }

    #[test]
    fn unconfirmed_or_changed_question_evidence_is_rejected() {
        let (store, evidence) = fixture();
        let pending = OperatorAnswerEvidence::new(
            evidence.target().clone(),
            "Narrow".into(),
            OperatorAnswerConsumption::Unconfirmed,
        )
        .unwrap();
        assert!(matches!(
            store.record_operator_statement(OperatorStatementId::new(), &pending, 100),
            Err(OperatorStatementError::Invalid)
        ));
        let mut target = evidence.target().clone();
        target.question.options.reverse();
        let changed = OperatorAnswerEvidence::new(
            target,
            "Narrow".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        assert!(matches!(
            store.record_operator_statement(OperatorStatementId::new(), &changed, 100),
            Err(OperatorStatementError::Invalid)
        ));
    }

    #[test]
    fn capacity_preserves_old_evidence_for_pending_decisions() {
        let (store, evidence) = fixture();
        store
            .record_operator_statement(OperatorStatementId::new(), &evidence, 0)
            .unwrap();
        store.connection().unwrap().execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095)
            INSERT INTO operator_statements SELECT 'fixture-'||n.x, s.decision_id,s.worker_id,s.session_id,s.operator_id,s.question,s.answer,s.recorded_at FROM n CROSS JOIN (SELECT * FROM operator_statements LIMIT 1) s;").unwrap();
        assert!(matches!(
            store.record_operator_statement(OperatorStatementId::new(), &evidence, RETENTION + 1),
            Err(OperatorStatementError::Capacity)
        ));
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM operator_statements", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, MAX_RECORDS);
    }

    #[test]
    fn upgrade_and_restart_preserve_exact_receipt_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("statements.sqlite3");
        let (store, evidence) = fixture_in(TaskStore::open(&path).unwrap());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE operator_submissions; DROP TABLE operator_statement_resolutions; DROP TABLE operator_statements; PRAGMA user_version = 129;")
            .unwrap();
        drop(store);
        let reopened = TaskStore::open(&path).unwrap();
        assert_eq!(
            reopened.schema_version().unwrap(),
            crate::CURRENT_SCHEMA_VERSION
        );
        let id = OperatorStatementId::new();
        assert!(
            reopened
                .record_operator_statement(id, &evidence, 100)
                .unwrap()
        );
        drop(reopened);
        let reopened = TaskStore::open(&path).unwrap();
        assert!(
            !reopened
                .record_operator_statement(id, &evidence, 101)
                .unwrap()
        );
    }

    #[test]
    fn expired_closed_receipts_are_pruned_but_open_receipts_are_retained() {
        let (store, evidence) = fixture();
        let old = OperatorStatementId::new();
        store.record_operator_statement(old, &evidence, 0).unwrap();
        store
            .answer_decision_request(
                evidence.target().decision_id,
                &std::collections::BTreeMap::from([("Scope".into(), vec!["Narrow".into()])]),
                "",
                "test",
            )
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute("UPDATE worker_sessions SET ended_at=1", [])
            .unwrap();
        let (store, new_evidence) = fixture_in(store);
        store
            .record_operator_statement(OperatorStatementId::new(), &new_evidence, RETENTION + 1)
            .unwrap();
        let exists: bool = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM operator_statements WHERE id=?1)",
                [old.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists);
    }
}
