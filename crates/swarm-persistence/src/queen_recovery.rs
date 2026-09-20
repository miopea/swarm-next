//! Recovery identity comes from the live attention predicate and one task snapshot.
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    QueenRecoveryAssessment, QueenRecoveryFacts, QueenRecoveryIdentity, QueenRecoveryRecord,
    TaskActivityActor, TaskId, WorkerId, WorkerSessionId, assess_queen_recovery,
};

use crate::{TaskStore, TaskStoreError};

pub(super) fn migrate(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_recovery_receipts (
        task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
        run_id TEXT NOT NULL, attention_id TEXT NOT NULL,
        worker_id TEXT NOT NULL REFERENCES worker_profiles(id), session_id TEXT NOT NULL,
        accepted_revision TEXT NOT NULL CHECK(length(accepted_revision)=64),
        terminal_revision TEXT CHECK(terminal_revision IS NULL OR length(terminal_revision)=64),
        input_payload TEXT NOT NULL CHECK(length(input_payload)<=8192),
        recorded_at INTEGER NOT NULL,
        times_seen INTEGER NOT NULL DEFAULT 1,
        first_seen_at INTEGER,
        recorded_sequence INTEGER
    );",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::QUEEN_RECOVERY_RECEIPTS_SCHEMA_VERSION,
    )
}

/// How many runs have reached the same verdict about a worker that has not moved.
///
/// ⚠️ THIS COUNT IS A SYMPTOM REPORT, NOT A COVERAGE BOUND, and that is the one
/// way it differs from `RepeatedReview`. On the review path the equivalent count
/// is allowed to certify a wait once it stops changing, because a park is
/// SUPPOSED to sit still. Here the obligations are `stale_owned_work_attention`
/// and `assigned_ready_work_not_started_attention` -- a worker that is not
/// moving -- so "seen six times without a state change" is the failure itself.
/// Letting it cover the obligation would teach Queen to stop looking at exactly
/// the worker most likely to be stuck.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepeatedRecovery {
    pub task_id: String,
    pub title: String,
    pub state: String,
    pub worker_id: String,
    pub times_seen: i64,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

/// Count the repetition that was previously overwritten on every pass.
///
/// `first_seen_at` rather than a bare counter, for the same reason schema 179
/// added it to the review receipt: a count without a start is a number nobody
/// can weigh. `recorded_sequence` anchors "has this task moved", which is the
/// only reading of the count that survives both a task that never changes and
/// one that converts and comes back.
pub(super) fn migrate_recovery_repetition(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    for (column, definition) in [
        ("times_seen", "INTEGER NOT NULL DEFAULT 1"),
        ("first_seen_at", "INTEGER"),
        ("recorded_sequence", "INTEGER"),
    ] {
        let present: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('queen_recovery_receipts')
             WHERE name = ?1",
            [column],
            |row| row.get(0),
        )?;
        if present == 0 {
            transaction.execute_batch(&format!(
                "ALTER TABLE queen_recovery_receipts ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    // Existing rows have been seen at least once, at the time they were last
    // recorded. Backfilling the start from that says "no earlier repetition is
    // known", not "this began now".
    transaction.execute_batch(
        "UPDATE queen_recovery_receipts SET first_seen_at = recorded_at
         WHERE first_seen_at IS NULL",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::RECOVERY_REPETITION_SCHEMA_VERSION,
    )
}

/// Has this task had a state change since the recovery receipt was written.
///
/// The same anchor schema 179 settled on for reviews, and for the same reason:
/// `accepted_revision` cannot work because recording an assessment WRITES an
/// activity row, so no two passes ever match, and the task's state alone cannot
/// tell a task that never moved from one that moved away and came back.
fn moved_since_last_recovery(
    transaction: &rusqlite::Transaction<'_>,
    task_id: &str,
) -> Result<bool, TaskStoreError> {
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_activity a
             WHERE a.task_id = ?1 AND a.kind = 'state_changed'
               AND a.sequence > COALESCE(
                   (SELECT recorded_sequence FROM queen_recovery_receipts WHERE task_id = ?1),
                   0))",
        [task_id],
        |row| row.get(0),
    )?)
}

impl TaskStore {
    /// Recovery verdicts that keep being re-reached about work that has not moved.
    ///
    /// Reporting only. Nothing here covers an obligation or resumes anything.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn recoveries_repeating_without_progress(
        &self,
        at_least: i64,
    ) -> Result<Vec<RepeatedRecovery>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT r.task_id, t.title, t.state, r.worker_id, r.times_seen,
                    COALESCE(r.first_seen_at, r.recorded_at), r.recorded_at
             FROM queen_recovery_receipts r
             JOIN tasks t ON t.id = r.task_id AND t.removed_at IS NULL
             WHERE r.times_seen >= ?1
               AND t.state NOT IN ('completed', 'abandoned')
             ORDER BY r.times_seen DESC, r.first_seen_at LIMIT 64",
        )?;
        let rows = statement.query_map([at_least], |row| {
            Ok(RepeatedRecovery {
                task_id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                worker_id: row.get(3)?,
                times_seen: row.get(4)?,
                first_seen_at: row.get(5)?,
                last_seen_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Read the same live recovery obligations Queen sees, with exact transport
    /// evidence, in one database snapshot. No terminal reads or writes occur.
    ///
    /// # Errors
    /// Propagates corrupt stored evidence instead of presenting an empty queue.
    pub fn recovery_queue_snapshot(
        &self,
        observations: &[swarm_domain::RecoveryQueueObservation],
        now: i64,
    ) -> Result<swarm_domain::RecoveryQueueSnapshot, TaskStoreError> {
        use swarm_domain::{RecoveryQueueDelivery, RecoveryQueueItem, RecoveryQueueSnapshot};
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT action.id, task.id, worker.id, session.session_id,
                    task.updated_at, action.finished_at, action.reason,
                    EXISTS(SELECT 1 FROM worker_engagements e WHERE e.worker_id=worker.id AND e.expires_at>?1),
                    (SELECT json_object('message_id',m.id,'state',d.state,'updated_at',d.updated_at)
                     FROM task_messages m JOIN task_message_deliveries d ON d.message_id=m.id
                     WHERE m.task_id=task.id AND m.sender='queen' AND m.recipient_worker_id=worker.id
                       AND m.created_at>=action.finished_at AND d.superseded=0
                       AND d.state IN ('queued','dispatching','delivered','uncertain','rejected')
                       AND (d.state='queued' OR (d.state='delivered' AND m.delivered_session_id=session.session_id)
                            OR (d.state IN ('dispatching','uncertain','rejected') AND d.session_id=session.session_id))
                     ORDER BY m.created_at DESC,m.id DESC LIMIT 1),
                    (SELECT r.input_payload FROM queen_recovery_receipts r
                     WHERE r.task_id=task.id AND r.attention_id=action.id
                       AND r.worker_id=worker.id AND r.session_id=session.session_id)
             {} AND action.kind IN ('stale_owned_work_attention',
                 'assigned_ready_work_not_started_attention','owned_work_never_briefed_attention')
             ORDER BY action.finished_at DESC,action.id DESC LIMIT 33",
            crate::coordinator::LIVE_ATTENTION_SOURCE
        ))?;
        let rows = statement
            .query_map([now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut snapshot = RecoveryQueueSnapshot {
            items: Vec::new(),
            truncated: rows.len() > 32,
        };
        for (
            attention_id,
            task_id,
            worker_id,
            session_id,
            task_revision,
            observed_at,
            reason,
            engaged,
            delivery,
            assessment,
        ) in rows.into_iter().take(32)
        {
            let session_id = session_id
                .parse()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let delivery: Option<RecoveryQueueDelivery> = delivery
                .map(|raw| serde_json::from_str(&raw))
                .transpose()
                .map_err(|_| {
                    TaskStoreError::IntegrityFailure("invalid recovery queue delivery".into())
                })?;
            let observation = observations
                .iter()
                .find(|item| item.session_id == session_id)
                .copied();
            let Some(state) = swarm_domain::recovery_queue_state(
                session_id,
                observation,
                engaged,
                delivery.as_ref().map(|item| item.state.as_str()),
            ) else {
                continue;
            };
            snapshot.items.push(RecoveryQueueItem {
                attention_id,
                task_id: task_id.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
                worker_id: worker_id
                    .parse()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                session_id,
                task_revision,
                observed_at,
                reason,
                state,
                delivery,
                last_assessment: assessment
                    .map(|raw| read_saved_recovery(&raw))
                    .transpose()?,
            });
        }
        Ok(snapshot)
    }

    /// Evaluate all current recovery obligations against freshly obtained facts.
    /// A partial observation pass cannot certify complete recovery coverage.
    ///
    /// # Errors
    /// Propagates source/receipt corruption rather than declaring the fleet clear.
    pub fn queen_recovery_coverage(
        &self,
        run_id: &str,
        observations: &[QueenRecoveryFacts],
        complete: bool,
        now: i64,
    ) -> Result<swarm_domain::QueenReviewCoverage, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let result = recovery_coverage(&tx, run_id, observations, complete, now)?;
        tx.commit()?;
        Ok(result)
    }

    /// Obtain a current recovery identity without certifying terminal health.
    ///
    /// # Errors
    /// Refuses source overflow, invalid stored identities and unavailable storage.
    pub fn queen_recovery_identity(
        &self,
        attention_id: &str,
    ) -> Result<Option<QueenRecoveryIdentity>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let identity = recovery_identity(&transaction, attention_id)?;
        transaction.commit()?;
        Ok(identity)
    }

    /// Save a bounded, fenced assessment, not a task transition or authorization.
    /// Terminal facts must be freshly obtained by the application, never parsed
    /// from the agent's request. Durable facts are re-derived inside this write.
    ///
    /// # Errors
    /// Refuses stale identities, invalid sources, uncovered claims and capacity.
    pub fn record_queen_recovery(
        &self,
        input: &QueenRecoveryRecord,
        observed: &QueenRecoveryFacts,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<QueenRecoveryIdentity, TaskStoreError> {
        input.validate().map_err(refused)?;
        let payload = serde_json::to_string(input)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        if payload.len() > 8192 {
            return Err(refused("recovery assessment exceeds its payload bound"));
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        crate::task_prerequisites::authorize(&tx, actor)?;
        let mut current = recovery_identity(&tx, &input.identity.attention_id)?
            .ok_or_else(|| refused("recovery obligation changed; read current attention"))?;
        current
            .terminal_revision
            .clone_from(&observed.identity.terminal_revision);
        let mut facts = observed.clone();
        refresh_durable_facts(&tx, &mut facts, now)?;
        // The authenticated command explicitly records a checked judgment for
        // this run. This is not a boolean supplied by an observation or proof
        // that work completed. Validation requires condition, evidence, source.
        facts.verified_external_wait = matches!(
            input.disposition,
            swarm_domain::QueenRecoveryDisposition::VerifiedExternalWait { .. }
        );
        let replay: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_recovery_receipts WHERE task_id=?1
             AND input_payload=?2 AND accepted_revision=?3 AND terminal_revision IS ?4)",
            params![
                current.task_id.to_string(),
                payload,
                current.evidence_revision,
                current.terminal_revision
            ],
            |row| row.get(0),
        )?;
        if replay
            && observed.identity == current
            && assess_queen_recovery(&current, &facts, &input.disposition)
                != QueenRecoveryAssessment::Uncovered
        {
            return Ok(current);
        }
        if current != input.identity || observed.identity != current {
            return Err(refused(
                "task, session or terminal evidence changed; observe again",
            ));
        }
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_automation WHERE id=1 AND run_id=?1
             AND delivery_session_id IS NOT NULL
             AND (state IN ('running','uncertain')
                 OR (state IN ('queued','delivering') AND delivered_at IS NOT NULL)))",
            [&input.run_id],
            |row| row.get(0),
        )?;
        if !active {
            return Err(refused(
                "recovery assessment requires the current unfinished delivered review",
            ));
        }
        // External conditions remain explicit authenticated judgments, not
        // machine-verified facts or new authority to resume a task.
        if assess_queen_recovery(&input.identity, &facts, &input.disposition)
            == QueenRecoveryAssessment::Uncovered
        {
            return Err(refused(input.disposition.evidence_requirement()));
        }
        tx.execute("DELETE FROM queen_recovery_receipts WHERE task_id IN
            (SELECT id FROM tasks WHERE removed_at IS NOT NULL OR state IN ('completed','abandoned'))", [])?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM queen_recovery_receipts WHERE task_id!=?1",
            [current.task_id.to_string()],
            |row| row.get(0),
        )?;
        if count >= 256 {
            return Err(refused(
                "recovery receipt capacity reached; coverage is incomplete",
            ));
        }
        // Read BEFORE the activity row below, because that row is itself a
        // sequence past the saved anchor and would make every pass look moved.
        let moved = moved_since_last_recovery(&tx, &current.task_id.to_string())?;
        tx.execute(
            "INSERT INTO task_activity (task_id,kind,note,actor_kind,actor_id,occurred_at)
            VALUES (?1,'corrected',?2,?3,?4,?5)",
            params![
                current.task_id.to_string(),
                recovery_audit_note(input),
                actor.kind.to_string(),
                actor.id,
                now
            ],
        )?;
        let recorded_sequence = tx.last_insert_rowid();
        current.evidence_revision =
            crate::queen_review::task_review_evidence(&tx, current.task_id)?.evidence_revision;
        write_recovery_receipt(
            &tx,
            &current,
            &input.run_id,
            &payload,
            now,
            recorded_sequence,
            moved,
        )?;
        crate::insert_control_room_event(&tx, swarm_domain::ControlRoomEventKind::TasksChanged)?;
        tx.commit()?;
        Ok(current)
    }
}

/// Store the verdict and advance its repetition count in one statement.
///
/// Lifted out of `record_queen_recovery` only to keep that function under the
/// line bound; it has no caller anywhere else and no behaviour of its own.
fn write_recovery_receipt(
    tx: &rusqlite::Transaction<'_>,
    current: &QueenRecoveryIdentity,
    run_id: &str,
    payload: &str,
    now: i64,
    recorded_sequence: i64,
    moved: bool,
) -> Result<(), TaskStoreError> {
    // ⚠️ RESET ON MOVEMENT, INCREMENT OTHERWISE. The count only means anything
    // as "re-derived this many times while the work stood still".

    tx.execute("INSERT INTO queen_recovery_receipts
        (task_id,run_id,attention_id,worker_id,session_id,accepted_revision,terminal_revision,input_payload,recorded_at,times_seen,first_seen_at,recorded_sequence)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?9,?10) ON CONFLICT(task_id) DO UPDATE SET
        run_id=excluded.run_id,attention_id=excluded.attention_id,worker_id=excluded.worker_id,
        session_id=excluded.session_id,accepted_revision=excluded.accepted_revision,
        terminal_revision=excluded.terminal_revision,input_payload=excluded.input_payload,recorded_at=excluded.recorded_at,
        recorded_sequence=excluded.recorded_sequence,
        times_seen = CASE WHEN ?11 THEN 1 ELSE queen_recovery_receipts.times_seen + 1 END,
        first_seen_at = CASE WHEN ?11 THEN excluded.recorded_at
            ELSE COALESCE(queen_recovery_receipts.first_seen_at, excluded.recorded_at) END",
        params![current.task_id.to_string(), run_id, current.attention_id, current.worker_id.to_string(),
            current.session_id.to_string(), current.evidence_revision, current.terminal_revision, payload, now,
            recorded_sequence, moved])?;
    Ok(())
}

fn recovery_audit_note(input: &QueenRecoveryRecord) -> String {
    let judgment = match &input.disposition {
        swarm_domain::QueenRecoveryDisposition::VerifiedExternalWait { checked_evidence } => {
            format!(
                "\nExternal-condition judgment (not completion or authorization): {checked_evidence}"
            )
        }
        _ => String::new(),
    };
    format!(
        "Queen recovery assessment: {}\nChecked source: {}{judgment}",
        input.reason, input.source
    )
}

fn refused(reason: &str) -> TaskStoreError {
    TaskStoreError::RecoveryAssessmentRefused(reason.into())
}

fn read_saved_recovery(payload: &str) -> Result<QueenRecoveryRecord, TaskStoreError> {
    let saved: QueenRecoveryRecord = serde_json::from_str(payload).map_err(|_| {
        TaskStoreError::IntegrityFailure("stored recovery receipt is unreadable".into())
    })?;
    saved
        .validate()
        .map_err(|reason| TaskStoreError::IntegrityFailure(reason.into()))?;
    Ok(saved)
}

/// ⚠️ COVERAGE NO LONGER DEPENDS ON THE RUN, and `_run_id` is kept only because
/// every caller still legitimately names the run it is asking about -- the same
/// shape `review_coverage` took when its run fence was removed on 2026-09-18.
pub(super) fn recovery_coverage(
    connection: &Connection,
    _run_id: &str,
    observations: &[QueenRecoveryFacts],
    complete: bool,
    now: i64,
) -> Result<swarm_domain::QueenReviewCoverage, TaskStoreError> {
    use swarm_domain::{QueenRecoveryDisposition, QueenReviewCoverage};
    if !complete || observations.len() > 256 {
        return Ok(QueenReviewCoverage::Unavailable);
    }
    let mut observed = std::collections::HashMap::new();
    for facts in observations {
        if observed
            .insert(facts.identity.attention_id.as_str(), facts)
            .is_some()
        {
            return Ok(QueenReviewCoverage::Unavailable);
        }
    }
    let ids = {
        let mut statement = connection.prepare(&format!("SELECT action.id {} AND action.kind IN
            ('stale_owned_work_attention','assigned_ready_work_not_started_attention','owned_work_never_briefed_attention')
            ORDER BY action.id LIMIT 257", crate::coordinator::LIVE_ATTENTION_SOURCE))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    if ids.len() > 256 {
        return Ok(QueenReviewCoverage::Unavailable);
    }
    let mut missing = Vec::new();
    let mut waiting = 0;
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        let Some(mut current) = recovery_identity(connection, &id)? else {
            continue;
        };
        if !seen.insert(current.task_id) {
            return Ok(QueenReviewCoverage::Unavailable);
        }
        let Some(facts) = observed.get(id.as_str()) else {
            missing.push(current.task_id);
            continue;
        };
        current
            .terminal_revision
            .clone_from(&facts.identity.terminal_revision);
        let mut fresh = (*facts).clone();
        refresh_durable_facts(connection, &mut fresh, now)?;
        fresh.verified_external_wait = false;
        // Current machine evidence can settle a historical stall observation;
        // do not require Queen to approve a busy worker or genuine protected input.
        let mut dispositions = vec![
            QueenRecoveryDisposition::ObservedWorking,
            QueenRecoveryDisposition::ProtectOperatorInput,
        ];
        if let Some(message_id) = &fresh.pending_message_id {
            dispositions.push(QueenRecoveryDisposition::AwaitDelivery {
                message_id: message_id.clone(),
            });
        }
        if let Some(decision_id) = fresh.pending_decision_id {
            dispositions.push(QueenRecoveryDisposition::AwaitOperator { decision_id });
        }
        let receipt: Option<String> = connection
            .query_row(
                // ⚠️ NO `run_id` HERE, AND THAT IS THE FIX. This clause used to
                // read `AND run_id=?2`, which is the same defect removed from
                // review coverage on 2026-09-18: a receipt counted only inside
                // the run that wrote it, so the next run found the obligation
                // uncovered and the conductor forced Incomplete.
                //
                // Measured on this Hive 2026-09-20: 2109 runs before that review
                // fix, 2108 of them Incomplete. Afterwards the review gate went
                // healthy and 54 of 75 runs were STILL Incomplete, because this
                // gate is checked with `||` beside it. Across all 2184 runs the
                // Hive had recorded FOUR recovery receipts, each under a
                // different run_id, so not one of them could ever cover
                // anything.
                //
                // ⚠️ WHAT STILL UNCOVERS IT, and this is why dropping the run is
                // safe where dropping the revisions would not be. The verdict
                // holds only while the task evidence AND the terminal output are
                // both byte-identical. A worker that writes one line, or a task
                // that gains any activity, uncovers immediately and returns to
                // Queen. The run was never what made this judgment current;
                // these two revisions are.
                //
                // ⚠️ NO BOUND ON THIS PATH. `times_seen` is counted and reported
                // through `recoveries_repeating_without_progress`, but it never
                // certifies coverage the way the review path lets it. A recovery
                // obligation means a worker is not moving, so a count that rises
                // while nothing changes is the symptom, and covering it would
                // hide the stall instead of ending it.
                "SELECT input_payload FROM queen_recovery_receipts
            WHERE task_id=?1 AND attention_id=?2 AND worker_id=?3 AND session_id=?4
            AND accepted_revision=?5 AND terminal_revision IS ?6",
                params![
                    current.task_id.to_string(),
                    current.attention_id,
                    current.worker_id.to_string(),
                    current.session_id.to_string(),
                    current.evidence_revision,
                    current.terminal_revision
                ],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(payload) = receipt {
            let saved = read_saved_recovery(&payload)?;
            // The query fences this judgment to the current task, session and
            // terminal identity. It carries across runs while all three are
            // unchanged, and stops the moment any of them moves.
            fresh.verified_external_wait = matches!(
                saved.disposition,
                QueenRecoveryDisposition::VerifiedExternalWait { .. }
            );
            dispositions.push(saved.disposition);
        }
        let assessment = dispositions
            .iter()
            .map(|disposition| assess_queen_recovery(&current, &fresh, disposition))
            .find(|result| *result != QueenRecoveryAssessment::Uncovered);
        match assessment {
            Some(QueenRecoveryAssessment::Working) => {}
            Some(QueenRecoveryAssessment::Waiting) => waiting += 1,
            _ => missing.push(current.task_id),
        }
    }
    Ok(if missing.is_empty() {
        QueenReviewCoverage::Covered {
            waiting_obligations: waiting,
        }
    } else {
        QueenReviewCoverage::Missing { task_ids: missing }
    })
}

fn refresh_durable_facts(
    connection: &Connection,
    facts: &mut QueenRecoveryFacts,
    now: i64,
) -> Result<(), TaskStoreError> {
    facts.operator_engaged = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_engagements
        WHERE worker_id=?1 AND expires_at>?2)",
        params![facts.identity.worker_id.to_string(), now],
        |row| row.get(0),
    )?;
    facts.pending_message_id = connection
        .query_row(
            "SELECT m.id FROM task_messages m
        JOIN task_message_deliveries d ON d.message_id=m.id
        WHERE m.task_id=?1 AND m.sender='queen' AND m.recipient_worker_id=?2
          AND d.state IN ('queued','dispatching') AND d.superseded=0
          AND (d.state='queued' OR d.session_id=?3)
        ORDER BY m.created_at DESC,m.id DESC LIMIT 1",
            params![
                facts.identity.task_id.to_string(),
                facts.identity.worker_id.to_string(),
                facts.identity.session_id.to_string()
            ],
            |row| row.get(0),
        )
        .optional()?;
    let decision: Option<String> = connection
        .query_row(
            "SELECT id FROM decision_requests
        WHERE id IN (SELECT decision_id FROM task_decision_membership WHERE task_id=?1)
          AND state='pending' ORDER BY id LIMIT 1",
            [facts.identity.task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    facts.pending_decision_id = decision
        .map(|id| {
            id.parse().map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid recovery decision identity".into())
            })
        })
        .transpose()?;
    Ok(())
}

fn recovery_identity(
    connection: &Connection,
    attention_id: &str,
) -> Result<Option<QueenRecoveryIdentity>, TaskStoreError> {
    let identities: Option<(String, String, String)> = connection
        .query_row(
            &format!(
                "SELECT task.id, worker.id, session.session_id {} AND action.id=?1
                 AND action.kind IN ('stale_owned_work_attention',
                     'assigned_ready_work_not_started_attention',
                     'owned_work_never_briefed_attention')",
                crate::coordinator::LIVE_ATTENTION_SOURCE,
            ),
            [attention_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((task, worker, session)) = identities else {
        return Ok(None);
    };
    let invalid = || TaskStoreError::IntegrityFailure("invalid stored recovery identity".into());
    let task_id = TaskId::from_str(&task).map_err(|_| invalid())?;
    let identity = QueenRecoveryIdentity {
        task_id,
        worker_id: WorkerId::from_str(&worker).map_err(|_| invalid())?,
        session_id: WorkerSessionId::from_str(&session).map_err(|_| invalid())?,
        attention_id: attention_id.to_owned(),
        terminal_revision: None,
        evidence_revision: crate::queen_review::task_review_evidence(connection, task_id)?
            .evidence_revision,
    };
    Ok(Some(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        ProviderKind, QueenRecoveryDisposition, RecoveryTerminalActivity, TaskState,
    };
    const NOW: i64 = 4_000_000_000;

    #[test]
    fn recovery_queue_reports_overflow_instead_of_certifying_a_partial_fleet() {
        let store = TaskStore::in_memory().unwrap();
        for index in 0..33 {
            let workspace = format!("/workspace/bounded-{index}");
            let worker = store
                .create_worker(
                    &format!("Bounded {index}"),
                    ProviderKind::ClaudeCode,
                    &workspace,
                    false,
                    1,
                )
                .unwrap();
            store
                .bind_worker_session(worker.id, WorkerSessionId::new())
                .unwrap();
            let task = store
                .create_task("Unfinished fictional work", &workspace)
                .unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store
                .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
                .unwrap();
            store.transition_task(task.id, TaskState::Active).unwrap();
            let candidate = store
                .stale_owned_work_candidates(NOW, 600)
                .unwrap()
                .into_iter()
                .find(|candidate| candidate.task_id == task.id)
                .unwrap();
            store
                .record_stale_owned_work_attention(
                    &candidate,
                    NOW,
                    600,
                    crate::BackgroundWorkReading::NoneVisible,
                )
                .unwrap();
        }
        let snapshot = store.recovery_queue_snapshot(&[], NOW).unwrap();
        assert_eq!(snapshot.items.len(), 32);
        assert!(snapshot.truncated);
        assert!(
            snapshot
                .items
                .iter()
                .all(|item| item.state == swarm_domain::RecoveryQueueState::ObservationUnavailable)
        );
    }

    #[test]
    fn recovery_queue_tracks_exact_delivery_without_settling_the_task() {
        use swarm_domain::{RecoveryQueueObservation, RecoveryQueueState};
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        let id = input.identity;
        let observations = [RecoveryQueueObservation {
            session_id: id.session_id,
            activity: RecoveryTerminalActivity::Resting,
            background_work: false,
        }];
        let read = |now| store.recovery_queue_snapshot(&observations, now).unwrap();
        assert_eq!(
            read(NOW).items[0].state,
            RecoveryQueueState::NeedsQueenCheck
        );
        assert_eq!(
            store.recovery_queue_snapshot(&[], NOW).unwrap().items[0].state,
            RecoveryQueueState::ObservationUnavailable
        );
        let message = store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Continue this fictional task",
                NOW + 1,
            )
            .unwrap();
        let queued = read(NOW + 1);
        assert_eq!(queued.items[0].state, RecoveryQueueState::AwaitingDelivery);
        assert_eq!(
            queued.items[0].delivery.as_ref().unwrap().message_id,
            message.id
        );
        store
            .mark_task_message_delivered(&message.id, id.session_id, NOW + 2)
            .unwrap();
        assert_eq!(
            read(NOW + 2).items[0].state,
            RecoveryQueueState::VerifyWorkerResponse
        );
        assert_eq!(store.get_task(id.task_id).unwrap().state, TaskState::Active);
        store
            .renew_worker_engagement(id.session_id, None, NOW + 3, 60)
            .unwrap();
        assert!(read(NOW + 4).items.is_empty());
        assert_eq!(read(NOW + 64).items.len(), 1);
        let working = [RecoveryQueueObservation {
            activity: RecoveryTerminalActivity::Working,
            ..observations[0]
        }];
        assert!(
            store
                .recovery_queue_snapshot(&working, NOW + 64)
                .unwrap()
                .items
                .is_empty()
        );
        store
            .transition_task(id.task_id, TaskState::Review)
            .unwrap();
        assert!(read(NOW + 65).items.is_empty());
    }

    #[test]
    fn recovery_queue_does_not_borrow_old_or_other_session_messages() {
        use swarm_domain::{RecoveryQueueObservation, RecoveryQueueState};
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        let id = input.identity;
        let observations = [RecoveryQueueObservation {
            session_id: id.session_id,
            activity: RecoveryTerminalActivity::Resting,
            background_work: false,
        }];
        store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Older request",
                NOW - 1,
            )
            .unwrap();
        assert!(
            store
                .recovery_queue_snapshot(&observations, NOW)
                .unwrap()
                .items[0]
                .delivery
                .is_none()
        );
        let message = store
            .send_task_message(
                id.task_id,
                crate::MessageEnd::queen(),
                crate::MessageEnd::worker(id.worker_id),
                "Different session",
                NOW + 1,
            )
            .unwrap();
        store
            .mark_task_message_delivered(&message.id, WorkerSessionId::new(), NOW + 2)
            .unwrap();
        let snapshot = store
            .recovery_queue_snapshot(&observations, NOW + 2)
            .unwrap();
        assert_eq!(snapshot.items[0].state, RecoveryQueueState::NeedsQueenCheck);
        assert!(snapshot.items[0].delivery.is_none());
        assert!(!snapshot.truncated);
    }

    fn fixture(store: &TaskStore) -> (QueenRecoveryRecord, QueenRecoveryFacts) {
        let worker = store
            .create_worker(
                "Recovery fixture",
                ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store
            .create_task("Recovery fixture task", "/workspace/demo")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        let candidate = store
            .stale_owned_work_candidates(NOW, 600)
            .unwrap()
            .pop()
            .unwrap();
        store
            .record_stale_owned_work_attention(
                &candidate,
                NOW,
                600,
                crate::BackgroundWorkReading::NoneVisible,
            )
            .unwrap();
        let attention = store
            .current_coordinator_attention(NOW)
            .unwrap()
            .into_iter()
            .find(|a| a.task_id == Some(task.id))
            .unwrap();
        let mut identity = store
            .queen_recovery_identity(&attention.action_id)
            .unwrap()
            .unwrap();
        identity.terminal_revision = Some("b".repeat(64));
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(NOW).unwrap();
        let run = store.claim_queen_automation(NOW).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, NOW)
            .unwrap();
        let facts = QueenRecoveryFacts {
            identity: identity.clone(),
            terminal_is_current: true,
            activity: RecoveryTerminalActivity::Working,
            operator_engaged: false,
            unsent_input: Some(false),
            pending_message_id: None,
            pending_decision_id: None,
            verified_external_wait: false,
        };
        (
            QueenRecoveryRecord {
                run_id: run.run_id,
                identity,
                disposition: QueenRecoveryDisposition::ObservedWorking,
                reason: "Current canonical terminal is working".into(),
                source: "Isolated trusted terminal observation fixture".into(),
            },
            facts,
        )
    }

    #[test]
    fn recovery_receipt_accepts_delivered_continuations_only() {
        for (state, delivered, accepted) in [
            ("queued", true, true),
            ("delivering", true, true),
            ("queued", false, false),
            ("delivering", false, false),
            ("completed", true, false),
        ] {
            let store = TaskStore::in_memory().unwrap();
            let (input, facts) = fixture(&store);
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE queen_automation SET state=?1,
                 delivered_at=CASE WHEN ?2 THEN delivered_at ELSE NULL END WHERE id=1",
                    params![state, delivered],
                )
                .unwrap();
            let result =
                store.record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW);
            assert_eq!(
                result.is_ok(),
                accepted,
                "{state}, delivered={delivered}: {result:?}"
            );
        }
    }

    #[test]
    fn external_judgment_is_checked_bounded_and_valid_for_its_evidence_across_runs() {
        let directory = tempfile::tempdir().unwrap();
        let store = TaskStore::open(directory.path().join("external.sqlite3")).unwrap();
        let (mut input, mut facts) = fixture(&store);
        facts.activity = RecoveryTerminalActivity::Resting;
        input.reason = "Fictional upstream endpoint is unavailable".into();
        input.source = "Fictional endpoint status response".into();
        input.disposition = QueenRecoveryDisposition::VerifiedExternalWait {
            checked_evidence: " ".into(),
        };
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        input.disposition = QueenRecoveryDisposition::VerifiedExternalWait {
            checked_evidence: "Checked during this run: fixture returned service unavailable"
                .into(),
        };
        assert!(
            store
                .record_queen_recovery(
                    &input,
                    &facts,
                    &TaskActivityActor::worker(input.identity.worker_id),
                    NOW
                )
                .is_err()
        );
        facts.identity = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        ));
        // ⚠️ THIS ASSERTION WAS INVERTED ON 2026-09-20, and the inversion is the
        // point rather than a loosening. It used to require Missing under any
        // other run, which is what made a verdict die with the run that reached
        // it. The evidence and terminal predicates below still fence it.
        assert!(matches!(
            store
                .queen_recovery_coverage(&TaskId::new().to_string(), &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        ));
        facts.identity.terminal_revision = Some("c".repeat(64));
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing { .. }
        ));
        assert_eq!(
            store.get_task(input.identity.task_id).unwrap().state,
            TaskState::Active
        );
    }

    #[test]
    fn receipt_survives_reopen_and_replays_without_another_activity_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let (input, facts) = fixture(&store);
        let saved = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        assert_ne!(saved.evidence_revision, input.identity.evidence_revision);
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        let mut current = facts;
        current.identity = saved.clone();
        assert_eq!(
            store
                .record_queen_recovery(&input, &current, &TaskActivityActor::operator(), NOW + 1)
                .unwrap(),
            saved
        );
        assert_eq!(
            store.get_task(saved.task_id).unwrap().state,
            TaskState::Active
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        current.identity.terminal_revision = Some("c".repeat(64));
        assert!(
            store
                .record_queen_recovery(&input, &current, &TaskActivityActor::operator(), NOW + 2)
                .is_err()
        );
    }

    #[test]
    fn worker_and_invented_pending_delivery_cannot_certify_recovery() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        assert!(
            store
                .record_queen_recovery(
                    &input,
                    &facts,
                    &TaskActivityActor::worker(input.identity.worker_id),
                    NOW
                )
                .is_err()
        );
        input.disposition = QueenRecoveryDisposition::AwaitDelivery {
            message_id: "invented".into(),
        };
        facts.pending_message_id = Some("invented".into());
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[test]
    fn completion_coverage_requires_fresh_complete_observations_not_a_saved_working_claim() {
        let store = TaskStore::in_memory().unwrap();
        let (input, mut facts) = fixture(&store);
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered { .. }
        ));
        let saved = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        facts.identity = saved;
        assert!(matches!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], false, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Unavailable
        ));
        facts.activity = RecoveryTerminalActivity::Resting;
        facts.identity.terminal_revision = Some("c".repeat(64));
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            }
        );
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            }
        );
    }

    #[test]
    fn finishing_without_observing_worker_recovery_closes_honestly_as_incomplete() {
        let store = TaskStore::in_memory().unwrap();
        let (input, _) = fixture(&store);
        assert!(matches!(
            store
                .finish_queen_automation_run(
                    &input.run_id,
                    swarm_domain::QueenAutomationOutcome::NoAction,
                    NOW
                )
                .unwrap(),
            crate::QueenAutomationFinish::Closed(swarm_domain::QueenAutomationOutcome::Incomplete)
        ));
    }

    #[test]
    fn finish_accepts_current_working_evidence_without_a_queen_approval_round_trip() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        assert!(matches!(
            store
                .finish_queen_automation_run_with_recovery(
                    &input.run_id,
                    swarm_domain::QueenAutomationOutcome::NoAction,
                    NOW,
                    &[facts],
                    true,
                    None
                )
                .unwrap(),
            crate::QueenAutomationFinish::Closed(swarm_domain::QueenAutomationOutcome::NoAction)
        ));
    }

    /// Facts on which NOTHING covers by default, so only a stored verdict can.
    ///
    /// `ObservedWorking` and `ProtectOperatorInput` are pushed on every pass
    /// regardless of any receipt, so a fixture left Working proves nothing about
    /// what the receipt carried: it would read Covered with the receipt deleted.
    fn needs_a_stored_verdict(facts: &mut QueenRecoveryFacts) {
        facts.activity = RecoveryTerminalActivity::Resting;
        facts.operator_engaged = false;
        facts.unsent_input = Some(false);
        facts.pending_message_id = None;
        facts.pending_decision_id = None;
    }

    fn external_wait(input: &mut QueenRecoveryRecord) {
        input.disposition = QueenRecoveryDisposition::VerifiedExternalWait {
            checked_evidence: "Fixture external dependency re-read this pass".into(),
        };
    }

    /// ⚠️ THE REGRESSION TEST FOR THE RUN FENCE. Recorded under one run, asked
    /// about under another. Before the fence came out this was Missing, which is
    /// what forced 54 of 75 runs to Incomplete after the review gate was healthy.
    #[test]
    fn a_recovery_verdict_survives_the_run_that_reached_it() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        needs_a_stored_verdict(&mut facts);
        external_wait(&mut input);
        facts.identity = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone()], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered {
                waiting_obligations: 1
            },
            "the run that recorded it must still be covered"
        );
        let later_run = format!("{}-later", input.run_id);
        assert_eq!(
            store
                .queen_recovery_coverage(&later_run, &[facts], true, NOW + 60)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Covered {
                waiting_obligations: 1
            },
            "a later run must not have to re-derive an unchanged verdict"
        );
    }

    /// The safety half: carrying across runs must not carry across a CHANGE.
    /// A worker that writes one line uncovers its own verdict immediately.
    #[test]
    fn a_moved_terminal_uncovers_the_carried_verdict() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        needs_a_stored_verdict(&mut facts);
        external_wait(&mut input);
        facts.identity = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
            .unwrap();
        let mut moved = facts.clone();
        moved.identity.terminal_revision = Some("c".repeat(64));
        assert_eq!(
            store
                .queen_recovery_coverage(&format!("{}-later", input.run_id), &[moved], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            },
            "a terminal that moved must return to Queen even in the same run"
        );
    }

    /// ⚠️ COUNTED, NEVER CERTIFYING, and this is the one way this path departs
    /// from `reviews_repeating`. A recovery obligation means a worker is NOT
    /// moving, so a count that rises while nothing changes is the symptom. It is
    /// reported so a stall stays visible; it must never become coverage.
    #[test]
    fn a_repeating_recovery_is_reported_and_still_never_covers() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        needs_a_stored_verdict(&mut facts);
        external_wait(&mut input);
        for pass in 0..8 {
            facts.identity = store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW + pass)
                .unwrap();
            input.identity = facts.identity.clone();
        }
        let repeating = store.recoveries_repeating_without_progress(6).unwrap();
        assert_eq!(repeating.len(), 1, "a standing stall must be reportable");
        assert_eq!(repeating[0].times_seen, 8);
        assert_eq!(repeating[0].first_seen_at, NOW);

        // Past the review path's bound of 6, and STILL uncovered once the
        // terminal moves. The review path would have accepted the current
        // revision here; this one must not.
        let mut moved = facts.clone();
        moved.identity.terminal_revision = Some("d".repeat(64));
        assert_eq!(
            store
                .queen_recovery_coverage("some-other-run", &[moved], true, NOW + 99)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Missing {
                task_ids: vec![input.identity.task_id]
            },
            "repetition must never certify a stall"
        );
    }

    /// The count means "re-derived this many times WITHOUT the work moving", so
    /// a real state change has to reset it or the number says nothing.
    ///
    /// ⚠️ ASSERTED ON THE ANCHOR RATHER THAN A FOURTH RECORDING, because a state
    /// change retires the attention the recording needs: `LIVE_ATTENTION_SOURCE`
    /// requires `task.updated_at = action.evidence_revision`, so moving the task
    /// removes the obligation entirely and there is nothing left to record
    /// against. The anchor is what the reset is made of, so it is what this
    /// pins.
    #[test]
    fn the_recovery_repeat_count_resets_when_the_task_moves() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        needs_a_stored_verdict(&mut facts);
        external_wait(&mut input);
        for pass in 0..3 {
            facts.identity = store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW + pass)
                .unwrap();
            input.identity = facts.identity.clone();
        }
        let task_id = input.identity.task_id.to_string();
        assert_eq!(
            store.recoveries_repeating_without_progress(3).unwrap()[0].times_seen,
            3,
            "three passes over work that never moved must count as three"
        );
        let mut connection = store.connection().unwrap();
        let tx = connection.transaction().unwrap();
        assert!(
            !moved_since_last_recovery(&tx, &task_id).unwrap(),
            "recording an assessment must not read as the work having moved"
        );
        drop(tx);
        drop(connection);

        store
            .transition_task(input.identity.task_id, TaskState::Blocked)
            .unwrap();
        let mut connection = store.connection().unwrap();
        let tx = connection.transaction().unwrap();
        assert!(
            moved_since_last_recovery(&tx, &task_id).unwrap(),
            "a real state change must reset the count on the next recording"
        );
    }

    #[test]
    fn duplicate_observations_cannot_certify_recovery_coverage() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        assert_eq!(
            store
                .queen_recovery_coverage(&input.run_id, &[facts.clone(), facts], true, NOW)
                .unwrap(),
            swarm_domain::QueenReviewCoverage::Unavailable
        );
    }

    #[test]
    fn delivered_request_invalidates_its_pending_delivery_assessment() {
        let store = TaskStore::in_memory().unwrap();
        let (mut input, mut facts) = fixture(&store);
        let message = store
            .send_queen_worker_message(
                input.identity.task_id,
                input.identity.worker_id,
                swarm_domain::WorkerMessagePurpose::AssignedTask,
                "Continue this fixture task",
                NOW,
            )
            .unwrap();
        input.identity = store
            .queen_recovery_identity(&input.identity.attention_id)
            .unwrap()
            .unwrap();
        facts.identity = input.identity.clone();
        input.disposition = QueenRecoveryDisposition::AwaitDelivery {
            message_id: message.id.clone(),
        };
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
        store
            .mark_task_message_delivered(&message.id, input.identity.session_id, NOW + 1)
            .unwrap();
        input.identity = store
            .queen_recovery_identity(&input.identity.attention_id)
            .unwrap()
            .unwrap();
        facts.identity = input.identity.clone();
        facts.pending_message_id = Some(message.id);
        let error = store
            .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW + 1)
            .unwrap_err();
        assert!(matches!(
            &error,
            TaskStoreError::RecoveryAssessmentRefused(_)
        ));
        assert!(
            error
                .to_string()
                .contains("A delivered message is not pending delivery")
        );
        assert!(!error.to_string().contains("database integrity"));
        assert_eq!(
            store.get_task(input.identity.task_id).unwrap().state,
            TaskState::Active
        );
    }

    #[test]
    fn corrupt_saved_receipts_remain_integrity_failures_not_command_refusals() {
        assert!(matches!(
            read_saved_recovery("not json"),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
        let store = TaskStore::in_memory().unwrap();
        let (mut input, _) = fixture(&store);
        input.run_id = "invalid stored run".into();
        assert!(matches!(
            read_saved_recovery(&serde_json::to_string(&input).unwrap()),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
    }

    #[test]
    fn capacity_refusal_is_atomic_and_retired_receipts_release_space() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        let mut retired = None;
        for index in 0..256 {
            let task = store
                .create_task(&format!("Capacity fixture {index}"), "/workspace/capacity")
                .unwrap();
            retired = Some(task.id);
            store.connection().unwrap().execute("INSERT INTO queen_recovery_receipts
                (task_id,run_id,attention_id,worker_id,session_id,accepted_revision,input_payload,recorded_at)
                VALUES (?1,?2,?3,?4,?5,?6,'{}',?7)", params![task.id.to_string(), input.run_id,
                format!("capacity-{index}"), input.identity.worker_id.to_string(), input.identity.session_id.to_string(),
                input.identity.evidence_revision, NOW]).unwrap();
        }
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .queen_recovery_identity(&input.identity.attention_id)
                .unwrap()
                .unwrap()
                .evidence_revision,
            input.identity.evidence_revision
        );
        store
            .remove_task_as(
                retired.unwrap(),
                &TaskActivityActor::operator(),
                "Retire capacity fixture",
            )
            .unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row("SELECT count(*) FROM queen_recovery_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            256
        );
    }

    #[test]
    fn failed_activity_write_rolls_back_and_the_same_assessment_can_retry() {
        let store = TaskStore::in_memory().unwrap();
        let (input, facts) = fixture(&store);
        store.connection().unwrap().execute_batch("CREATE TRIGGER fail_recovery_activity BEFORE INSERT ON task_activity BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_err()
        );
        assert_eq!(
            store
                .queen_recovery_identity(&input.identity.attention_id)
                .unwrap()
                .unwrap()
                .evidence_revision,
            input.identity.evidence_revision
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_recovery_activity;")
            .unwrap();
        assert!(
            store
                .record_queen_recovery(&input, &facts, &TaskActivityActor::operator(), NOW)
                .is_ok()
        );
    }
}
