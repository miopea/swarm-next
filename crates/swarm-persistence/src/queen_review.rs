//! Authoritative evidence identity for Queen's task-scoped review receipts.

use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use sha2::{Digest, Sha256};
use swarm_domain::{
    ControlRoomEventKind, MAX_QUEEN_REVIEW_OBLIGATIONS, NextMoveOwner, QueenReviewCoverage,
    QueenReviewDispositionInput, QueenReviewDispositionKind, QueenReviewObligation,
    RecordedQueenReviewAssessment, TaskActivityActor, TaskId, VerifiedQueenReviewReceipt,
    queen_review_coverage,
};

use crate::{TaskStore, TaskStoreError};

const MAX_REVIEW_SOURCE_ROWS: usize = 256;
const MAX_QUEUE_ASSESSMENTS: usize = 64;

/// How many times the rotation will re-derive the same answer about work that
/// has not moved, before it stops offering it.
///
/// ⚠️ COUNTING THE REPETITION WAS NEVER THE PROBLEM. `times_seen` has been
/// recorded since schema 179 and IS read in production — swarm-api serves it as
/// `reviews_repeating` at a threshold of 3, with a comment describing this exact
/// failure in Queen's own words. Measured 2026-09-17: 31 receipts sat at or
/// above that threshold, 15 of them at twenty or more, and one at FIFTY-EIGHT.
/// Every one was served on every read and the count kept climbing. She was told,
/// accurately and repeatedly, and told does not mean stopped.
///
/// The rotation never consulted the number. A task re-read fifty-eight times was
/// offered again exactly as though it were new.
///
/// SIX, not three, and the gap is the point: the attention surface fires at 3,
/// so this leaves three further passes in which Queen can act on being told
/// before the work stops being offered. Dropping it at the same threshold would
/// take the work away in the same breath as mentioning it.
///
/// ⚠️ NOTHING IS HIDDEN. Skipped work stays in `reviews_repeating`, which exists
/// to say "you keep seeing this", and returns to the rotation the moment the
/// task MOVES — `times_seen` resets to 1 on a state change, so the skip cannot
/// outlive the condition that caused it.
const MAX_UNCHANGED_REVIEW_PASSES: i64 = 6;

/// How long work may stand still, unasked, before another assessment is REFUSED.
///
/// ⚠️ THE ONE CONSTANT, shared with the surface that reports the same condition.
/// swarm-api re-exports this rather than declaring its own: the last time this
/// codebase held two numbers for one idea they diverged, and a surface that
/// reports at three days beside a guard that refuses at four would be a rule
/// nobody could predict.
pub const MAX_UNASKED_STILL_SECONDS: i64 = 3 * 24 * 60 * 60;

pub(super) fn migrate_incomplete_assessments(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE queen_task_review_receipts_next (
         task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
         run_id TEXT NOT NULL,
         kind TEXT NOT NULL CHECK(kind IN ('external_condition','operator_deferral','insufficient_evidence')),
         accepted_revision TEXT NOT NULL CHECK(length(accepted_revision)=64),
         input_payload TEXT NOT NULL CHECK(length(input_payload)<=32768),
         recorded_sequence INTEGER NOT NULL REFERENCES task_activity(sequence),
         recorded_at INTEGER NOT NULL);
         -- NAMED COLUMNS, NOT A STAR. This rebuild ran happily against a
         -- seven-column table and broke the moment a later migration added two
         -- more: 45 tests failed at once, all reporting 7 columns against 9
         -- values. A star in a table rebuild silently depends on every future
         -- migration never adding a column, which is not a promise anyone can keep.
         INSERT INTO queen_task_review_receipts_next
             (task_id,run_id,kind,accepted_revision,input_payload,recorded_sequence,recorded_at)
         SELECT task_id,run_id,kind,accepted_revision,input_payload,recorded_sequence,recorded_at
         FROM queen_task_review_receipts;
         DROP TABLE queen_task_review_receipts;
         ALTER TABLE queen_task_review_receipts_next RENAME TO queen_task_review_receipts;"
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::QUEEN_INCOMPLETE_ASSESSMENTS_SCHEMA_VERSION,
    )
}

/// A task the review has re-derived the same answer about, and how long for.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepeatedReview {
    pub task_id: String,
    pub title: String,
    pub state: String,
    pub kind: String,
    pub times_seen: i64,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

/// Work that reached Ready and has been waiting for somebody to own it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnroutedReadyWork {
    pub task_id: String,
    pub title: String,
    pub workspace: String,
    pub priority: String,
    pub ready_since: i64,
    pub waiting_seconds: i64,
}

/// Work that has stopped moving and that nobody has been asked about.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnaskedStalledWork {
    pub task_id: String,
    pub title: String,
    pub state: String,
    pub workspace: String,
    pub assigned_worker_id: Option<String>,
    pub last_moved_at: i64,
    pub still_seconds: i64,
}

pub(crate) struct ReviewQueueCache {
    local_changes: u64,
    data_version: i64,
    next_deadline: Option<i64>,
    snapshot: swarm_domain::QueenReviewQueueSnapshot,
}

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

/// The two refusals that stop a review repeating instead of escalating.
///
/// Both are placed AFTER the replay check by their caller, so an exact replay
/// stays idempotent and only a NEW assessment is refused. They are separate
/// causes with separate errors on purpose: one is work that has not MOVED in
/// days, the other a missing FACT re-derived past a count. A task trips the
/// second while moving briskly — of the twenty worst offenders measured on
/// 2026-09-19, exactly one was also stalled three days.
fn refuse_repetition_without_escalation(
    transaction: &rusqlite::Transaction<'_>,
    input: &QueenReviewDispositionInput,
    id: &str,
    now: i64,
) -> Result<(), TaskStoreError> {
    if stalled_with_nobody_asked(transaction, id, now)? {
        return Err(TaskStoreError::ReviewNeedsEscalationNotRepetition);
    }
    if input.kind == QueenReviewDispositionKind::InsufficientEvidence
        && let Some(times) = evidence_rechecked_without_asking(transaction, id)?
    {
        return Err(TaskStoreError::EvidenceNeedsAskingNotRechecking { times });
    }
    Ok(())
}

/// How many times this task's missing fact has been re-derived, when nobody has
/// been asked for it.
///
/// ⚠️ AN INSUFFICIENT-EVIDENCE FINDING CANNOT DISCHARGE ITS OWN OBLIGATION, and
/// that is the whole reason this exists. `review_coverage` counts every live
/// task owned by Queen as an obligation and accepts only `operator_deferral` or
/// `external_condition` receipts as cover; `queen_conductor` then forces a run
/// Incomplete while any obligation is uncovered. So a task judged
/// insufficient-evidence is re-reviewed, judged the same way, and is still
/// uncovered — with no exit through the review path at all. Measured on
/// 2026-09-19: one task at 90 re-derivations, 15 live receipts carrying 425 of
/// 533 live passes.
///
/// The only exits leave her queue rather than satisfy it: the task moves, or a
/// decision is raised, which makes `NextMoveOwner::derive` return `Operator`.
/// Both are in the caller's hands, so refusing here cannot strand the work.
///
/// ⚠️ DELIBERATELY NOT KEYED ON ELAPSED TIME. The sibling guard
/// `stalled_with_nobody_asked` refuses work that has not moved in days, and it
/// does not reach this: of the twenty worst offenders exactly one was stalled
/// three days. Repetition here is a COUNT, not a duration.
fn evidence_rechecked_without_asking(
    transaction: &rusqlite::Transaction<'_>,
    task_id: &str,
) -> Result<Option<i64>, TaskStoreError> {
    let asked: bool = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM decision_requests d
                         WHERE d.task_id = ?1 AND d.state = 'pending')
             OR EXISTS (SELECT 1 FROM task_decision_links l
                          JOIN decision_requests d2 ON d2.id = l.decision_id
                         WHERE l.task_id = ?1 AND d2.state = 'pending')",
        params![task_id],
        |row| row.get(0),
    )?;
    if asked {
        return Ok(None);
    }
    let times: Option<i64> = transaction
        .query_row(
            "SELECT times_seen FROM queen_task_review_receipts
              WHERE task_id = ?1 AND kind = 'insufficient_evidence'",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(times.filter(|seen| *seen >= MAX_UNCHANGED_REVIEW_PASSES))
}

/// Whether this task has stood still past the bound with no question raised.
///
/// ⚠️ THE SAME CONDITION THE BOARD REPORTS, so the guard and the surface cannot
/// disagree about what a stall is. A decision counts whether it is the task's
/// OWN — primary membership, which `swarm_request_decision` writes as
/// `decision_requests.task_id` with no link row — or an additional link.
fn stalled_with_nobody_asked(
    transaction: &rusqlite::Transaction<'_>,
    task_id: &str,
    now: i64,
) -> Result<bool, TaskStoreError> {
    Ok(transaction.query_row(
        "SELECT COALESCE(
                    (SELECT MAX(occurred_at) FROM task_activity
                      WHERE task_id = ?1 AND kind = 'state_changed'),
                    (SELECT created_at FROM tasks WHERE id = ?1)
                ) + ?3 <= ?2
            AND NOT EXISTS (SELECT 1 FROM decision_requests d
                             WHERE d.task_id = ?1 AND d.state = 'pending')
            AND NOT EXISTS (SELECT 1 FROM task_decision_links l
                              JOIN decision_requests d ON d.id = l.decision_id
                             WHERE l.task_id = ?1 AND d.state = 'pending')",
        params![task_id, now, MAX_UNASKED_STILL_SECONDS],
        |row| row.get(0),
    )?)
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
        let snapshot = task_review_snapshot(&transaction, task_id)?;
        transaction.commit()?;
        Ok(snapshot)
    }

    /// Bounded queue judgments, reusing a snapshot only across an unchanged
    /// database generation and before any recorded hold deadline. All access
    /// follows connection -> cache lock order. No terminal observation occurs.
    ///
    /// # Errors
    /// Fails closed on corrupt/unavailable evidence; never returns stale cache
    /// after a failed revalidation or a database recovery fence.
    pub fn queen_review_queue_snapshot(
        &self,
    ) -> Result<swarm_domain::QueenReviewQueueSnapshot, TaskStoreError> {
        let mut connection = self.connection()?;
        let mut cache = self
            .review_queue_cache
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        let local_changes = connection.total_changes();
        let data_version: i64 =
            connection.pragma_query_value(None, "data_version", |row| row.get(0))?;
        let now: i64 = connection.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        if let Some(saved) = cache.as_ref()
            && saved.local_changes == local_changes
            && saved.data_version == data_version
            && now >= saved.snapshot.checked_at
            && saved.next_deadline.is_none_or(|deadline| now < deadline)
        {
            return Ok(saved.snapshot.clone());
        }
        *cache = None;
        let transaction = connection.transaction()?;
        let ids = {
            let mut statement = transaction.prepare(
                // ⚠️ THE TERMINATING CONDITION, and it is the whole of this
                // change: stop offering work whose answer has not changed in
                // MAX_UNCHANGED_REVIEW_PASSES passes. times_seen rises only
                // while the task has NOT moved state, so this skips repetition
                // and never skips progress.
                "SELECT r.task_id FROM queen_task_review_receipts r JOIN tasks t ON t.id=r.task_id
                 WHERE t.removed_at IS NULL AND t.state NOT IN ('completed','abandoned')
                   AND t.hive_id=(SELECT hive_id FROM local_hive_identity WHERE singleton=1)
                   AND r.times_seen < ?1
                 ORDER BY r.task_id LIMIT 65",
            )?;
            statement
                .query_map([MAX_UNCHANGED_REVIEW_PASSES], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let next_deadline = transaction.query_row(
            "SELECT min(blocked_until) FROM tasks WHERE state='blocked' AND removed_at IS NULL
             AND blocked_until>?1 AND hive_id=(SELECT hive_id FROM local_hive_identity WHERE singleton=1)",
            [now], |row| row.get(0),
        )?;
        let mut snapshot = swarm_domain::QueenReviewQueueSnapshot {
            items: Vec::new(),
            truncated: ids.len() > MAX_QUEUE_ASSESSMENTS,
            checked_at: now,
        };
        for id in ids.into_iter().take(MAX_QUEUE_ASSESSMENTS) {
            let id = id
                .parse::<TaskId>()
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            let evidence = task_review_snapshot(&transaction, id)?;
            if evidence.task.next_move_owner == NextMoveOwner::Queen {
                snapshot.items.push(evidence);
            }
        }
        transaction.commit()?;
        *cache = Some(ReviewQueueCache {
            local_changes,
            data_version,
            next_deadline,
            snapshot: snapshot.clone(),
        });
        Ok(snapshot)
    }
}

fn task_review_snapshot(
    transaction: &Connection,
    task_id: TaskId,
) -> Result<swarm_domain::QueenTaskReviewEvidence, TaskStoreError> {
    let obligation = task_review_evidence(transaction, task_id)?;
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
    Ok(swarm_domain::QueenTaskReviewEvidence {
        task,
        obligation,
        current_run_id,
        previous_assessment,
    })
}

/// Whether this task has actually MOVED since the review last recorded on it.
///
/// ⚠️ EXCLUDES THE REVIEW'S OWN ROW. A disposition writes a 'corrected' activity
/// entry carrying the task's current state, so counting any activity would make
/// every review look like movement and reset the repetition count forever —
/// precisely the blindness this change exists to remove. Only a real transition
/// counts.
fn moved_since_last_review(
    transaction: &rusqlite::Transaction<'_>,
    task_id: &str,
) -> Result<bool, TaskStoreError> {
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_activity a
             WHERE a.task_id = ?1 AND a.kind = 'state_changed'
               AND a.sequence > COALESCE(
                   (SELECT recorded_sequence FROM queen_task_review_receipts WHERE task_id = ?1),
                   0))",
        [task_id],
        |row| row.get(0),
    )?)
}

impl TaskStore {
    /// Work that has not moved in days and that NOBODY HAS BEEN ASKED ABOUT.
    ///
    /// ⚠️ THE SYSTEM CANNOT SEE ITS OWN OPERATOR-GATED WORK. `NextMoveOwner`
    /// derives `Operator` ONLY when `awaiting_operator_decision` is true — that
    /// is, only when a decision already exists. So a task genuinely waiting on a
    /// person, with no decision filed, reads as Queen, Blocked or Release and is
    /// invisible as theirs. Asking is what makes it visible, which means the one
    /// failure the board cannot show is nobody having asked.
    ///
    /// Measured 2026-09-18: of 33 live non-terminal tasks with no pending
    /// decision, NINETEEN had gone more than two days without moving and six had
    /// sat over a week. Three of them were waiting on the operator — a capability
    /// nobody had been granted, a design answer nobody had been asked for, and a
    /// config value nobody had requested — and one of those carried a block note
    /// explicitly reasoning that it was "not a crisply fileable operator
    /// decision", so the choice not to ask was deliberate and then invisible.
    ///
    /// ⚠️ IT DELIBERATELY DOES NOT SAY WHO TO ASK. A task this old with no
    /// decision may need the operator, or Queen, or simply to be abandoned. The
    /// honest claim is "this stopped and nobody raised it", and inventing an
    /// owner would be the same guess that produced a wrong `Blocked` label on
    /// work that was merely unfinished.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn unasked_stalled_work(
        &self,
        now: i64,
        still_for_seconds: i64,
    ) -> Result<Vec<UnaskedStalledWork>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            // A decision counts whether it is this task's OWN (primary
            // membership, what swarm_request_decision writes) or an additional
            // link. Reading only the link table would call a task unasked while
            // an operator question about it sits pending — the same
            // primary-versus-link blindness that made a guard refuse blocks it
            // should have allowed.
            "SELECT t.id, t.title, t.state, t.workspace, t.assigned_worker_id,
                    COALESCE(moved.at, t.created_at)
             FROM tasks t
             LEFT JOIN (SELECT task_id, MAX(occurred_at) AS at FROM task_activity
                         WHERE kind = 'state_changed' GROUP BY task_id) moved
               ON moved.task_id = t.id
             WHERE t.removed_at IS NULL
               AND t.state NOT IN ('completed', 'abandoned')
               AND t.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)
               AND COALESCE(moved.at, t.created_at) + ?2 <= ?1
               AND NOT EXISTS (SELECT 1 FROM decision_requests d
                                WHERE d.task_id = t.id AND d.state = 'pending')
               AND NOT EXISTS (SELECT 1 FROM task_decision_links l
                                 JOIN decision_requests d ON d.id = l.decision_id
                                WHERE l.task_id = t.id AND d.state = 'pending')
             ORDER BY COALESCE(moved.at, t.created_at) LIMIT 64",
        )?;
        let rows = statement.query_map(params![now, still_for_seconds], |row| {
            let last_moved_at: i64 = row.get(5)?;
            Ok(UnaskedStalledWork {
                task_id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                workspace: row.get(3)?,
                assigned_worker_id: row.get(4)?,
                last_moved_at,
                still_seconds: (now - last_moved_at).max(0),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Ready work nobody owns, once it has waited longer than routing takes.
    ///
    /// ⚠️ OWNERLESS READY IS INVISIBLE TO THE DETECTOR THAT WATCHES READY WORK.
    /// `UNSTARTED_WORK_CANDIDATES_SQL` opens with an INNER join on
    /// `assigned_worker_id`, so a task with no owner is excluded before any age
    /// test runs. That detector has fired 265 times and cannot once have been
    /// about ownerless work. It is the same blindness the Blocked enforcement
    /// was built for — 24 of 25 blocked tasks were ownerless and therefore
    /// unwatched — reproduced one state over.
    ///
    /// ⚠️ THIS IS A SEPARATE QUERY ON PURPOSE, NOT A LOOSENED JOIN. Relaxing
    /// that INNER join to a LEFT join would silently change what the EXISTING
    /// detector reports, because every field it selects downstream — worker id,
    /// session id, the dispatch join — assumes an owner exists.
    ///
    /// ⚠️ AND IT SURFACES RATHER THAN REFUSES. Ownerless Ready is the ordinary
    /// transient between promotion and routing: six tasks were legitimately in
    /// that state when this was written, all under twelve hours old, and a guard
    /// at the transition would have refused every one of them.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn unrouted_ready_work(
        &self,
        now: i64,
        waiting_at_least_seconds: i64,
    ) -> Result<Vec<UnroutedReadyWork>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            // `ready_since` is the LATEST entry into Ready, not the earliest:
            // work that went Ready, was routed, came back and lost its owner has
            // been waiting since it came back, not since it first arrived.
            "SELECT t.id, t.title, t.workspace, t.priority, ready.at
             FROM tasks t
             JOIN (SELECT task_id, MAX(occurred_at) AS at FROM task_activity
                    WHERE kind = 'state_changed' AND to_state = 'ready'
                    GROUP BY task_id) ready ON ready.task_id = t.id
             WHERE t.state = 'ready'
               AND t.assigned_worker_id IS NULL
               AND t.removed_at IS NULL
               AND t.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)
               AND ready.at + ?2 <= ?1
             ORDER BY ready.at LIMIT 64",
        )?;
        let rows = statement.query_map(params![now, waiting_at_least_seconds], |row| {
            let ready_since: i64 = row.get(4)?;
            Ok(UnroutedReadyWork {
                task_id: row.get(0)?,
                title: row.get(1)?,
                workspace: row.get(2)?,
                priority: row.get(3)?,
                ready_since,
                waiting_seconds: (now - ready_since).max(0),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Tasks the review keeps reaching the same conclusion about, unchanged.
    ///
    /// ⚠️ THIS REPORTS A FACT ABOUT THE REVIEW, NOT A BELIEF ABOUT THE TASK, and
    /// that is deliberate. Queen's own account of this defect includes two reads
    /// that were wrong in OPPOSITE directions: a task whose prose said ready
    /// carried an operator gate in its resolving decision, and a task everyone
    /// read as parked completed within the hour once actually routed. A surface
    /// that recorded the review's conclusion would have made both wrong answers
    /// durable. "Seen four times without converting" cannot be wrong in that
    /// way — it says only that the same material produced the same answer
    /// repeatedly, which is true whichever answer was right.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn reviews_repeating_without_conversion(
        &self,
        at_least: i64,
    ) -> Result<Vec<RepeatedReview>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT r.task_id, t.title, t.state, r.kind, r.times_seen,
                    COALESCE(r.first_seen_at, r.recorded_at), r.recorded_at
             FROM queen_task_review_receipts r
             JOIN tasks t ON t.id = r.task_id AND t.removed_at IS NULL
             WHERE r.times_seen >= ?1
               AND t.state NOT IN ('completed', 'abandoned')
             ORDER BY r.times_seen DESC, r.first_seen_at LIMIT 64",
        )?;
        let rows = statement.query_map([at_least], |row| {
            Ok(RepeatedReview {
                task_id: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                kind: row.get(3)?,
                times_seen: row.get(4)?,
                first_seen_at: row.get(5)?,
                last_seen_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
    ) -> Result<RecordedQueenReviewAssessment, TaskStoreError> {
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
        let moved = moved_since_last_review(&transaction, &id)?;
        if replay {
            transaction.commit()?;
            return Ok(RecordedQueenReviewAssessment {
                task_id: input.task_id,
                evidence_revision: current.evidence_revision,
                covers_wait: input.kind != QueenReviewDispositionKind::InsufficientEvidence,
            });
        }
        // ⚠️ RE-READING IS REFUSED ONCE WORK HAS STOOD STILL AND NOBODY HAS
        // ASKED. This is the one control in the review path that REFUSES rather
        // than reports, and it exists because reporting was not enough: the
        // repetition was already counted, already surfaced to Queen at a
        // threshold of three, and already showing one task at fifty-eight
        // re-reads — and the count kept climbing. Being told is not being
        // stopped.
        //
        // Placed AFTER the replay check on purpose: an exact replay is
        // idempotent and already returns early, so this only refuses a NEW
        // assessment of work that is going nowhere.
        //
        // Every exit is in the caller's own hands — raise the question, move the
        // task, or abandon it — and any of them clears the condition, so this
        // cannot strand work it refuses.
        refuse_repetition_without_escalation(&transaction, input, &id, now)?;
        let active: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM queen_automation WHERE id=1 AND run_id=?1
             AND delivery_session_id IS NOT NULL
             AND (state IN ('running','uncertain')
                 OR (state IN ('queued','delivering') AND delivered_at IS NOT NULL)))",
            [&input.run_id],
            |row| row.get(0),
        )?;
        if !active || current.evidence_revision != input.expected_revision {
            return Err(TaskStoreError::IntegrityFailure("review run or task evidence changed; reread current evidence before recording a disposition".into()));
        }
        let task = transaction.query_row(
            &format!("{} WHERE t.id=?1", TaskStore::TASK_PROJECTION),
            [&id],
            crate::task_from_row,
        )?;
        if !input.can_record(task.state, task.next_move_owner) {
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
            QueenReviewDispositionKind::InsufficientEvidence => "insufficient_evidence",
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
            // ⚠️ THE COUNT RISES ONLY WHILE THE TASK HAS NOT MOVED, and two
            // obvious anchors were wrong before this one.
            //
            // `accepted_revision` looked exact — same evidence, same answer —
            // but recording a disposition WRITES an activity row, so the
            // revision changes every time the review speaks and no two passes
            // can ever match. A test expecting three found zero.
            //
            // The task's STATE was no better: a disposition can only be recorded
            // on Queen-owned waiting work, so a task that moves leaves the
            // eligible population entirely and the comparison never fires — and
            // a task that goes Blocked, Ready, Blocked would read as unchanged
            // when it had in fact converted and come back.
            //
            // `moved_since_last_review` asks the activity log directly: has this
            // task had a STATE CHANGE since the last receipt. That is what "not
            // converted" means, and it survives both shapes.
            "INSERT INTO queen_task_review_receipts (task_id,run_id,kind,accepted_revision,input_payload,recorded_sequence,recorded_at,times_seen,first_seen_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,1,?7) ON CONFLICT(task_id) DO UPDATE SET
             run_id=excluded.run_id,kind=excluded.kind,accepted_revision=excluded.accepted_revision,input_payload=excluded.input_payload,
             recorded_sequence=excluded.recorded_sequence,recorded_at=excluded.recorded_at,
             times_seen = CASE WHEN ?8 THEN 1 ELSE queen_task_review_receipts.times_seen + 1 END,
             first_seen_at = CASE WHEN ?8 THEN excluded.recorded_at
                 ELSE COALESCE(queen_task_review_receipts.first_seen_at, excluded.recorded_at) END",
            rusqlite::params![id, input.run_id, kind, accepted.evidence_revision, payload, recorded_sequence, now, moved],
        )?;
        crate::insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(RecordedQueenReviewAssessment {
            task_id: input.task_id,
            evidence_revision: accepted.evidence_revision,
            covers_wait: input.can_cover(task.state, task.next_move_owner),
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

/// ⚠️ COVERAGE NO LONGER DEPENDS ON THE RUN, and `_run_id` is kept only because
/// every caller still legitimately names the run it is asking about.
///
/// Until 2026-09-18 an `external_condition` receipt counted only inside the run
/// that recorded it, so the next run found the obligation uncovered and
/// `queen_conductor` marked the run Incomplete unless Queen re-reviewed a wait
/// whose evidence had not moved. That is what "Queen still shows external waits
/// between review runs" meant, measured at 177 re-review passes across 8
/// receipts, one of them 86 times.
///
/// The operator answered it on 2026-09-18 (ADR0082's presentation, doc 09,
/// after decision 01a0b73f-1263 asked for an interview): show a checked external
/// wait as a HOLD with Last checked wording rather than re-raising it.
///
/// ⚠️ WHAT STILL RE-RAISES, and it is the reason this is presentation and not
/// permission: coverage matches a receipt against the task's CURRENT evidence
/// revision. Changed evidence does not match, so it reads uncovered and returns
/// to Queen exactly as before. `insufficient_evidence` receipts are still never
/// selected here at all. Fresh evidence remains required before acting; what
/// stops is paying a full review pass to re-derive an unchanged answer.
pub(super) fn review_coverage(
    connection: &Connection,
    _run_id: &str,
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
        let saved: Option<(String, i64)> = connection.query_row(
            "SELECT accepted_revision, times_seen FROM queen_task_review_receipts WHERE task_id=?1 AND kind IN ('operator_deferral','external_condition')",
            rusqlite::params![task.id.to_string()], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        if let Some((accepted_revision, times_seen)) = saved {
            receipts.push(VerifiedQueenReviewReceipt {
                task_id: task.id,
                // ⚠️ THE BOUND, ON THE COVERAGE PATH TOO, and it reuses both the
                // constant and the anchor rather than adding a third number.
                //
                // `times_seen` resets to 1 whenever the task MOVES state and
                // otherwise increments, so reaching the bound already means
                // "re-derived this many times while standing still". Matching
                // the saved revision was never a question about whether the
                // ANSWER changed: the revision hashes task_messages and message
                // deliveries, so a message or a delivery-state update uncovered
                // the receipt and forced a re-review that could only reach the
                // same conclusion. Measured 2026-09-19: a receipt created after
                // the offer-path and refusal-path bounds both shipped still
                // reached twice the bound in 3.7 hours.
                //
                // Accepting the CURRENT revision here says the wait is covered
                // until the task moves, which is exactly what the count means.
                // Operator decision 01a0bc10-821a resolved this; ADR 0104
                // declined it for these two kinds and is superseded on that
                // point, with the cost named there and in its amendment.
                //
                // ⚠️ NOTHING IS HIDDEN. `reviews_repeating` still reports the
                // count, and a real state change resets it and returns the work.
                evidence_revision: if times_seen >= MAX_UNCHANGED_REVIEW_PASSES {
                    current.evidence_revision.clone()
                } else {
                    accepted_revision
                },
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
    let mut task = connection.query_row(
        &format!("{} WHERE t.id = ?1 AND t.removed_at IS NULL AND t.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)", TaskStore::TASK_PROJECTION),
        [task_id.to_string()],
        crate::task_from_row,
    ).optional()?.ok_or(TaskStoreError::NotFound)?;
    let mut digest = Sha256::new();
    // ⚠️ NEVER HASH `park`. It is derived from the review receipt, so recording
    // a disposition would change the very revision that disposition accepted --
    // no two passes could ever match, every wait would read uncovered, and the
    // repetition 07c24d3d just bounded would come straight back through a new
    // door. Caught by review_disposition_tools_are_queen_only_and_preserve_
    // blocked_work, which went from covered_for_current_run to evidence_changed
    // the moment the column was added.
    task.park = None;
    if task.state == swarm_domain::TaskState::Blocked {
        // A retained worker's new PTY is not new evidence about its blocker.
        // Keep durable ownership, every task fact and all decision/message/
        // activity sources below. Recovery receipts retain their own exact
        // session fence; this normalization cannot cover worker execution.
        task.assigned_session_id = None;
        task.updated_at = 0;
        digest.update(b"swarm-queen-blocked-review-evidence-v2");
    } else {
        digest.update(b"swarm-queen-review-evidence-v1");
    }
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

    /// ⚠️ THE REPETITION USED TO LEAVE NO TRACE. The receipt is keyed by task
    /// and written ON CONFLICT DO UPDATE, so re-reading the same task overwrote
    /// the previous row: 93 receipts for 93 tasks however many times each was
    /// read. Queen re-read the same twelve drafts every few cycles for days and
    /// nothing anywhere could tell that it had happened more than once.
    #[test]
    fn re_deriving_the_same_answer_about_unchanged_work_is_counted() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);

        // ⚠️ THE PROSE VARIES AND THE VERDICT DOES NOT, which is exactly what
        // Queen described: a fresh sentence about the same wait, every cycle.
        // An IDENTICAL payload is short-circuited as a replay — idempotency for
        // retries — so a test that repeated itself verbatim wrote once and
        // measured nothing. That is a difference between a retry and a
        // re-derivation, and only the second is this signal.
        for (now, condition) in [
            (101, "External fixture endpoint reports maintenance"),
            (102, "Still reporting maintenance on the second look"),
            (103, "Maintenance again; nothing has moved"),
        ] {
            input.condition = condition.into();
            // Each cycle re-reads the evidence before speaking, because the
            // previous disposition moved the revision. That is the loop's real
            // shape, and it is also why `accepted_revision` could never serve as
            // the "unchanged" anchor: it changes every time the review speaks.
            input.expected_revision = store
                .queen_task_review_evidence(input.task_id)
                .unwrap()
                .evidence_revision;
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), now)
                .unwrap();
        }

        let repeating = store.reviews_repeating_without_conversion(3).unwrap();
        assert_eq!(repeating.len(), 1, "three identical passes is the signal");
        assert_eq!(repeating[0].times_seen, 3);
        assert_eq!(
            repeating[0].first_seen_at, 101,
            "the operator's question is how long this has been going round, so \
             the count carries its start"
        );
    }

    /// Re-reads `task` `passes` times without ever letting it move.
    fn re_read_without_moving(
        store: &TaskStore,
        input: &mut QueenReviewDispositionInput,
        passes: i64,
    ) {
        for pass in 0..passes {
            input.condition = format!("Still waiting, pass {pass}");
            // Re-read each cycle: recording a disposition moves the revision,
            // which is why accepted_revision could never anchor "unchanged".
            input.expected_revision = store
                .queen_task_review_evidence(input.task_id)
                .unwrap()
                .evidence_revision;
            store
                .record_queen_review_disposition(input, &TaskActivityActor::operator(), 200 + pass)
                .unwrap();
        }
    }

    /// ⚠️ AN INSUFFICIENT-EVIDENCE FINDING CANNOT DISCHARGE ITS OWN OBLIGATION,
    /// so re-deriving it is the one move that can never end the loop.
    ///
    /// `review_coverage` counts every live Queen-owned task as an obligation and
    /// accepts only `operator_deferral` or `external_condition` as cover;
    /// `queen_conductor` forces a run Incomplete while any obligation is
    /// uncovered. A task judged insufficient-evidence is therefore re-reviewed,
    /// judged identically, and still uncovered — for ever.
    ///
    /// Measured 2026-09-19 before building: one task at 90 re-derivations, and
    /// 15 live receipts carrying 425 of 533 live passes. The sibling guard does
    /// NOT reach this population — of the twenty worst offenders exactly one had
    /// stood still for three days; the rest had moved within two.
    ///
    /// Boundary asserted on both sides, because this REFUSES and an off-by-one
    /// either blocks ordinary review or never fires.
    #[test]
    fn a_missing_fact_re_checked_past_the_bound_is_refused() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        input.kind = QueenReviewDispositionKind::InsufficientEvidence;

        // One short of the bound: still ordinary review, and it must stay silent.
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES - 1);
        input.condition = "One short of the bound".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 400)
                .is_ok(),
            "a task under the bound is ordinary work and must not be refused"
        );

        // Now at the bound, and the next assessment is refused.
        input.condition = "And again, with nobody asked".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        let refused =
            store.record_queen_review_disposition(&input, &TaskActivityActor::operator(), 401);
        assert!(
            matches!(
                refused,
                Err(TaskStoreError::EvidenceNeedsAskingNotRechecking { times })
                    if times == MAX_UNCHANGED_REVIEW_PASSES
            ),
            "at the bound with nobody asked, re-checking must be refused: {refused:?}"
        );
    }

    /// Asking clears THIS refusal, which is the exit the error names. Asserted
    /// separately because a guard that refuses without a working exit strands
    /// the work it refuses.
    #[test]
    fn asking_for_the_missing_fact_clears_the_evidence_refusal() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let mut input = external_wait(&store);
        input.kind = QueenReviewDispositionKind::InsufficientEvidence;
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES);

        input.condition = "Refused before asking".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        assert!(
            matches!(
                store.record_queen_review_disposition(&input, &TaskActivityActor::operator(), 500),
                Err(TaskStoreError::EvidenceNeedsAskingNotRechecking { .. })
            ),
            "precondition: it must be refused before asking can clear it"
        );

        let actions = vec!["Supply the fact".to_owned(), "Abandon it".to_owned()];
        store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(input.task_id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "The missing fact has been re-checked past the bound",
                summary: "Fixture.",
                reason: "Fixture.",
                risk: "",
                evidence: "",
                suggested_action: "Supply the fact",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();

        input.condition = "Now the question is pending".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        let after =
            store.record_queen_review_disposition(&input, &TaskActivityActor::operator(), 501);

        // As with the sibling guard, the outcome is stronger than "allowed
        // again": a pending decision makes the task the OPERATOR's, and a
        // pre-existing rule then bars Queen from dispositioning it at all. What
        // is asserted is only what THIS guard owns — it is no longer the refuser.
        assert!(
            !matches!(
                after,
                Err(TaskStoreError::EvidenceNeedsAskingNotRechecking { .. })
            ),
            "asking must clear THIS refusal, whatever other rules then apply: {after:?}"
        );
    }

    /// Puts `title` into Ready with no owner, `ready_at` seconds on the clock.
    fn unowned_ready(store: &TaskStore, title: &str) -> TaskId {
        let task = store.create_task(title, "/workspace/demo").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        task.id
    }

    /// ⚠️ RE-READING STALLED WORK IS REFUSED, NOT MERELY REPORTED.
    ///
    /// Reporting was tried first and was not enough. The repetition was already
    /// counted by `times_seen`, already served to Queen at a threshold of three,
    /// and already showing one task re-read FIFTY-EIGHT times — and the count
    /// kept climbing. Measured the day this shipped: 741 corrections against 23
    /// state changes in a single day. Being told is not being stopped.
    ///
    /// Boundary asserted on both sides, because this REFUSES and an off-by-one
    /// either blocks ordinary review or never fires at all.
    #[test]
    fn re_reading_work_nobody_asked_about_is_refused_once_it_has_stood_still() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        let moved = store
            .list_task_activity(input.task_id, 50)
            .unwrap()
            .events
            .iter()
            .filter(|entry| entry.kind == swarm_domain::TaskActivityKind::StateChanged)
            .map(|entry| entry.occurred_at)
            .next_back()
            .expect("it moved into Blocked");

        input.condition = "Still waiting, just under the bound".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .record_queen_review_disposition(
                &input,
                &TaskActivityActor::operator(),
                moved + MAX_UNASKED_STILL_SECONDS - 1,
            )
            .expect("under the bound this is ordinary review and must be allowed");

        input.condition = "Still waiting, at the bound".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        let refused = store.record_queen_review_disposition(
            &input,
            &TaskActivityActor::operator(),
            moved + MAX_UNASKED_STILL_SECONDS,
        );
        assert!(
            matches!(
                refused,
                Err(TaskStoreError::ReviewNeedsEscalationNotRepetition)
            ),
            "at the bound another assessment must be refused: {refused:?}"
        );
    }

    /// ⚠️ AND ASKING CLEARS IT — WITHOUT THIS THE REFUSAL IS A TRAP.
    ///
    /// A control with no exit does not force escalation, it strands work and
    /// teaches people to route around the tool. Every named way out is in the
    /// caller's own hands; this asserts the first of them actually works, end to
    /// end, through the same guard that refused a moment earlier.
    ///
    /// The decision is raised FROM the task — primary membership, which writes
    /// `decision_requests.task_id` and NO `task_decision_links` row — so a guard
    /// reading only the link table would keep refusing after the question had
    /// been asked, which is the worst version of this control.
    #[test]
    fn raising_the_question_lets_the_review_speak_again() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let mut input = external_wait(&store);
        let moved = store
            .list_task_activity(input.task_id, 50)
            .unwrap()
            .events
            .iter()
            .filter(|entry| entry.kind == swarm_domain::TaskActivityKind::StateChanged)
            .map(|entry| entry.occurred_at)
            .next_back()
            .expect("it moved into Blocked");
        let past_bound = moved + MAX_UNASKED_STILL_SECONDS;

        input.condition = "Still waiting".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), past_bound)
                .is_err(),
            "precondition: it must actually be refused before asking can clear it"
        );

        let actions = vec!["Release it".to_owned(), "Keep waiting".to_owned()];
        store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(input.task_id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "This has not moved in days -- release or keep waiting?",
                summary: "Fixture.",
                reason: "Fixture.",
                risk: "",
                evidence: "",
                suggested_action: "Release it",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();

        input.condition = "Waiting on the operator's answer now".into();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        let after = store.record_queen_review_disposition(
            &input,
            &TaskActivityActor::operator(),
            past_bound,
        );

        // ⚠️ THE OUTCOME IS BETTER THAN "ALLOWED AGAIN", AND THE TEST SAYS SO
        // RATHER THAN OVERSTATING IT. Raising the question makes the task the
        // OPERATOR's — NextMoveOwner::derive returns Operator whenever a decision
        // is pending — and a pre-existing rule then refuses Queen a disposition
        // on work that is not hers to wait on. So the escalation does not merely
        // unblock re-reading; it takes the task out of her review loop entirely,
        // which is the outcome this control exists to produce.
        //
        // What is asserted is exactly what this guard is responsible for: it is
        // no longer the one refusing.
        assert!(
            !matches!(
                after,
                Err(TaskStoreError::ReviewNeedsEscalationNotRepetition)
            ),
            "asking must clear THIS refusal, whatever other rules then apply: {after:?}"
        );
        assert!(
            matches!(after, Err(TaskStoreError::IntegrityFailure(ref reason))
                if reason.contains("not Queen-owned waiting work")),
            "and the task should now be the operator's, out of the review loop: {after:?}"
        );
    }

    /// ⚠️ THE BOARD CANNOT SEE THE ONE FAILURE THAT MATTERS: NOBODY ASKING.
    ///
    /// `NextMoveOwner` derives Operator ONLY when a decision already exists. So a
    /// task genuinely waiting on a person, with none filed, reads as Queen or
    /// Blocked and is invisible as theirs — and the act that would make it
    /// visible is the very act nobody performed.
    ///
    /// Measured the day this shipped: 19 of 33 live non-terminal tasks with no
    /// pending decision had not moved in over two days, six for more than a week,
    /// and three of those were waiting on the operator.
    ///
    /// Boundary asserted on both sides: a task one second under the bound is
    /// ordinary work in progress and must stay silent.
    #[test]
    fn work_that_stopped_and_nobody_raised_it_is_surfaced_after_the_bound() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Nobody asked about this", "/workspace/demo")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let moved = store
            .list_task_activity(task.id, 50)
            .unwrap()
            .events
            .iter()
            .filter(|entry| entry.kind == swarm_domain::TaskActivityKind::StateChanged)
            .map(|entry| entry.occurred_at)
            .next_back()
            .expect("it moved at least once");
        let three_days = 3 * 24 * 60 * 60;

        assert!(
            store
                .unasked_stalled_work(moved + three_days - 1, three_days)
                .unwrap()
                .is_empty(),
            "one second under the bound is work in progress, not a stall"
        );

        let stalled = store
            .unasked_stalled_work(moved + three_days, three_days)
            .unwrap();
        assert_eq!(
            stalled.len(),
            1,
            "at the bound, work nobody raised must be surfaced"
        );
        assert_eq!(stalled[0].task_id, task.id.to_string());
        assert_eq!(
            stalled[0].still_seconds, three_days,
            "it must say HOW LONG, or a three-day stall reads like a three-week one"
        );
    }

    /// ⚠️ AND ASKING MUST TAKE IT OFF THE LIST. This is the property the whole
    /// framework rests on: the surface exists to provoke a question, so a task
    /// with one already pending is not a failure and must go quiet. Without this
    /// the list grows monotonically, every raised decision keeps shouting, and it
    /// is ignored within a week.
    ///
    /// A decision raised FROM the task is primary membership —
    /// `swarm_request_decision` writes `decision_requests.task_id` and NO
    /// `task_decision_links` row — so a query reading only the link table would
    /// call this task unasked while an operator question about it sat pending.
    #[test]
    fn asking_about_stalled_work_takes_it_off_the_list() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let task = store
            .create_task("Someone did ask", "/workspace/demo")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let three_days = 3 * 24 * 60 * 60;
        // Far past any plausible clock, so the bound is what decides, not the date.
        let far_future = i64::MAX / 4;

        assert_eq!(
            store
                .unasked_stalled_work(far_future, three_days)
                .unwrap()
                .len(),
            1,
            "precondition: it must actually be stalled before asking can matter"
        );

        let actions = vec!["Grant it".to_owned(), "Leave it".to_owned()];
        store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(task.id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Does this need you?",
                summary: "Fixture.",
                reason: "Fixture.",
                risk: "",
                evidence: "",
                suggested_action: "Grant it",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();

        assert!(
            store
                .unasked_stalled_work(far_future, three_days)
                .unwrap()
                .is_empty(),
            "a task with a question pending has been raised, and must stop being reported"
        );
    }
    /// ⚠️ OWNERLESS READY IS INVISIBLE TO THE DETECTOR THAT WATCHES READY WORK.
    ///
    /// `UNSTARTED_WORK_CANDIDATES_SQL` opens with an INNER join on
    /// `assigned_worker_id`, so work with no owner is excluded before any age test
    /// runs. That detector has fired 265 times and cannot once have been about
    /// ownerless work — the same blindness the Blocked enforcement was built
    /// for, where 24 of 25 blocked tasks were ownerless and therefore unwatched,
    /// reproduced one state over.
    ///
    /// The boundary is asserted on BOTH sides. Under the bound this must stay
    /// silent, because ownerless Ready is the ordinary transient between
    /// promotion and routing — six tasks were legitimately in it when this was
    /// written, all under twelve hours old, and a rule that fired on them would
    /// be punishing normal work.
    #[test]
    fn ready_work_nobody_owns_surfaces_only_after_routing_has_had_its_chance() {
        let store = TaskStore::in_memory().unwrap();
        let task = unowned_ready(&store, "Nobody has routed this");
        let ready_at = store
            .list_task_activity(task, 50)
            .unwrap()
            .events
            .iter()
            .filter(|entry| entry.to_state == Some(TaskState::Ready))
            .map(|entry| entry.occurred_at)
            .next_back()
            .expect("it entered Ready");
        let day = 24 * 60 * 60;

        assert!(
            store
                .unrouted_ready_work(ready_at + day - 1, day)
                .unwrap()
                .is_empty(),
            "one second under the bound is ordinary routing and must stay silent"
        );

        let surfaced = store.unrouted_ready_work(ready_at + day, day).unwrap();
        assert_eq!(
            surfaced.len(),
            1,
            "at the bound, work nobody has taken must be surfaced for routing"
        );
        assert_eq!(surfaced[0].task_id, task.to_string());
        assert_eq!(
            surfaced[0].waiting_seconds, day,
            "it has to say HOW LONG, or the reader cannot tell a day from a week"
        );
    }

    /// ⚠️ AND IT MUST NOT REPORT WORK THAT HAS AN OWNER, WHICH IS THE WHOLE
    /// DISTINCTION. A task assigned and simply not started yet is already
    /// covered by `assigned_ready_work_not_started_attention`; reporting it here
    /// too would duplicate that surface and make this one noise.
    #[test]
    fn ready_work_that_has_an_owner_is_somebody_elses_problem() {
        let store = TaskStore::in_memory().unwrap();
        let task = unowned_ready(&store, "Routed promptly");
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store.assign_task(task, session).unwrap();

        assert!(
            store
                .unrouted_ready_work(i64::MAX / 2, 24 * 60 * 60)
                .unwrap()
                .is_empty(),
            "owned work is watched elsewhere, however long it sits"
        );
    }

    /// ⚠️ THE ROTATION HAS TO STOP ASKING A QUESTION IT HAS ALREADY ANSWERED.
    ///
    /// Counting the repetition was never the gap. `times_seen` has been recorded
    /// since schema 179 and is served to Queen as `reviews_repeating` at a
    /// threshold of 3. Measured on the live board 2026-09-17: 31 receipts at or
    /// above it, 15 at twenty or more, one at FIFTY-EIGHT — every one served on
    /// every read, and the count still climbing. Being told is not stopping.
    ///
    /// The rotation never consulted the number, so a task re-read fifty-eight
    /// times was offered again as though it were new. This is the terminating
    /// condition, and the assertion is on the boundary rather than on "some
    /// large number", because an off-by-one here either drops work a pass early
    /// or never drops it at all.
    #[test]
    fn the_rotation_stops_offering_work_whose_answer_has_not_changed() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);

        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES - 1);
        assert!(
            store
                .queen_review_queue_snapshot()
                .unwrap()
                .items
                .iter()
                .any(|item| item.task.id == input.task_id),
            "one pass below the limit it is still offered — the rotation must not \
             give up while Queen may still act on being told"
        );

        re_read_without_moving(&store, &mut input, 1);
        assert!(
            !store
                .queen_review_queue_snapshot()
                .unwrap()
                .items
                .iter()
                .any(|item| item.task.id == input.task_id),
            "at the limit the rotation stops re-deriving the same answer"
        );
    }

    /// ⚠️ AND THE SKIP MUST NOT OUTLIVE THE CONDITION THAT CAUSED IT.
    ///
    /// Work dropped from the rotation and never returned would be worse than the
    /// repetition it replaced: the repetition at least kept the task in front of
    /// somebody. `times_seen` resets to 1 when the task MOVES, so movement is
    /// what brings it back — and this asserts that end to end rather than
    /// trusting the reset in isolation.
    #[test]
    fn work_that_moves_is_offered_again_after_being_skipped() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES);
        assert!(
            !store
                .queen_review_queue_snapshot()
                .unwrap()
                .items
                .iter()
                .any(|item| item.task.id == input.task_id),
            "precondition: it must actually be skipped before the return means anything"
        );

        // The task moves: Blocked -> Ready -> Blocked, which is what converting
        // a wait and re-encountering it looks like.
        store
            .transition_task(input.task_id, TaskState::Ready)
            .unwrap();
        store
            .transition_task_with_note(input.task_id, TaskState::Blocked, "Waiting again")
            .unwrap();
        re_read_without_moving(&store, &mut input, 1);

        assert!(
            store
                .queen_review_queue_snapshot()
                .unwrap()
                .items
                .iter()
                .any(|item| item.task.id == input.task_id),
            "a task that moved is genuinely new work and must return to the rotation"
        );
    }

    /// ⚠️ AND IT MUST NOT FIRE ON WORK THAT IS MOVING. Counting every visit
    /// would make a task under active discussion look as stuck as one nobody
    /// has touched.
    ///
    /// ⚠️ THE FIRST VERSION OF THIS TEST PASSED WITHOUT MEASURING ANYTHING: it
    /// asserted the repeating list was empty, which was equally true when the
    /// counter never rose at all. It would have gone green against a completely
    /// broken counter. It now asserts the count itself, and the reset is driven
    /// by the task moving — Blocked, out, and back — which is what converting a
    /// wait and re-encountering it actually looks like.
    #[test]
    fn a_task_that_moves_starts_the_count_again() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        let record = |input: &mut QueenReviewDispositionInput, condition: &str, now: i64| {
            input.condition = condition.into();
            input.expected_revision = store
                .queen_task_review_evidence(input.task_id)
                .unwrap()
                .evidence_revision;
            store
                .record_queen_review_disposition(input, &TaskActivityActor::operator(), now)
                .unwrap();
        };

        record(&mut input, "Waiting once", 101);
        record(&mut input, "Waiting twice", 102);
        assert_eq!(
            store.reviews_repeating_without_conversion(2).unwrap()[0].times_seen,
            2,
            "two passes over work that has not moved is two"
        );

        // The wait gets converted: the task moves out of Blocked. Later it comes
        // back — the shape a state comparison would have read as "unchanged".
        store
            .transition_task_with_note(input.task_id, TaskState::Ready, "Unblocked")
            .unwrap();
        store
            .transition_task_with_note(input.task_id, TaskState::Blocked, "Waiting again")
            .unwrap();
        record(&mut input, "A fresh wait after a real move", 103);

        let after = store.reviews_repeating_without_conversion(1).unwrap();
        assert_eq!(
            after[0].times_seen, 1,
            "a task that moved is not a task being re-derived: {after:?}"
        );
    }

    #[test]
    fn assessment_migration_preserves_saved_receipt_and_unknown_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("review.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let mut input = external_wait(&store);
        let saved = store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        {
            let mut connection = store.connection().unwrap();
            let tx = connection.transaction().unwrap();
            let sql: String = tx
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='queen_task_review_receipts'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let legacy = sql
                .replacen("queen_task_review_receipts", "legacy_receipts", 1)
                .replace(", 'insufficient_evidence'", "")
                .replace(",'insufficient_evidence'", "");
            tx.execute_batch(&legacy).unwrap();
            tx.execute_batch("INSERT INTO legacy_receipts SELECT * FROM queen_task_review_receipts; DROP TABLE queen_task_review_receipts; ALTER TABLE legacy_receipts RENAME TO queen_task_review_receipts; PRAGMA user_version=148;").unwrap();
            tx.commit().unwrap();
        }
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 102)
                .unwrap(),
            saved
        );
        assert_eq!(
            store.queen_review_check_times().unwrap()[&input.task_id],
            101
        );
        input.expected_revision = saved.evidence_revision;
        input.kind = QueenReviewDispositionKind::InsufficientEvidence;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 103)
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store
                .queen_task_review_snapshot(input.task_id)
                .unwrap()
                .previous_assessment
                .unwrap()
                .status,
            swarm_domain::QueenReviewAssessmentStatus::InsufficientEvidence
        );
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
    }

    #[test]
    fn incomplete_assessment_orders_attention_but_never_covers_or_duplicates_history() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        let before = store.get_task(input.task_id).unwrap();
        input.kind = QueenReviewDispositionKind::InsufficientEvidence;
        input.condition = "Original operator statement is not available".into();
        let result = store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        assert!(!result.covers_wait);
        assert_eq!(
            store.queen_review_check_times().unwrap()[&input.task_id],
            101
        );
        for run in [&input.run_id, &TaskId::new().to_string()] {
            assert!(matches!(
                store.queen_run_review_coverage(run).unwrap(),
                QueenReviewCoverage::Missing { .. }
            ));
        }
        let snapshot = store.queen_task_review_snapshot(input.task_id).unwrap();
        assert_eq!(
            snapshot.previous_assessment.unwrap().status,
            swarm_domain::QueenReviewAssessmentStatus::InsufficientEvidence
        );
        let events = store
            .list_task_activity(input.task_id, 100)
            .unwrap()
            .events
            .len();
        let replay = store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 200)
            .unwrap();
        assert_eq!(replay, result);
        assert_eq!(
            store.queen_review_check_times().unwrap()[&input.task_id],
            101
        );
        assert_eq!(
            store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len(),
            events
        );
        let after = store.get_task(input.task_id).unwrap();
        assert_eq!(before.state, after.state);
        assert_eq!(before.next_move_owner, after.next_move_owner);
        assert_eq!(before.assigned_worker_id, after.assigned_worker_id);
        let mut stale = input.clone();
        stale.evidence.push_str(" changed");
        assert!(
            store
                .record_queen_review_disposition(&stale, &TaskActivityActor::operator(), 201)
                .is_err()
        );
    }

    #[test]
    fn incomplete_assessment_replaces_coverage_without_preserving_a_verified_wait() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        assert!(
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
                .unwrap()
                .covers_wait
        );
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Covered { .. }
        ));
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        input.kind = QueenReviewDispositionKind::InsufficientEvidence;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 102)
            .unwrap();
        assert!(matches!(
            store.queen_run_review_coverage(&input.run_id).unwrap(),
            QueenReviewCoverage::Missing { .. }
        ));
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
    fn review_queue_cache_reuses_unchanged_reads_and_invalidates_local_writes() {
        use swarm_domain::QueenReviewAssessmentStatus as Status;
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        assert!(
            store
                .queen_review_queue_snapshot()
                .unwrap()
                .items
                .is_empty()
        );
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        let first = store.queen_review_queue_snapshot().unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(
            first.items[0].previous_assessment.as_ref().unwrap().status,
            Status::CoveredForCurrentRun
        );
        let changes = store.connection().unwrap().total_changes();
        // Instrument the saved read time, not a timer or production task state.
        store
            .review_queue_cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .snapshot
            .checked_at = first.checked_at - 1;
        assert_eq!(
            store.queen_review_queue_snapshot().unwrap().checked_at,
            first.checked_at - 1
        );
        assert_eq!(store.connection().unwrap().total_changes(), changes);
        // A same-second edit with unchanged updated_at still invalidates evidence.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET description='Changed scope' WHERE id=?1",
                [input.task_id.to_string()],
            )
            .unwrap();
        let refreshed = store.queen_review_queue_snapshot().unwrap();
        assert_eq!(
            refreshed.items[0]
                .previous_assessment
                .as_ref()
                .unwrap()
                .status,
            Status::EvidenceChanged
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE queen_task_review_receipts SET input_payload='{}' WHERE task_id=?1",
                [input.task_id.to_string()],
            )
            .unwrap();
        assert!(store.queen_review_queue_snapshot().is_err());
        assert!(store.review_queue_cache.lock().unwrap().is_none());
    }

    #[test]
    fn review_queue_cache_invalidates_external_writes_and_restarts() {
        use swarm_domain::QueenReviewAssessmentStatus as Status;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("review.sqlite");
        let store = TaskStore::open(&path).unwrap();
        let input = external_wait(&store);
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        store.queen_review_queue_snapshot().unwrap();
        let other = rusqlite::Connection::open(&path).unwrap();
        other
            .execute(
                "UPDATE tasks SET description='External writer corrected scope' WHERE id=?1",
                [input.task_id.to_string()],
            )
            .unwrap();
        assert_eq!(
            store.queen_review_queue_snapshot().unwrap().items[0]
                .previous_assessment
                .as_ref()
                .unwrap()
                .status,
            Status::EvidenceChanged
        );
        drop(other);
        drop(store);
        let reopened = TaskStore::open(&path).unwrap();
        assert!(reopened.review_queue_cache.lock().unwrap().is_none());
        assert_eq!(
            reopened.queen_review_queue_snapshot().unwrap().items[0]
                .previous_assessment
                .as_ref()
                .unwrap()
                .status,
            Status::EvidenceChanged
        );
    }

    #[test]
    fn review_queue_cache_deadline_clock_and_recovery_fences_do_not_need_sleep() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.queen_review_queue_snapshot().unwrap();
        {
            let mut cache = store.review_queue_cache.lock().unwrap();
            let saved = cache.as_mut().unwrap();
            saved.next_deadline = Some(0);
            saved.snapshot.checked_at = first.checked_at - 1;
        }
        assert!(store.queen_review_queue_snapshot().unwrap().checked_at >= first.checked_at);
        store
            .review_queue_cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .snapshot
            .checked_at = i64::MAX;
        assert_ne!(
            store.queen_review_queue_snapshot().unwrap().checked_at,
            i64::MAX
        );
        store
            .recovery_required
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            store.queen_review_queue_snapshot(),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
    }

    #[test]
    fn review_queue_bounds_and_observes_cold_and_cached_read_cost() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        for index in 0..MAX_QUEUE_ASSESSMENTS {
            let task = store
                .create_task(&format!("Bounded fixture {index}"), "/workspace/demo")
                .unwrap();
            store.transition_task(task.id, TaskState::Ready).unwrap();
            store
                .transition_task_with_note(task.id, TaskState::Blocked, "External fixture gate")
                .unwrap();
            input.task_id = task.id;
            input.expected_revision = store
                .queen_task_review_evidence(task.id)
                .unwrap()
                .evidence_revision;
            store
                .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
                .unwrap();
        }
        let started = std::time::Instant::now();
        let snapshot = store.queen_review_queue_snapshot().unwrap();
        let cold = started.elapsed();
        assert!(snapshot.truncated);
        assert_eq!(snapshot.items.len(), MAX_QUEUE_ASSESSMENTS);
        let changes = store.connection().unwrap().total_changes();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            let cached = store.queen_review_queue_snapshot().unwrap();
            assert_eq!(cached.checked_at, snapshot.checked_at);
            assert_eq!(cached.items.len(), MAX_QUEUE_ASSESSMENTS);
        }
        eprintln!(
            "review queue: 64 receipts cold={}us; 100 cached reads={}us",
            cold.as_micros(),
            started.elapsed().as_micros()
        );
        assert_eq!(store.connection().unwrap().total_changes(), changes);
        store
            .transition_task(input.task_id, TaskState::Abandoned)
            .unwrap();
        let reduced = store.queen_review_queue_snapshot().unwrap();
        assert!(!reduced.truncated);
        assert!(
            reduced
                .items
                .iter()
                .all(|item| item.task.id != input.task_id)
        );
    }

    #[test]
    fn disposition_accepts_delivered_continuations_but_not_undelivered_or_closed_runs() {
        for (state, delivered, session_present, accepted) in [
            ("running", true, true, true),
            ("uncertain", true, true, true),
            ("queued", true, true, true),
            ("delivering", true, true, true),
            ("queued", false, true, false),
            ("delivering", false, true, false),
            ("queued", true, false, false),
            ("completed", true, true, false),
        ] {
            let store = TaskStore::in_memory().unwrap();
            let input = external_wait(&store);
            store.connection().unwrap().execute(
                "UPDATE queen_automation SET state=?1,
                 delivered_at=CASE WHEN ?2 THEN delivered_at ELSE NULL END,
                 delivery_session_id=CASE WHEN ?3 THEN delivery_session_id ELSE NULL END WHERE id=1",
                rusqlite::params![state, delivered, session_present],
            ).unwrap();
            let before = store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len();
            let result =
                store.record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101);
            assert_eq!(
                result.is_ok(),
                accepted,
                "{state}, delivered={delivered}, session={session_present}: {result:?}"
            );
            let after = store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len();
            assert_eq!(after, before + usize::from(accepted));
        }
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

    /// ⚠️ THE PAIR TO THE TEST BELOW, AND THE REASON THIS IS PRESENTATION RATHER
    /// THAN PERMISSION. Carrying a wait across runs is only safe because changed
    /// evidence still breaks it, and it has to be asserted IN A LATER RUN — the
    /// older test checks that within the recording run, which a change scoped to
    /// runs could satisfy while leaving this broken.
    #[test]
    fn changed_evidence_re_raises_an_external_wait_in_a_later_run() {
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
            QueenReviewCoverage::Covered { .. }
        ));

        store
            .append_task_correction(
                input.task_id,
                "Changed external evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();

        assert!(
            matches!(
                store.queen_run_review_coverage(&next).unwrap(),
                QueenReviewCoverage::Missing { .. }
            ),
            "a wait whose evidence moved is Queen's work again, run or no run"
        );
    }

    #[test]
    fn an_external_claim_survives_a_new_run_but_never_new_facts() {
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 102)
            .unwrap();
        let next = start_review(&store, 103);
        // ⚠️ THIS ASSERTION IS INVERTED FROM WHAT IT WAS, and the rename says so.
        // It used to require a fresh check every run, which is what made Queen
        // re-review unchanged external waits -- 177 passes across 8 receipts,
        // one of them 86 times. The operator answered on 2026-09-18: show it as
        // a hold with Last checked instead. An unchanged wait now carries.
        assert!(
            matches!(
                store.queen_run_review_coverage(&next).unwrap(),
                QueenReviewCoverage::Covered { .. }
            ),
            "an external wait whose evidence has not moved is a hold, not new work"
        );
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
    fn blocked_review_survives_session_replacement_but_not_changed_evidence() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Fixture",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                0,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let mut input = external_wait(&store);
        store
            .assign_task_to_worker(input.task_id, worker.id)
            .unwrap();
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        let before = store.queen_task_review_evidence(input.task_id).unwrap();
        store.release_session_assignments(session).unwrap();
        store.release_worker_session(session).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET updated_at=updated_at+1 WHERE id=?1",
                [input.task_id.to_string()],
            )
            .unwrap();
        assert_eq!(
            before,
            store.queen_task_review_evidence(input.task_id).unwrap()
        );
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        assert_eq!(
            before,
            store.queen_task_review_evidence(input.task_id).unwrap()
        );
        store
            .append_task_correction(
                input.task_id,
                "The external gate changed",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        assert_ne!(
            before,
            store.queen_task_review_evidence(input.task_id).unwrap()
        );
    }

    #[test]
    fn active_review_evidence_remains_session_bound() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Fixture",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                0,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Active fixture", "/workspace/demo")
            .unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        let before = store.queen_task_review_evidence(task.id).unwrap();
        store.release_session_assignments(session).unwrap();
        store.release_worker_session(session).unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
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

    #[test]
    fn a_wait_one_pass_under_the_bound_still_returns_when_its_evidence_moves() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES - 1);
        store
            .append_task_correction(
                input.task_id,
                "Changed external evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 250)
            .unwrap();
        let next = start_review(&store, 300);
        assert!(
            matches!(
                store.queen_run_review_coverage(&next).unwrap(),
                QueenReviewCoverage::Missing { .. }
            ),
            "one pass under the bound, changed evidence is still Queen's work"
        );
    }

    #[test]
    fn a_wait_at_the_bound_stops_returning_even_when_its_evidence_moves() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES);
        store
            .append_task_correction(
                input.task_id,
                "Changed external evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 250)
            .unwrap();
        let next = start_review(&store, 300);
        assert!(
            matches!(
                store.queen_run_review_coverage(&next).unwrap(),
                QueenReviewCoverage::Covered { .. }
            ),
            "at the bound the wait is a hold until the task moves, not a re-review"
        );
    }

    #[test]
    fn a_wait_stopped_by_the_bound_returns_the_moment_the_task_moves() {
        // ⚠️ THE TEST THAT MATTERS MOST. Work dropped and never returning is
        // WORSE than the repetition it replaces, because repetition at least
        // kept the task in front of somebody.
        let store = TaskStore::in_memory().unwrap();
        let mut input = external_wait(&store);
        re_read_without_moving(&store, &mut input, MAX_UNCHANGED_REVIEW_PASSES);
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 250)
            .unwrap();
        let next = start_review(&store, 300);
        assert!(matches!(
            store.queen_run_review_coverage(&next).unwrap(),
            QueenReviewCoverage::Covered { .. }
        ));
        store
            .finish_queen_automation_run(&next, QueenAutomationOutcome::NoAction, 350)
            .unwrap();

        store
            .transition_task(input.task_id, TaskState::Ready)
            .unwrap();
        store
            .transition_task_with_note(
                input.task_id,
                TaskState::Blocked,
                "External condition requires verification",
            )
            .unwrap();
        input.run_id = start_review(&store, 400);
        input.expected_revision = store
            .queen_task_review_evidence(input.task_id)
            .unwrap()
            .evidence_revision;
        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 401)
            .unwrap();
        store
            .append_task_correction(
                input.task_id,
                "Changed external evidence",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        store
            .finish_queen_automation_run(&input.run_id, QueenAutomationOutcome::NoAction, 450)
            .unwrap();
        let after_move = start_review(&store, 500);
        assert!(
            matches!(
                store.queen_run_review_coverage(&after_move).unwrap(),
                QueenReviewCoverage::Missing { .. }
            ),
            "a state change resets the count, so the wait is Queen's work again"
        );
    }
    #[test]
    fn a_park_is_read_live_and_leaves_the_moment_its_task_moves() {
        // ⚠️ THE PROPERTY THAT KEEPS THE SURFACE HONEST. The thing a Parked tab
        // replaces -- being nagged -- went stale by design once the coverage
        // bound landed. A stored flag would go stale the same way, so `park` is
        // derived from the live receipt and a state change retires it with
        // nothing having to clear anything.
        let store = TaskStore::in_memory().unwrap();
        let input = external_wait(&store);
        assert_eq!(
            store.get_task(input.task_id).unwrap().park,
            None,
            "a block with no review disposition is not a park"
        );

        store
            .record_queen_review_disposition(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        assert_eq!(
            store.get_task(input.task_id).unwrap().park,
            Some(swarm_domain::TaskPark::ExternalCondition),
            "an external wait reads as one while it is blocked"
        );

        store
            .transition_task(input.task_id, TaskState::Ready)
            .unwrap();
        assert_eq!(
            store.get_task(input.task_id).unwrap().park,
            None,
            "work that MOVED is not parked, and nothing had to clear a flag"
        );
    }
}
