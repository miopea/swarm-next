//! Authoritative evidence identity for Queen's task-scoped review receipts.

use rusqlite::{Connection, OptionalExtension, types::ValueRef};
use sha2::{Digest, Sha256};
use swarm_domain::{
    ControlRoomEventKind, MAX_QUEEN_REVIEW_OBLIGATIONS, NextMoveOwner, QueenReviewCoverage,
    QueenReviewDispositionInput, QueenReviewDispositionKind, QueenReviewObligation,
    TaskActivityActor, TaskId, VerifiedQueenReviewReceipt, queen_review_coverage,
};

use crate::{TaskStore, TaskStoreError};

const MAX_REVIEW_SOURCE_ROWS: usize = 256;

pub(super) fn migrate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let original: String = transaction.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='queen_automation'",
        [],
        |row| row.get(0),
    )?;
    if !original.contains("'incomplete'") {
        let objects = {
            let mut statement = transaction.prepare(
                "SELECT sql FROM sqlite_master WHERE tbl_name='queen_automation' AND type IN ('index','trigger') AND sql IS NOT NULL",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let replacement = original
            .replacen("queen_automation", "queen_automation_review", 1)
            .replace(
                "'completed','needs_operator','no_action'",
                "'completed','needs_operator','no_action','incomplete'",
            );
        transaction.execute_batch(&replacement)?;
        transaction.execute_batch(
            "INSERT INTO queen_automation_review SELECT * FROM queen_automation;
             DROP TABLE queen_automation;
             ALTER TABLE queen_automation_review RENAME TO queen_automation;",
        )?;
        for sql in objects {
            transaction.execute_batch(&sql)?;
        }
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_task_review_receipts (
        task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        run_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('external_condition','operator_deferral')),
        accepted_revision TEXT NOT NULL CHECK(length(accepted_revision)=64),
        input_payload TEXT NOT NULL CHECK(length(input_payload)<=32768),
        recorded_sequence INTEGER NOT NULL REFERENCES task_activity(sequence),
        recorded_at INTEGER NOT NULL
    );",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::QUEEN_REVIEW_RECEIPTS_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Historical review order only, never evidence that a wait remains valid.
    ///
    /// # Errors
    /// Refuses storage failures and an oversized open-task receipt set.
    pub fn queen_review_check_times(
        &self,
    ) -> Result<std::collections::HashMap<TaskId, i64>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT r.task_id,r.recorded_at FROM queen_task_review_receipts r
             JOIN tasks t ON t.id=r.task_id
             WHERE t.removed_at IS NULL
               AND t.hive_id=(SELECT hive_id FROM local_hive_identity WHERE singleton=1)
               AND t.state NOT IN ('completed','abandoned')
             ORDER BY r.task_id LIMIT 257",
        )?;
        let mut times = std::collections::HashMap::new();
        for row in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })? {
            let (id, checked_at) = row?;
            if times.len() == MAX_QUEEN_REVIEW_OBLIGATIONS {
                return Err(TaskStoreError::IntegrityFailure(
                    "open review ordering exceeds its bounded read".into(),
                ));
            }
            let id = id
                .parse::<TaskId>()
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            times.insert(id, checked_at);
        }
        Ok(times)
    }

    /// Read task facts and a revision from the same transaction.
    ///
    /// # Errors
    /// Refuses absent tasks, bounded-source overflow and storage failures.
    pub fn queen_task_review_snapshot(
        &self,
        task_id: TaskId,
    ) -> Result<swarm_domain::QueenTaskReviewEvidence, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let obligation = task_review_evidence(&transaction, task_id)?;
        let task = transaction.query_row(
            &format!("{} WHERE t.id=?1", TaskStore::TASK_PROJECTION),
            [task_id.to_string()],
            crate::task_from_row,
        )?;
        let current_run_id: Option<String> = transaction.query_row(
            "SELECT run_id FROM queen_automation WHERE id=1
             AND (state IN ('running','uncertain') OR (state IN ('queued','delivering') AND delivered_at IS NOT NULL))
             AND run_id IS NOT NULL AND delivery_session_id IS NOT NULL",
            [], |row| row.get(0),
        ).optional()?.flatten();
        let saved: Option<(String, String, i64)> = transaction.query_row(
            "SELECT input_payload,accepted_revision,recorded_at FROM queen_task_review_receipts WHERE task_id=?1",
            [task_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let previous_assessment = saved
            .map(|(payload, revision, recorded_at)| {
                let assessment: QueenReviewDispositionInput = serde_json::from_str(&payload)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
                assessment
                    .validate()
                    .map_err(|reason| TaskStoreError::IntegrityFailure(reason.into()))?;
                if assessment.task_id != task_id {
                    return Err(TaskStoreError::IntegrityFailure(
                        "saved review assessment belongs to another task".into(),
                    ));
                }
                let status = swarm_domain::queen_review_assessment_status(
                    assessment.kind,
                    &assessment.run_id,
                    current_run_id.as_deref(),
                    revision == obligation.evidence_revision,
                );
                Ok(swarm_domain::QueenReviewAssessmentEvidence {
                    assessment,
                    recorded_at,
                    status,
                })
            })
            .transpose()?;
        transaction.commit()?;
        Ok(swarm_domain::QueenTaskReviewEvidence {
            task,
            obligation,
            current_run_id,
            previous_assessment,
        })
    }

    /// Record an explicit current assessment, never create authority or resume work.
    ///
    /// # Errors
    /// Refuses stale evidence, invalid run/source, ordinary workers and routing debt.
    pub fn record_queen_review_disposition(
        &self,
        input: &QueenReviewDispositionInput,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<VerifiedQueenReviewReceipt, TaskStoreError> {
        input
            .validate()
            .map_err(|reason| TaskStoreError::IntegrityFailure(reason.into()))?;
        let payload = serde_json::to_string(input)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        crate::task_prerequisites::authorize(&transaction, actor)?;
        let current = task_review_evidence(&transaction, input.task_id)?;
        let id = input.task_id.to_string();
        let replay: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_task_review_receipts WHERE task_id=?1 AND input_payload=?2 AND accepted_revision=?3)",
            rusqlite::params![id, payload, current.evidence_revision], |row| row.get(0),
        )?;
        if replay {
            transaction.commit()?;
            return Ok(VerifiedQueenReviewReceipt {
                task_id: input.task_id,
                evidence_revision: current.evidence_revision,
            });
        }
        let active: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_automation WHERE id=1 AND run_id=?1 AND state IN ('running','uncertain'))",
            [&input.run_id], |row| row.get(0),
        )?;
        if !active || current.evidence_revision != input.expected_revision {
            return Err(TaskStoreError::IntegrityFailure("review run or task evidence changed; reread current evidence before recording a disposition".into()));
        }
        let task = transaction.query_row(
            &format!("{} WHERE t.id=?1", TaskStore::TASK_PROJECTION),
            [&id],
            crate::task_from_row,
        )?;
        if !input.can_cover(task.state, task.next_move_owner) {
            return Err(TaskStoreError::IntegrityFailure("this is not Queen-owned waiting work; route Ready work or use the existing dependency, decision and lifecycle commands".into()));
        }
        if let Some(sequence) = input.operator_activity_sequence {
            let genuine: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_activity WHERE task_id=?1 AND sequence=?2 AND actor_kind='operator' AND length(trim(note))>0)",
                rusqlite::params![id, sequence], |row| row.get(0),
            )?;
            if !genuine {
                return Err(TaskStoreError::IntegrityFailure(
                    "the cited activity is not an authenticated task-linked operator statement"
                        .into(),
                ));
            }
        }
        if let Some(decision_id) = input.operator_decision_id {
            let genuine: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM decision_requests d
                 JOIN task_decision_membership m ON m.decision_id=d.id
                 WHERE m.task_id=?1 AND d.id=?2 AND d.state='resolved'
                   AND d.resolved_by_operator_id IS NOT NULL)",
                rusqlite::params![id, decision_id.to_string()],
                |row| row.get(0),
            )?;
            if !genuine {
                return Err(TaskStoreError::IntegrityFailure("the cited decision is not an authenticated resolved operator decision for this task".into()));
            }
        }
        let kind = match input.kind {
            QueenReviewDispositionKind::ExternalCondition => "external_condition",
            QueenReviewDispositionKind::OperatorDeferral => "operator_deferral",
        };
        transaction.execute(
            "INSERT INTO task_activity (task_id,kind,to_state,note,actor_kind,actor_id,occurred_at) VALUES (?1,'corrected',?2,?3,?4,?5,?6)",
            rusqlite::params![id, task.state.to_string(), format!("Queen review ({kind}): {}\nChecked: {}\nSource: {}", input.condition, input.evidence, input.source), actor.kind.to_string(), actor.id, now],
        )?;
        let recorded_sequence = transaction.last_insert_rowid();
        let accepted = task_review_evidence(&transaction, input.task_id)?;
        transaction.execute(
            "INSERT INTO queen_task_review_receipts (task_id,run_id,kind,accepted_revision,input_payload,recorded_sequence,recorded_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(task_id) DO UPDATE SET
             run_id=excluded.run_id,kind=excluded.kind,accepted_revision=excluded.accepted_revision,input_payload=excluded.input_payload,
             recorded_sequence=excluded.recorded_sequence,recorded_at=excluded.recorded_at",
            rusqlite::params![id, input.run_id, kind, accepted.evidence_revision, payload, recorded_sequence, now],
        )?;
        crate::insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(VerifiedQueenReviewReceipt {
            task_id: input.task_id,
            evidence_revision: accepted.evidence_revision,
        })
    }

    /// Evaluate current obligations without changing the run or its tasks.
    ///
    /// # Errors
    /// Propagates storage/source failures rather than declaring empty coverage.
    pub fn queen_run_review_coverage(
        &self,
        run_id: &str,
    ) -> Result<QueenReviewCoverage, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let coverage = review_coverage(&transaction, run_id)?;
        transaction.commit()?;
        Ok(coverage)
    }
    /// Read an opaque task evidence revision without recording a review.
    ///
    /// # Errors
    /// Refuses absent/nonlocal tasks, incomplete bounded source reads and storage errors.
    pub fn queen_task_review_evidence(
        &self,
        task_id: TaskId,
    ) -> Result<QueenReviewObligation, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let evidence = task_review_evidence(&transaction, task_id)?;
        transaction.commit()?;
        Ok(evidence)
    }
}

pub(super) fn review_coverage(
    connection: &Connection,
    run_id: &str,
) -> Result<QueenReviewCoverage, TaskStoreError> {
    let sql = format!(
        "{} WHERE t.removed_at IS NULL AND t.hive_id=(SELECT hive_id FROM local_hive_identity WHERE singleton=1) AND NOT {}",
        TaskStore::TASK_PROJECTION,
        TaskStore::SETTLED_PREDICATE
    );
    let mut statement = connection.prepare(&sql)?;
    let mut obligations = Vec::new();
    let mut receipts = Vec::new();
    for task in statement.query_map([], crate::task_from_row)? {
        let task = task?;
        if task.next_move_owner != NextMoveOwner::Queen {
            continue;
        }
        if obligations.len() == MAX_QUEEN_REVIEW_OBLIGATIONS {
            return Ok(QueenReviewCoverage::Unavailable);
        }
        let current = task_review_evidence(connection, task.id)?;
        let saved: Option<String> = connection.query_row(
            "SELECT accepted_revision FROM queen_task_review_receipts WHERE task_id=?1 AND (kind='operator_deferral' OR run_id=?2)",
            rusqlite::params![task.id.to_string(), run_id], |row| row.get(0),
        ).optional()?;
        if let Some(evidence_revision) = saved {
            receipts.push(VerifiedQueenReviewReceipt {
                task_id: task.id,
                evidence_revision,
            });
        }
        obligations.push(current);
    }
    Ok(queen_review_coverage(&obligations, &receipts, true))
}

/// Called inside the same transaction as eventual receipt acceptance/completion.
pub(super) fn task_review_evidence(
    connection: &Connection,
    task_id: TaskId,
) -> Result<QueenReviewObligation, TaskStoreError> {
    let task = connection.query_row(
        &format!("{} WHERE t.id = ?1 AND t.removed_at IS NULL AND t.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)", TaskStore::TASK_PROJECTION),
        [task_id.to_string()],
        crate::task_from_row,
    ).optional()?.ok_or(TaskStoreError::NotFound)?;
    let mut digest = Sha256::new();
    digest.update(b"swarm-queen-review-evidence-v1");
    let projection = serde_json::to_vec(&task)
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    digest.update((projection.len() as u64).to_be_bytes());
    digest.update(projection);
    // A correction/message can matter without changing lifecycle state. The
    // sequence, not second-resolution time, observes that change.
    hash_rows(
        connection,
        "SELECT coalesce(max(sequence),0) FROM task_activity WHERE task_id = ?1",
        task_id,
        &mut digest,
    )?;
    // Include exact decision contents: one pending decision replacing another
    // must invalidate a receipt even if the pending count remains the same.
    hash_rows(
        connection,
        "SELECT * FROM decision_requests WHERE id IN
         (SELECT decision_id FROM task_decision_membership WHERE task_id=?1) ORDER BY id LIMIT 257",
        task_id,
        &mut digest,
    )?;
    hash_rows(
        connection,
        "SELECT * FROM task_messages WHERE task_id = ?1 ORDER BY id LIMIT 257",
        task_id,
        &mut digest,
    )?;
    hash_rows(
        connection,
        "SELECT delivery.* FROM task_message_deliveries delivery JOIN task_messages message ON message.id = delivery.message_id WHERE message.task_id = ?1 ORDER BY delivery.message_id LIMIT 257",
        task_id,
        &mut digest,
    )?;
    Ok(QueenReviewObligation {
        task_id,
        evidence_revision: format!("{:x}", digest.finalize()),
    })
}

fn hash_rows(
    connection: &Connection,
    sql: &str,
    task_id: TaskId,
    digest: &mut Sha256,
) -> Result<(), TaskStoreError> {
    // Each fixed source and each typed field is delimited, avoiding ambiguous
    // concatenation. No source text or credentials leave this boundary.
    digest.update((sql.len() as u64).to_be_bytes());
    digest.update(sql.as_bytes());
    let mut statement = connection.prepare(sql)?;
    let columns = statement.column_count();
    let mut rows = statement.query([task_id.to_string()])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > MAX_REVIEW_SOURCE_ROWS {
            return Err(TaskStoreError::IntegrityFailure(
                "Queen review source exceeds its bounded read; coverage is unavailable, not complete".into(),
            ));
        }
        digest.update([255]);
        for column in 0..columns {
            match row.get_ref(column)? {
                ValueRef::Null => digest.update([0]),
                ValueRef::Integer(value) => {
                    digest.update([1]);
                    digest.update(value.to_be_bytes());
                }
                ValueRef::Real(value) => {
                    digest.update([2]);
                    digest.update(value.to_bits().to_be_bytes());
                }
                ValueRef::Text(value) | ValueRef::Blob(value) => {
                    digest.update([if matches!(row.get_ref(column)?, ValueRef::Text(_)) {
                        3
                    } else {
                        4
                    }]);
                    digest.update((value.len() as u64).to_be_bytes());
                    digest.update(value);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{QueenAutomationOutcome, TaskActivityActor, TaskState, WorkerSessionId};

    fn start_review(store: &TaskStore, now: i64) -> String {
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        if queen.active_session_id.is_none() {
            store
                .bind_worker_session(queen.id, WorkerSessionId::new())
                .unwrap();
        }
        store.request_queen_automation_run(now).unwrap();
        let run = store.claim_queen_automation(now).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, now)
            .unwrap();
        run.run_id
    }

    fn external_wait(store: &TaskStore) -> QueenReviewDispositionInput {
        let task = store
            .create_task("External fixture gate", "/workspace/demo")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .transition_task_with_note(
                task.id,
                TaskState::Blocked,
                "External condition requires verification",
            )
            .unwrap();
        let run_id = start_review(store, 100);
        QueenReviewDispositionInput {
            task_id: task.id,
            run_id,
            expected_revision: store
                .queen_task_review_evidence(task.id)
                .unwrap()
                .evidence_revision,
            kind: QueenReviewDispositionKind::ExternalCondition,
            condition: "External fixture endpoint reports maintenance".into(),
            evidence: "Read the fictional maintenance response".into(),
            source: "Fictional endpoint status response".into(),
            operator_activity_sequence: None,
            operator_decision_id: None,
        }
    }

    #[test]
    fn review_order_metadata_is_read_only_and_excludes_settled_work() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        assert!(store.queen_review_check_times().unwrap().is_empty());
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        let count = store
            .list_task_activity(input.task_id, 100)
            .unwrap()
            .events
            .len();
        assert_eq!(
            store
                .queen_review_check_times()
                .unwrap()
                .get(&input.task_id),
            Some(&101)
        );
        assert_eq!(
            store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len(),
            count
        );
        store
            .transition_task(input.task_id, TaskState::Abandoned)
            .unwrap();
        assert!(store.queen_review_check_times().unwrap().is_empty());
    }

    #[test]
    fn review_snapshot_distinguishes_reusable_history_from_a_new_check() {
        use swarm_domain::QueenReviewAssessmentStatus as Status;
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        let before = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(
            before.current_run_id.as_deref(),
            Some(input.run_id.as_str())
        );
        assert!(before.previous_assessment.is_none());
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        let snapshot = store.queen_task_review_snapshot(input.task_id).unwrap();
        let previous = snapshot.previous_assessment.unwrap();
        assert_eq!(previous.status, Status::CoveredForCurrentRun);
        assert_eq!(previous.assessment.condition, input.condition);
        assert_eq!(previous.assessment.source, input.source);
        assert_eq!(previous.recorded_at, 101);
        // A delivered run queued for same-session continuation is still current.
        store
            .connection()
            .unwrap()
            .execute("UPDATE queen_automation SET state='queued' WHERE id=1", [])
            .unwrap();
        let continuing = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(
            continuing.current_run_id.as_deref(),
            Some(input.run_id.as_str())
        );
        assert_eq!(
            continuing.previous_assessment.unwrap().status,
            Status::CoveredForCurrentRun
        );
        let count = store
            .list_task_activity(input.task_id, 100)
            .unwrap()
            .events
            .len();
        store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(
            store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len(),
            count
        );
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 102)
            .unwrap();
        let finished = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert!(finished.current_run_id.is_none());
        assert_eq!(
            finished.previous_assessment.unwrap().status,
            Status::NoActiveReview
        );
        let next = start_review(&store, 103);
        let snapshot = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(snapshot.current_run_id.as_deref(), Some(next.as_str()));
        assert_eq!(
            snapshot.previous_assessment.unwrap().status,
            Status::FreshExternalCheckRequired
        );
        store
            .append_task_correction(
                input.task_id,
                "Changed evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        let stale = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(
            stale.previous_assessment.unwrap().status,
            Status::EvidenceChanged
        );
        assert_eq!(stale.task.state, TaskState::Blocked);
    }

    #[test]
    fn disposition_is_durable_idempotent_and_never_resumes_work() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("review.db");
        let store = TaskStore::open(&path).unwrap();
        let input = external_wait(&store);
        let receipt = store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        let count = store
            .list_task_activity(input.task_id, 100)
            .unwrap()
            .events
            .len();
        assert_eq!(
            store.get_task(input.task_id).unwrap().state,
            TaskState::Blocked
        );
        assert_eq!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        );
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 102)
            .unwrap();
        drop(store);
        let reopened = TaskStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 103)
                .unwrap(),
            receipt
        );
        assert_eq!(
            reopened
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len(),
            count
        );
    }

    #[test]
    fn external_claims_need_a_new_check_each_run_and_new_facts_invalidate() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 102)
            .unwrap();
        let next = start_review(&store, 103);
        assert!(matches!(
            store.queen_run_review_coverage(&next).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
        store
            .append_task_correction(
                input.task_id,
                "Changed external evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 104)
                .is_err()
        );
    }

    #[test]
    fn deferral_requires_an_actual_operator_source_and_can_survive_a_new_run() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        input.kind = QueenReviewDispositionKind::OperatorDeferral;
        input.condition = "Operator requested an interview before proceeding".into();
        input.operator_activity_sequence = Some(i64::MAX);
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
                .is_err()
        );
        store
            .append_task_correction(
                input.task_id,
                "Please wait until my interview",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        input.operator_activity_sequence =
            Some(store.list_task_activity(input.task_id, 1).unwrap().events[0].sequence);
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 102)
            .unwrap();
        let next = start_review(&store, 103);
        assert_eq!(
            store.queen_run_review_coverage(&next).unwrap(),
            QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        );
        assert_eq!(
            store
                .queen_task_review_snapshot(input.task_id)
                .unwrap()
                .previous_assessment
                .unwrap()
                .status,
            swarm_domain::QueenReviewAssessmentStatus::CoveredForCurrentRun,
        );
    }

    #[test]
    fn resolved_decision_deferral_requires_explicit_membership_and_removal_invalidates_it() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let parent = store
            .create_task("Original ruling", "/workspace/demo")
            .unwrap();
        let decision = swarm_domain::DecisionRequestId::new();
        store.connection().unwrap().execute(
            "INSERT INTO decision_requests(id,hive_id,requesting_worker_id,task_id,kind,urgency,title,reason,risk,evidence,suggested_action,allowed_actions)
             VALUES (?1,?2,?3,?4,'input','normal','Fictional deferral','Original scope','','','','[\"Defer this scope\"]')",
            rusqlite::params![decision.to_string(),parent.hive_id.to_string(),queen.id.to_string(),parent.id.to_string()],
        ).unwrap();
        store
            .resolve_decision_request(
                decision,
                "Defer this scope",
                "Fictional original ruling",
                "inbox",
            )
            .unwrap();
        input.kind = QueenReviewDispositionKind::OperatorDeferral;
        input.operator_decision_id = Some(decision);
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
                .is_err()
        );
        store
            .add_task_decision_link(
                input.task_id,
                decision,
                "Same original deferred scope; no new permission",
                &input.expected_revision,
                &TaskActivityActor::operator(),
                102,
            )
            .unwrap();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 103)
            .unwrap();
        let revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .remove_task_decision_link(
                input.task_id,
                decision,
                "Applicability withdrawn",
                &revision,
                &TaskActivityActor::operator(),
                104,
            )
            .unwrap();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 105)
                .is_err()
        );
        assert_eq!(
            store.get_task(input.task_id).unwrap().state,
            TaskState::Blocked
        );
    }

    #[test]
    fn failed_receipt_transaction_preserves_the_original_evidence() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        store.connection().unwrap().execute_batch("CREATE TEMP TRIGGER fail_review_event BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT, 'fictional event failure'); END;").unwrap();
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
                .is_err()
        );
        assert_eq!(
            store
                .queen_task_review_evidence(input.task_id)
                .unwrap()
                .evidence_revision,
            input.expected_revision
        );
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
    }

    #[test]
    fn an_unreviewed_wait_finishes_incomplete_without_resuming_or_escalating() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        let result = store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 101)
            .unwrap();
        assert_eq!(
            result,
            crate::QueenAutomationFinish::Closed(QueenAutomationOutcome::Incomplete)
        );
        assert_eq!(
            store.get_task(input.task_id).unwrap().state,
            TaskState::Blocked
        );
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
    }

    #[test]
    fn checked_wait_can_finish_but_a_changed_fact_prevents_success() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        store
            .append_task_correction(
                input.task_id,
                "The fixture gate changed",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        assert_eq!(
            store
                .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::Completed, 102)
                .unwrap(),
            crate::QueenAutomationFinish::Closed(QueenAutomationOutcome::Incomplete)
        );
    }

    #[test]
    fn explicitly_incomplete_can_close_even_when_evidence_is_unavailable() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        for index in 0..=MAX_REVIEW_SOURCE_ROWS {
            store.connection().unwrap().execute(
                "INSERT INTO task_messages (id,task_id,sender,recipient,body) VALUES (?1,?2,'operator','queen','Fictional overflow')",
                rusqlite::params![format!("unavailable-{index}"), input.task_id.to_string()],
            ).unwrap();
        }
        assert!(store.queen_run_review_coverage(&input.run_id).is_err());
        assert_eq!(
            store
                .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::Incomplete, 101)
                .unwrap(),
            crate::QueenAutomationFinish::Closed(QueenAutomationOutcome::Incomplete)
        );
        assert_eq!(
            store.get_task(input.task_id).unwrap().state,
            TaskState::Blocked
        );
    }

    #[test]
    fn unchanged_reads_are_stable_but_same_second_corrections_invalidate() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Fixture", "/workspace/demo").unwrap();
        let before = store.queen_task_review_evidence(task.id).unwrap();
        assert_eq!(before, store.queen_task_review_evidence(task.id).unwrap());
        store
            .append_task_correction(task.id, "New evidence", &TaskActivityActor::operator())
            .unwrap();
        assert_ne!(before, store.queen_task_review_evidence(task.id).unwrap());
    }

    #[test]
    fn dependency_completion_invalidates_without_editing_the_consumer() {
        let store = TaskStore::in_memory().unwrap();
        let consumer = store.create_task("Consumer", "/workspace/demo").unwrap();
        let prerequisite = store
            .create_task("Prerequisite", "/workspace/demo")
            .unwrap();
        for task in [consumer.id, prerequisite.id] {
            store.transition_task(task, TaskState::Ready).unwrap();
        }
        store
            .transition_task(consumer.id, TaskState::Blocked)
            .unwrap();
        store
            .add_task_prerequisite(
                consumer.id,
                prerequisite.id,
                "Contract first",
                &TaskActivityActor::operator(),
                100,
            )
            .unwrap();
        let before = store.queen_task_review_evidence(consumer.id).unwrap();
        let updated_at = store.get_task(consumer.id).unwrap().updated_at;
        for state in [TaskState::Active, TaskState::Review, TaskState::Completed] {
            store.transition_task(prerequisite.id, state).unwrap();
        }
        assert_eq!(updated_at, store.get_task(consumer.id).unwrap().updated_at);
        assert_ne!(
            before,
            store.queen_task_review_evidence(consumer.id).unwrap()
        );
    }

    #[test]
    fn missing_task_is_not_an_empty_successful_snapshot() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.queen_task_review_evidence(TaskId::new()),
            Err(TaskStoreError::NotFound)
        ));
    }

    #[test]
    fn source_overflow_refuses_instead_of_hashing_a_partial_history() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Bounded source", "/workspace/demo")
            .unwrap();
        for index in 0..=MAX_REVIEW_SOURCE_ROWS {
            store.connection().unwrap().execute(
                "INSERT INTO task_messages (id, task_id, sender, recipient, body) VALUES (?1, ?2, 'operator', 'queen', 'Fictional evidence')",
                rusqlite::params![format!("fixture-{index}"), task.id.to_string()],
            ).unwrap();
        }
        assert!(matches!(
            store.queen_task_review_evidence(task.id),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
    }

    #[test]
    fn changed_decision_contents_invalidate_with_the_same_pending_count() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let task = store
            .create_task("Decision evidence", "/workspace/demo")
            .unwrap();
        store.connection().unwrap().execute(
            "INSERT INTO decision_requests (id, hive_id, requesting_worker_id, task_id, kind, urgency, title, reason, risk, evidence, suggested_action, allowed_actions)
             VALUES ('fixture-decision', ?1, ?2, ?3, 'input', 'normal', 'Fictional question', 'Original reason', '', '', '', '[]')",
            rusqlite::params![task.hive_id.to_string(), queen.id.to_string(), task.id.to_string()],
        ).unwrap();
        let before = store.queen_task_review_evidence(task.id).unwrap();
        store.connection().unwrap().execute("UPDATE decision_requests SET reason = 'Corrected reason' WHERE id = 'fixture-decision'", []).unwrap();
        assert_ne!(before, store.queen_task_review_evidence(task.id).unwrap());
    }

    #[test]
    fn delivery_state_changes_invalidate_without_a_task_transition() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Delivery evidence", "/workspace/demo")
            .unwrap();
        store.connection().unwrap().execute(
            "INSERT INTO task_messages (id, task_id, sender, recipient, body) VALUES ('fixture-message', ?1, 'operator', 'queen', 'Fictional evidence')",
            [task.id.to_string()],
        ).unwrap();
        store.connection().unwrap().execute("INSERT INTO task_message_deliveries (message_id, state, updated_at) VALUES ('fixture-message', 'queued', 100)", []).unwrap();
        let before = store.queen_task_review_evidence(task.id).unwrap();
        store.connection().unwrap().execute("UPDATE task_message_deliveries SET state = 'uncertain' WHERE message_id = 'fixture-message'", []).unwrap();
        assert_ne!(before, store.queen_task_review_evidence(task.id).unwrap());
        assert_eq!(store.get_task(task.id).unwrap().state, TaskState::Draft);
    }
}
