//! What a blocked task waits for, who must act, and bringing it back when the
//! wait clears (ADR 0119).
//!
//! A wait row is anchored to the activity entry that moved its task into
//! Blocked, as a block reassessment is (ADR 0081). Leaving Blocked writes a new
//! entry, so the wait stops being current without anything deleting it, and
//! the row stays as history.

use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{TaskActivityActor, TaskId, TaskState, TaskWait, TaskWaitKind};

use crate::{ControlRoomEventKind, TaskStore, TaskStoreError, insert_control_room_event};

/// How many tasks one pass brings back. The rest come back on the next pass.
const MAX_RESUMED_PER_PASS: i64 = 64;

pub(crate) fn migrate_task_waits(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_waits (
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             block_sequence INTEGER NOT NULL,
             kind TEXT NOT NULL
                 CHECK (kind IN ('task','decision','operator_action','outside','date')),
             needs TEXT NOT NULL CHECK (length(needs) BETWEEN 1 AND 1000),
             party TEXT CHECK (party IS NULL OR length(party) <= 120),
             resume_at INTEGER,
             created_at INTEGER NOT NULL,
             actor_kind TEXT NOT NULL,
             actor_id TEXT,
             done_at INTEGER,
             PRIMARY KEY (task_id, block_sequence),
             CHECK ((kind IN ('outside','date')) = (resume_at IS NOT NULL)),
             CHECK ((kind = 'outside') = (party IS NOT NULL)),
             CHECK (kind = 'operator_action' OR done_at IS NULL)
         );
         CREATE INDEX IF NOT EXISTS task_waits_due ON task_waits(resume_at)
             WHERE resume_at IS NOT NULL;",
    )?;
    // The work already on the board (Decision 7). Each open block gets the
    // wait its record can support, against its current block entry. A date
    // wins over a park because it is the more specific promise. Anything left
    // is untyped and stays Queen's.
    transaction.execute_batch(&format!(
        "INSERT OR IGNORE INTO task_waits
             (task_id, block_sequence, kind, needs, party, resume_at, created_at, actor_kind)
         SELECT id, block_entry, kind, needs,
                CASE WHEN kind = 'outside' THEN 'an outside party' END,
                CASE WHEN kind = 'date' THEN blocked_until
                     WHEN kind = 'outside' THEN unixepoch() + 86400 END,
                unixepoch(), 'system'
         FROM (
             SELECT t.id, {block_entry} AS block_entry, t.blocked_until,
                    CASE
                      WHEN t.blocked_until > unixepoch() THEN 'date'
                      WHEN {open_prerequisite} THEN 'task'
                      WHEN {pending_decision} THEN 'decision'
                      WHEN r.kind = 'external_condition' THEN 'outside'
                      WHEN r.kind = 'operator_deferral' THEN 'operator_action'
                    END AS kind,
                    substr(coalesce(
                        (SELECT '\"' || upstream.title || '\" finishes.' FROM task_prerequisites p
                         JOIN tasks upstream ON upstream.id = p.prerequisite_id
                         WHERE p.task_id = t.id AND upstream.removed_at IS NULL
                           AND upstream.state NOT IN ('completed','abandoned')
                         ORDER BY p.created_at LIMIT 1),
                        (SELECT 'The operator answers \"' || d.title || '\".' FROM task_decision_membership m
                         JOIN decision_requests d ON d.id = m.decision_id
                         WHERE m.task_id = t.id AND d.state = 'pending'
                         ORDER BY d.created_at LIMIT 1),
                        NULLIF(json_extract(r.input_payload, '$.condition'), ''),
                        CASE WHEN t.blocked_until > unixepoch()
                             THEN 'Nothing can happen until this date.' END,
                        'Recorded from the block this task was already in.'), 1, 1000) AS needs
             FROM tasks t
             LEFT JOIN queen_task_review_receipts r
               ON r.task_id = t.id AND r.kind IN ('operator_deferral','external_condition')
             WHERE t.state = 'blocked' AND t.removed_at IS NULL
         )
         WHERE kind IS NOT NULL AND block_entry IS NOT NULL;",
        block_entry = BLOCK_ENTRY,
        open_prerequisite = "EXISTS(SELECT 1 FROM task_prerequisites p
             JOIN tasks upstream ON upstream.id = p.prerequisite_id
             WHERE p.task_id = t.id AND upstream.removed_at IS NULL
               AND upstream.state NOT IN ('completed','abandoned'))",
        pending_decision = "EXISTS(SELECT 1 FROM task_decision_membership m
             JOIN decision_requests d ON d.id = m.decision_id
             WHERE m.task_id = t.id AND d.state = 'pending')",
    ))?;
    transaction.pragma_update(None, "user_version", crate::TASK_WAITS_SCHEMA_MARKER)
}

/// Admits a wait on Queen, and brings the blocks already parked on her into
/// her queue (ADR 0166).
///
/// The table is rebuilt because its CHECKs name the kinds. Then the current
/// wait of each open block whose party is Queen alone becomes hers: no party
/// and no recheck, its words untouched. Waits on blocks the task has since
/// left are history and stay as they were written.
pub(crate) fn migrate_queen_waits(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE task_waits_v224 (
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             block_sequence INTEGER NOT NULL,
             kind TEXT NOT NULL
                 CHECK (kind IN ('task','decision','operator_action','outside','date','queen')),
             needs TEXT NOT NULL CHECK (length(needs) BETWEEN 1 AND 1000),
             party TEXT CHECK (party IS NULL OR length(party) <= 120),
             resume_at INTEGER,
             created_at INTEGER NOT NULL,
             actor_kind TEXT NOT NULL,
             actor_id TEXT,
             done_at INTEGER,
             PRIMARY KEY (task_id, block_sequence),
             CHECK ((kind IN ('outside','date')) = (resume_at IS NOT NULL)),
             CHECK ((kind = 'outside') = (party IS NOT NULL)),
             CHECK (kind IN ('operator_action','queen') OR done_at IS NULL)
         );
         -- Named, not `SELECT *`, so a column added later cannot shift one.
         INSERT INTO task_waits_v224
                 (task_id, block_sequence, kind, needs, party, resume_at,
                  created_at, actor_kind, actor_id, done_at)
             SELECT task_id, block_sequence, kind, needs, party, resume_at,
                    created_at, actor_kind, actor_id, done_at
             FROM task_waits;
         DROP TABLE task_waits;
         ALTER TABLE task_waits_v224 RENAME TO task_waits;
         CREATE INDEX task_waits_due ON task_waits(resume_at)
             WHERE resume_at IS NOT NULL;",
    )?;
    let open_block = format!(
        "kind = 'outside' AND lower(trim(party)) = 'queen'
         AND block_sequence = (SELECT {BLOCK_ENTRY} FROM tasks t
             WHERE t.id = task_waits.task_id AND t.state = 'blocked'
               AND t.removed_at IS NULL)"
    );
    // Said in each task's history, so a reader can tell why a task that was
    // due back at its Recheck is in Queen's queue instead.
    transaction.execute(
        &format!(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             SELECT task_id, 'noted', ?1, 'system', NULL
             FROM task_waits WHERE {open_block}"
        ),
        [QUEEN_WAIT_MIGRATED],
    )?;
    transaction.execute(
        &format!(
            "UPDATE task_waits SET kind = 'queen', party = NULL, resume_at = NULL
             WHERE {open_block}"
        ),
        [],
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// What a task parked on Queen by an earlier release says when it moves.
const QUEEN_WAIT_MIGRATED: &str = "Swarm read this block as waiting on Queen, who is on the \
     board: it is in her queue until she answers on it, and no longer comes back at its \
     Recheck. Its words are unchanged.";

/// The activity entry that put task `t` into its current state.
macro_rules! wait_block_entry {
    () => {
        "(SELECT max(entry.sequence) FROM task_activity entry
          WHERE entry.task_id = t.id AND entry.kind = 'state_changed')"
    };
}
pub(crate) use wait_block_entry;

/// Whether task `t`'s current block was entered from Draft (ADR 0152). A draft
/// was never made ready to start, so when its wait clears it goes back to
/// Queen: nothing brings it back by itself and no answer asks the operator to
/// release it. Read from the activity row that entered the block, so nothing
/// new is stored. A Jira status that moves a draft straight to Blocked writes
/// the same row outside the lifecycle, and is left as it was before.
macro_rules! block_entered_from_draft {
    () => {
        concat!(
            "EXISTS(SELECT 1 FROM task_activity entered WHERE entered.sequence = ",
            crate::task_waits::wait_block_entry!(),
            " AND entered.from_state = 'draft' AND entered.actor_kind IS NOT 'jira')"
        )
    };
}
pub(crate) use block_entered_from_draft;

/// Whether task `t`'s structured gates are clear: every prerequisite done and
/// no linked decision pending.
macro_rules! wait_gates_clear {
    () => {
        "NOT EXISTS(SELECT 1 FROM task_prerequisites p
             LEFT JOIN tasks upstream ON upstream.id = p.prerequisite_id
             WHERE p.task_id = t.id AND (upstream.id IS NULL
                 OR upstream.removed_at IS NOT NULL OR upstream.state != 'completed'))
         AND NOT EXISTS(SELECT 1 FROM task_decision_membership m
             JOIN decision_requests d ON d.id = m.decision_id
             WHERE m.task_id = t.id AND d.state = 'pending')"
    };
}
pub(crate) use wait_gates_clear;

/// The current wait on task `t` as JSON, for the task projection. Only while
/// blocked, and only the row anchored to the current block entry. One
/// definition, shared by the projection and by `resume_cleared_waits`, so what
/// the board calls cleared is exactly what comes back.
macro_rules! current_wait_json {
    () => {
        concat!(
            "(SELECT json_object('kind', w.kind, 'needs', w.needs, 'party', w.party,
                 'resume_at', w.resume_at, 'done_at', w.done_at,
                 'cleared', json(CASE WHEN ",
            crate::task_waits::wait_cleared!(),
            " THEN 'true' ELSE 'false' END),
                 'entered_from_draft', json(CASE WHEN ",
            crate::task_waits::block_entered_from_draft!(),
            " THEN 'true' ELSE 'false' END))
              FROM task_waits w
              WHERE t.state = 'blocked' AND w.task_id = t.id AND w.block_sequence = ",
            crate::task_waits::wait_block_entry!(),
            ")"
        )
    };
}
pub(crate) use current_wait_json;

/// Whether wait `w` on task `t` has cleared now.
macro_rules! wait_cleared {
    () => {
        concat!(
            "((",
            crate::task_waits::wait_gates_clear!(),
            ") AND (w.kind IN ('task','decision')
                OR (w.kind IN ('operator_action','queen') AND w.done_at IS NOT NULL)
                OR (w.kind IN ('outside','date') AND w.resume_at <= unixepoch())))"
        )
    };
}
pub(crate) use wait_cleared;

/// Whether a typed wait still holds the task aliased `$task` at `$now`: an
/// operator action not yet done, or an outside or date wait before its time.
/// Waits on a task or a decision are left to the gates themselves, which every
/// caller already reads. One definition for Queen's wake count and the two
/// blocked-work alarms, which exist to catch work nobody owns, and a typed wait
/// names its owner.
macro_rules! typed_wait_holds {
    ($task:literal, $now:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM task_waits wait WHERE wait.task_id = ",
            $task,
            ".id AND wait.block_sequence = (SELECT max(entry.sequence) FROM task_activity entry
                 WHERE entry.task_id = ",
            $task,
            ".id AND entry.kind = 'state_changed')
               AND ((wait.kind = 'operator_action' AND wait.done_at IS NULL)
                 OR (wait.kind IN ('outside','date') AND wait.resume_at > ",
            $now,
            ")))"
        )
    };
}
pub(crate) use typed_wait_holds;

/// Whether the task aliased `$task` waits on Queen's answer now (ADR 0166).
///
/// Deliberately NOT part of `typed_wait_holds`: a wait on Queen names her, so
/// it wakes her, and the alarms that catch blocked work nobody acts on still
/// count it. Her silence escalates instead of resting on a timer.
macro_rules! queen_wait_holds {
    ($task:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM task_waits wait WHERE wait.task_id = ",
            $task,
            ".id AND wait.kind = 'queen' AND wait.done_at IS NULL
               AND wait.block_sequence = (SELECT max(entry.sequence) FROM task_activity entry
                 WHERE entry.task_id = ",
            $task,
            ".id AND entry.kind = 'state_changed'))"
        )
    };
}
pub(crate) use queen_wait_holds;

/// Queen has answered a blocked task's worker on it, which is what a wait on
/// her waits for (ADR 0166). Records her answer on the current wait, in the
/// transaction that records her message, and says so in the history. The
/// coordination pass brings the task back.
///
/// # Errors
/// Returns database failures.
pub(crate) fn answer_queen_wait(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    now: i64,
) -> Result<bool, TaskStoreError> {
    let changed = transaction.execute(
        &format!(
            "UPDATE task_waits SET done_at = ?2
             WHERE task_id = ?1 AND kind = 'queen' AND done_at IS NULL
               AND block_sequence = (SELECT {BLOCK_ENTRY} FROM tasks t
                   WHERE t.id = ?1 AND t.state = 'blocked' AND t.removed_at IS NULL)"
        ),
        params![task_id.to_string(), now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    // Queen is who acted, so the history names her profile, as it does for
    // every other move she makes.
    transaction.execute(
        &format!(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             SELECT ?1, 'noted', 'Queen answered: ' || w.needs,
                    CASE WHEN queen.id IS NULL THEN 'system' ELSE 'worker' END, queen.id
             FROM task_waits w JOIN tasks t ON t.id = w.task_id
             LEFT JOIN (SELECT id FROM worker_profiles
                        WHERE role = 'queen' AND archived_at IS NULL LIMIT 1) queen ON 1
             WHERE w.task_id = ?1 AND w.block_sequence = {BLOCK_ENTRY}"
        ),
        [task_id.to_string()],
    )?;
    insert_control_room_event(transaction, ControlRoomEventKind::TasksChanged)?;
    Ok(true)
}

const BLOCK_ENTRY: &str = wait_block_entry!();

/// Records the wait a block names, against the entry that made the block.
///
/// A block that names no wait in its note but has a live gate (an open
/// prerequisite or a pending decision) waits on that gate, and says so. A block
/// with neither is untyped and records nothing.
pub(crate) fn record_block_wait(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    block_sequence: i64,
    wait: Option<&TaskWait>,
    actor: &TaskActivityActor,
) -> Result<(), TaskStoreError> {
    let (kind, needs, party, resume_at) = if let Some(wait) = wait {
        (
            wait.kind,
            wait.needs.clone(),
            wait.party.clone(),
            wait.resume_at,
        )
    } else if let Some(gate) = live_gate(transaction, task_id)? {
        gate
    } else {
        return Ok(());
    };
    transaction.execute(
        "INSERT INTO task_waits
             (task_id, block_sequence, kind, needs, party, resume_at, created_at, actor_kind, actor_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), ?7, ?8)",
        params![
            task_id.to_string(),
            block_sequence,
            kind.as_str(),
            needs,
            party,
            resume_at,
            actor.kind.to_string(),
            actor.id.as_deref(),
        ],
    )?;
    Ok(())
}

/// Records the wait a Blocked NOTE names.
///
/// ⚠️ A PENDING DECISION ON THE TASK OUTRANKS "Waiting on: operator". The
/// operator's part is answering it, and Needs you already shows the decision
/// with its answers. Recorded as an operator action it showed a second time, as
/// a to-do whose Done answered nothing, and answering the decision did not
/// bring the task back. On 2026-10-21 Platform blocked on "choose whether a
/// duplicated product starts Hidden … in decision 01a16084"; the operator
/// answered it twelve minutes later and the task sat Blocked behind a card for
/// a question already answered. A worker that also owes the operator a
/// separate action names it again when the task comes back.
pub(crate) fn record_note_wait(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    block_sequence: i64,
    wait: Option<&TaskWait>,
    actor: &TaskActivityActor,
) -> Result<(), TaskStoreError> {
    let answers_a_decision = wait.is_some_and(|wait| wait.kind == TaskWaitKind::OperatorAction)
        && transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_decision_membership m
                 JOIN decision_requests d ON d.id = m.decision_id
                 WHERE m.task_id = ?1 AND d.state = 'pending')",
            [task_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
    let wait = if answers_a_decision { None } else { wait };
    record_block_wait(transaction, task_id, block_sequence, wait, actor)
}

/// A gate linked to a task that is ALREADY blocked, with no wait recorded for
/// that block, becomes what the block waits on. Queen linking a decision to a
/// block a worker made in prose is the case: from then on the board knows what
/// ends the wait, and the task comes back when it does.
pub(crate) fn type_untyped_block(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    actor: &TaskActivityActor,
) -> Result<(), TaskStoreError> {
    let block_entry: Option<i64> = transaction
        .query_row(
            &format!(
                "SELECT {BLOCK_ENTRY} FROM tasks t
                 WHERE t.id = ?1 AND t.state = 'blocked' AND t.removed_at IS NULL
                   AND NOT EXISTS(SELECT 1 FROM task_waits w
                       WHERE w.task_id = t.id AND w.block_sequence = {BLOCK_ENTRY})"
            ),
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    match block_entry {
        Some(sequence) => record_block_wait(transaction, task_id, sequence, None, actor),
        None => Ok(()),
    }
}

type GateWait = (TaskWaitKind, String, Option<String>, Option<i64>);

fn live_gate(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<Option<GateWait>, TaskStoreError> {
    let prerequisite: Option<String> = transaction
        .query_row(
            "SELECT upstream.title FROM task_prerequisites p
             JOIN tasks upstream ON upstream.id = p.prerequisite_id
             WHERE p.task_id = ?1 AND upstream.removed_at IS NULL
               AND upstream.state NOT IN ('completed','abandoned')
             ORDER BY p.created_at LIMIT 1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(title) = prerequisite {
        return Ok(Some((
            TaskWaitKind::Task,
            bounded(&format!("\"{title}\" finishes.")),
            None,
            None,
        )));
    }
    // A command card Queen may grant is hers to answer first (ADR 0158); the
    // operator can still answer it themselves.
    let decision: Option<(String, bool)> = transaction
        .query_row(
            concat!(
                "SELECT d.title, ",
                crate::queen_grants::queen_card!("d"),
                " FROM task_decision_membership m
                 JOIN decision_requests d ON d.id = m.decision_id
                 WHERE m.task_id = ?1 AND d.state = 'pending'
                 ORDER BY d.created_at LIMIT 1"
            ),
            [task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(decision.map(|(title, queens)| {
        let needs = if queens {
            format!("Queen answers \"{title}\", or the operator does.")
        } else {
            format!("The operator answers \"{title}\".")
        };
        (TaskWaitKind::Decision, bounded(&needs), None, None)
    }))
}

fn bounded(value: &str) -> String {
    if value.len() <= 1000 {
        return value.to_owned();
    }
    let mut end = 1000;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// What one pass of `resume_cleared_waits` did.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResumedWaits {
    /// Tasks moved from Blocked to Ready.
    pub resumed: Vec<TaskId>,
    /// Tasks whose wait cleared but that could not move yet, and why.
    pub held: Vec<(TaskId, String)>,
}

/// A wait the operator owes, as Needs you shows it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OperatorWait {
    pub task_id: TaskId,
    pub task_title: String,
    pub worker_name: Option<String>,
    pub needs: String,
    pub waiting_since: i64,
}

impl TaskStore {
    /// Every operator action still owed, oldest first (ADR 0119 Decision 3).
    ///
    /// # Errors
    /// Returns database failures.
    pub fn operator_waits(&self) -> Result<Vec<OperatorWait>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT t.id, t.title, worker.name, w.needs, w.created_at
             FROM tasks t
             JOIN task_waits w ON w.task_id = t.id AND w.block_sequence = {BLOCK_ENTRY}
             LEFT JOIN worker_profiles worker ON worker.id = t.assigned_worker_id
             WHERE t.state = 'blocked' AND t.removed_at IS NULL
               AND w.kind = 'operator_action' AND w.done_at IS NULL
             ORDER BY w.created_at, t.id LIMIT 64"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut waits = Vec::new();
        for row in rows {
            let (task_id, task_title, worker_name, needs, waiting_since) = row?;
            waits.push(OperatorWait {
                task_id: TaskId::from_str(&task_id)
                    .map_err(|_| TaskStoreError::IntegrityFailure("task id".into()))?,
                task_title,
                worker_name,
                needs,
                waiting_since,
            });
        }
        Ok(waits)
    }

    /// The operator has done what a task was waiting on them for.
    ///
    /// Records Done on the current operator action, removes the operator-owed
    /// park receipt if one exists (ADR 0106, so the next unrelated block is not
    /// read as a park), and brings the task back if nothing else holds it.
    ///
    /// # Errors
    /// Refuses a task that owes the operator nothing, and returns database failures.
    pub fn complete_operator_wait(&self, task_id: TaskId, now: i64) -> Result<(), TaskStoreError> {
        {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                &format!(
                    "UPDATE task_waits SET done_at = ?2
                     WHERE task_id = ?1 AND kind = 'operator_action' AND done_at IS NULL
                       AND block_sequence = (SELECT {BLOCK_ENTRY} FROM tasks t
                           WHERE t.id = ?1 AND t.state = 'blocked' AND t.removed_at IS NULL)"
                ),
                params![task_id.to_string(), now],
            )?;
            if changed == 0 {
                return Err(TaskStoreError::NotFound);
            }
            // The operator is who acted, so the history says so in its own
            // line, not only in the system's note when the task comes back.
            transaction.execute(
                &format!(
                    "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
                     SELECT ?1, 'noted', 'Done: ' || w.needs, 'operator', NULL
                     FROM task_waits w JOIN tasks t ON t.id = w.task_id
                     WHERE w.task_id = ?1 AND w.block_sequence = {BLOCK_ENTRY}"
                ),
                [task_id.to_string()],
            )?;
            transaction.execute(
                "DELETE FROM queen_task_review_receipts
                 WHERE task_id = ?1 AND kind = 'operator_deferral'",
                [task_id.to_string()],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
            transaction.commit()?;
        }
        // Back now if nothing else holds it. A task that cannot move yet is
        // left for the coordination pass, which reports why.
        self.resume_cleared_waits(now)?;
        Ok(())
    }

    /// Brings back every blocked task whose recorded wait has cleared
    /// (ADR 0119 Decision 4).
    ///
    /// Each moves from Blocked to Ready through the ordinary transition, as the
    /// system, with a note saying what cleared. That transition re-arms the
    /// worker's briefing, which then waits behind any Active task as usual.
    /// Only a wait recorded with the CURRENT block can bring a task back, and
    /// only when its gates are clear too; an untyped block never comes back by
    /// itself. A task that has already left Blocked is not a candidate, so this
    /// is safe to run on every pass and after every event that can clear a wait.
    /// Nor is a block entered from Draft: a draft was never made ready to start,
    /// so when its wait clears it is Queen's to move (ADR 0152).
    ///
    /// Runs in the coordination pass: every 30 seconds, and straight after a
    /// decision is answered or Done is pressed. A wait on Queen that she has
    /// answered comes back on the next pass (ADR 0166).
    ///
    /// # Errors
    /// Returns database failures. A task that cannot move is reported in
    /// `held` and stays for the next pass rather than failing the rest.
    pub fn resume_cleared_waits(&self, now: i64) -> Result<ResumedWaits, TaskStoreError> {
        let due: Vec<(String, String, String, Option<String>)> = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(&format!(
                "SELECT t.id, w.kind, w.needs, w.party
                 FROM tasks t
                 JOIN task_waits w ON w.task_id = t.id AND w.block_sequence = {BLOCK_ENTRY}
                 WHERE t.state = 'blocked' AND t.removed_at IS NULL
                   AND ({gates})
                   AND (w.kind IN ('task','decision')
                     OR (w.kind IN ('operator_action','queen') AND w.done_at IS NOT NULL)
                     OR (w.kind IN ('outside','date') AND w.resume_at <= ?1))
                   AND NOT {from_draft}
                 ORDER BY t.position, t.id LIMIT ?2",
                gates = wait_gates_clear!(),
                from_draft = block_entered_from_draft!(),
            ))?;
            statement
                .query_map(params![now, MAX_RESUMED_PER_PASS], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<_, _>>()?
        };
        let mut outcome = ResumedWaits::default();
        for (task_id, kind, needs, party) in due {
            let id = TaskId::from_str(&task_id)
                .map_err(|_| TaskStoreError::IntegrityFailure("task id".into()))?;
            let note = match kind.as_str() {
                "task" => "Came back by itself: every task it was waiting on is complete.".to_owned(),
                "decision" => "Came back by itself: the operator answered the decision it was waiting on. Read the answer before going on.".to_owned(),
                "operator_action" => format!("Came back by itself: the operator pressed Done on \"{needs}\"."),
                "queen" => format!("Came back by itself: Queen answered on this task about \"{needs}\". Read her message before going on."),
                "outside" => format!(
                    "Came back by itself to be rechecked: is \"{needs}\" done yet? It was waiting on {}. \
                     If not, block it again with a new Recheck time.",
                    party.as_deref().unwrap_or("an outside party")
                ),
                _ => format!("Came back by itself: the date it was waiting for has arrived. {needs}"),
            };
            match self.transition_task_inner(
                id,
                TaskState::Ready,
                &note,
                None,
                &TaskActivityActor::system(),
            ) {
                Ok(_) => outcome.resumed.push(id),
                Err(error) => outcome.held.push((id, error.to_string())),
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{NextMoveOwner, ProviderKind, TaskWaitKind, WorkerSessionId};

    fn now(store: &TaskStore) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .unwrap()
    }

    /// A store with a running worker holding one Active task.
    fn active_task(title: &str) -> (TaskStore, TaskId) {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "RCG Networks",
                ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        let task = store.create_task(title, "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        (store, task.id)
    }

    fn queued_briefings(store: &TaskStore, task: TaskId) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_dispatches WHERE task_id = ?1 AND state = 'queued'",
                [task.to_string()],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// The Spectrum outage was re-read by Queen every fifteen minutes for a
    /// day. Named as an outside wait, it rests until its recheck time and then
    /// comes back to its worker, briefed, to be checked.
    #[test]
    fn an_outside_wait_rests_until_its_recheck_and_then_comes_back_to_its_worker() {
        let (store, task) = active_task("Spectrum outage");
        let recheck = now(&store) + 2 * 86_400;
        let blocked = store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                &format!(
                    "CityLink NAT is enabled.\nWaiting on: Spectrum\nNeeds: X4 back to Available\nRecheck: {}",
                    swarm_domain::format_rfc3339_seconds(recheck)
                ),
            )
            .unwrap();
        let wait = blocked.wait.clone().expect("the block names a wait");
        assert_eq!(wait.kind, TaskWaitKind::Outside);
        assert_eq!(wait.party.as_deref(), Some("Spectrum"));
        assert_eq!(wait.resume_at, Some(recheck));
        assert!(!wait.cleared);
        assert_eq!(blocked.next_move_owner, NextMoveOwner::Blocked);

        assert!(
            store
                .resume_cleared_waits(recheck - 1)
                .unwrap()
                .resumed
                .is_empty()
        );
        // The worker read its first briefing long ago.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_dispatches SET state = 'delivered', delivered_at = unixepoch()
                 WHERE task_id = ?1",
                [task.to_string()],
            )
            .unwrap();
        assert_eq!(queued_briefings(&store, task), 0);

        let outcome = store.resume_cleared_waits(recheck).unwrap();
        assert_eq!(outcome.resumed, [task]);
        let back = store.get_task(task).unwrap();
        assert_eq!(back.state, TaskState::Ready);
        assert!(
            back.assigned_worker_id.is_some(),
            "it goes back to the same worker"
        );
        assert_eq!(back.wait, None, "leaving Blocked ends the wait");
        assert_eq!(
            queued_briefings(&store, task),
            1,
            "and the worker is told again"
        );
        let note: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT note FROM task_activity WHERE task_id = ?1 ORDER BY sequence DESC LIMIT 1",
                [task.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            note.contains("rechecked") && note.contains("Spectrum"),
            "{note}"
        );

        // Already back, so another pass moves nothing.
        assert!(
            store
                .resume_cleared_waits(recheck + 60)
                .unwrap()
                .resumed
                .is_empty()
        );
    }

    /// The form older briefs taught for Queen: an outside wait with a Recheck.
    /// It was recorded as an outside party's, which is nobody's move, so the
    /// task sat in no queue until the timer sent it back to a worker who found
    /// nothing changed (ADR 0166). It is refused now, with the form that
    /// works, so no block can rest that way again.
    #[test]
    fn the_form_older_briefs_taught_for_queen_no_longer_leaves_a_task_to_nobody() {
        let (store, task) = active_task("Payments B1");
        let blocked = store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                &format!(
                    "Draft PR #12 is ready.\nWaiting on: Queen\nNeeds: her GO on the exact head\nRecheck: {}",
                    swarm_domain::format_rfc3339_seconds(now(&store) + 2 * 86_400)
                ),
            )
            .map(|task| task.next_move_owner);
        let Err(TaskStoreError::InvalidTaskWait(said)) = blocked else {
            panic!("a wait on Queen was left to nobody: {blocked:?}");
        };
        assert!(said.contains("Waiting on: Queen"), "{said}");
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Active);
    }

    /// Named alone, Queen is the party. The task is in her queue, and it wakes
    /// her, until she answers its worker on it. Her answer brings it back.
    #[test]
    fn a_block_on_queen_is_in_her_queue_until_she_answers_on_it() {
        let (store, task) = active_task("Payments B1");
        let queen = store.ensure_queen("/workspace").unwrap();
        let worker = store.get_task(task).unwrap().assigned_worker_id.unwrap();
        // The clock is read first: an in-memory store has one connection.
        let wakes = |store: &TaskStore| {
            let at = now(store);
            let connection = store.connection().unwrap();
            crate::queen_conductor::actionable_count(&connection, at).unwrap()
        };
        let before = wakes(&store);
        let blocked = store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                "Draft PR #12 is ready.\nWaiting on: Queen\nNeeds: her GO on the exact head",
            )
            .unwrap();
        let wait = blocked.wait.clone().expect("the block names a wait");
        assert_eq!(wait.kind.as_str(), "queen");
        assert_eq!(wait.needs, "her GO on the exact head");
        assert!(!wait.cleared);
        assert_eq!(blocked.next_move_owner, NextMoveOwner::Queen);
        assert_eq!(wakes(&store), before + 1, "it wakes Queen");
        assert!(
            store
                .resume_cleared_waits(i64::MAX / 2)
                .unwrap()
                .resumed
                .is_empty(),
            "no amount of time answers for her"
        );
        // Her silence escalates, as an unattended block does, which a wait
        // naming anyone else never does: four hours to the coordinator, twelve
        // to the operator.
        let a_day_later = now(&store) + 86_400;
        let unattended = store
            .unattended_block_candidates(a_day_later, 4 * 3_600)
            .unwrap();
        assert!(unattended.iter().any(|candidate| candidate.task_id == task));
        let escalated = store
            .operator_block_escalation_candidates(a_day_later, 12 * 3_600)
            .unwrap();
        assert!(escalated.iter().any(|candidate| candidate.task_id == task));

        store
            .send_queen_worker_message(
                task,
                worker,
                swarm_domain::WorkerMessagePurpose::AssignedTask,
                "GO on abc1234.",
                now(&store),
            )
            .unwrap();

        let answered = store.get_task(task).unwrap();
        assert!(answered.wait.as_ref().is_some_and(|wait| wait.cleared));
        assert_eq!(answered.next_move_owner, NextMoveOwner::Worker);
        let outcome = store.resume_cleared_waits(now(&store)).unwrap();
        assert_eq!(outcome.resumed, [task]);
        let back = store.get_task(task).unwrap();
        assert_eq!(back.state, TaskState::Ready);
        assert_eq!(back.assigned_worker_id, Some(worker));
        let notes: Vec<(Option<String>, String)> = {
            let connection = store.connection().unwrap();
            let mut statement = connection
                .prepare(
                    "SELECT actor_id, note FROM task_activity
                     WHERE task_id = ?1 AND note != '' ORDER BY sequence DESC LIMIT 2",
                )
                .unwrap();
            statement
                .query_map([task.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert!(notes[0].1.contains("Queen answered"), "{notes:?}");
        assert_eq!(
            notes[1].0,
            Some(queen.id.to_string()),
            "the history names Queen as who acted"
        );
        assert!(notes[1].1.contains("her GO on the exact head"), "{notes:?}");
    }

    /// Four tasks waited on the operator while Needs you was empty.
    #[test]
    fn an_operator_action_is_owed_in_needs_you_until_done() {
        let (store, task) = active_task("SOA Phase E");
        let blocked = store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                "Waiting on: operator\nNeeds: Sign in to the Entra portal and approve M2",
            )
            .unwrap();
        assert_eq!(blocked.next_move_owner, NextMoveOwner::Operator);
        let owed = store.operator_waits().unwrap();
        assert_eq!(owed.len(), 1);
        assert_eq!(owed[0].task_id, task);
        assert_eq!(owed[0].needs, "Sign in to the Entra portal and approve M2");
        assert_eq!(owed[0].worker_name.as_deref(), Some("RCG Networks"));
        assert!(
            store
                .resume_cleared_waits(i64::MAX / 2)
                .unwrap()
                .resumed
                .is_empty(),
            "no amount of time clears an operator action"
        );

        store.complete_operator_wait(task, now(&store)).unwrap();

        let done_by: (String, String) = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT actor_kind, note FROM task_activity
                 WHERE task_id = ?1 AND kind = 'noted' ORDER BY sequence DESC LIMIT 1",
                [task.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            done_by,
            (
                "operator".to_owned(),
                "Done: Sign in to the Entra portal and approve M2".to_owned()
            ),
            "the history names the operator as who acted"
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Ready);
        assert!(store.operator_waits().unwrap().is_empty());
        assert!(matches!(
            store.complete_operator_wait(task, now(&store)),
            Err(TaskStoreError::NotFound)
        ));
    }

    /// A wait on Queen is hers whatever else holds the task, as its next move
    /// says, so an unfinished prerequisite does not stop it waking her
    /// (ADR 0166).
    #[test]
    fn a_wait_on_queen_behind_an_unfinished_prerequisite_still_wakes_her() {
        let (store, task) = active_task("Payments B2");
        let upstream = store.create_task("Payments B1", "/workspace").unwrap();
        store
            .transition_task(upstream.id, TaskState::Ready)
            .unwrap();
        store
            .add_task_prerequisite(
                task,
                upstream.id,
                "B2 follows B1",
                &TaskActivityActor::operator(),
                1,
            )
            .unwrap();
        let wakes = |store: &TaskStore| {
            let at = now(store);
            let connection = store.connection().unwrap();
            crate::queen_conductor::actionable_count(&connection, at).unwrap()
        };
        let before = wakes(&store);
        let blocked = store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                "Waiting on: Queen\nNeeds: whether B2 may start before B1 merges",
            )
            .unwrap();
        assert_eq!(blocked.next_move_owner, NextMoveOwner::Queen);
        assert_eq!(wakes(&store), before, "it still wakes her");
    }

    #[test]
    fn a_block_on_another_task_comes_back_when_that_task_completes() {
        let (store, task) = active_task("SOA Phase F");
        let upstream = store.create_task("SOA Phase E", "/workspace").unwrap();
        store
            .transition_task(upstream.id, TaskState::Ready)
            .unwrap();
        store
            .add_task_prerequisite(
                task,
                upstream.id,
                "F follows E",
                &TaskActivityActor::operator(),
                1,
            )
            .unwrap();
        let blocked = store
            .transition_task_with_note(task, TaskState::Blocked, "Waiting for Phase E.")
            .unwrap();
        assert_eq!(blocked.wait.map(|wait| wait.kind), Some(TaskWaitKind::Task));
        // Not even attempted: a wait whose gate is open is not due, rather
        // than due and refused on every pass.
        assert_eq!(
            store.resume_cleared_waits(now(&store)).unwrap(),
            ResumedWaits::default()
        );

        store
            .transition_task(upstream.id, TaskState::Active)
            .unwrap();
        store
            .transition_task(upstream.id, TaskState::Review)
            .unwrap();
        store
            .transition_task(upstream.id, TaskState::Completed)
            .unwrap();

        assert_eq!(
            store.resume_cleared_waits(now(&store)).unwrap().resumed,
            [task]
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Ready);
    }

    /// A gate outlives the block it was for. A prerequisite finished last
    /// month must not bring back a task blocked today for a reason nobody
    /// recorded, so only a wait recorded with the CURRENT block counts.
    #[test]
    fn a_finished_gate_does_not_bring_back_a_block_it_was_not_recorded_for() {
        let (store, task) = active_task("Revert the gateway");
        let finished = store.create_task("Earlier work", "/workspace").unwrap();
        store
            .add_task_prerequisite(
                task,
                finished.id,
                "earlier",
                &TaskActivityActor::operator(),
                1,
            )
            .unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(finished.id, state).unwrap();
        }
        let blocked = store
            .transition_task_with_note(task, TaskState::Blocked, "Waiting for the hosts file.")
            .unwrap();

        assert_eq!(blocked.wait, None, "a prose block is untyped");
        assert_eq!(blocked.next_move_owner, NextMoveOwner::Queen);
        assert!(
            store
                .resume_cleared_waits(now(&store))
                .unwrap()
                .resumed
                .is_empty()
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Blocked);
    }

    #[test]
    fn a_wait_named_wrongly_refuses_the_block_and_writes_nothing() {
        let (store, task) = active_task("Spectrum outage");
        let refused = store
            .transition_task_with_note(task, TaskState::Blocked, "Waiting on: Spectrum")
            .unwrap_err();
        assert!(matches!(refused, TaskStoreError::InvalidTaskWait(_)));
        assert!(refused.to_string().contains("Needs:"), "{refused}");
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Active);
    }

    #[test]
    fn a_new_block_does_not_inherit_the_last_ones_wait() {
        let (store, task) = active_task("SOA Phase E");
        store
            .transition_task_with_note(
                task,
                TaskState::Blocked,
                "Waiting on: operator\nNeeds: approve M2",
            )
            .unwrap();
        store.transition_task(task, TaskState::Active).unwrap();
        let again = store
            .transition_task_with_note(task, TaskState::Blocked, "Something else entirely.")
            .unwrap();
        assert_eq!(again.wait, None);
        assert!(store.operator_waits().unwrap().is_empty());
    }

    /// Assigning Active work used to skip the one-Active rule that starting
    /// it obeys, so a worker could end up holding two (ADR 0119 Decision 5).
    #[test]
    fn active_work_cannot_be_assigned_to_a_worker_already_active_elsewhere() {
        let (store, held) = active_task("Already active here");
        let busy = store.get_task(held).unwrap().assigned_worker_id.unwrap();
        let other = store
            .create_worker("Fern", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        store
            .bind_worker_session(other.id, WorkerSessionId::new())
            .unwrap();
        let moving = store.create_task("Active elsewhere", "/workspace").unwrap();
        store.transition_task(moving.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(moving.id, other.id).unwrap();
        store.transition_task(moving.id, TaskState::Active).unwrap();

        let refused = store.assign_task_to_worker(moving.id, busy);

        assert!(
            matches!(
                refused,
                Err(TaskStoreError::WorkerAlreadyHasActiveTask { .. })
            ),
            "{refused:?}"
        );
        assert_eq!(
            store.get_task(moving.id).unwrap().assigned_worker_id,
            Some(other.id)
        );
    }

    /// Decision 7: the open board is typed from what its record can support.
    #[test]
    fn the_migration_types_the_blocks_already_on_the_board() {
        let (store, dated) = active_task("Verify the renewals");
        store.transition_task(dated, TaskState::Blocked).unwrap();
        let held_until = now(&store) + 10 * 86_400;
        let prose = store.create_task("Untyped", "/workspace").unwrap();
        store.transition_task(prose.id, TaskState::Ready).unwrap();
        store.transition_task(prose.id, TaskState::Blocked).unwrap();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "UPDATE tasks SET blocked_until = ?2 WHERE id = ?1",
                    params![dated.to_string(), held_until],
                )
                .unwrap();
            connection.execute("DELETE FROM task_waits", []).unwrap();
        }

        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_task_waits(&transaction).unwrap();
        transaction.commit().unwrap();
        drop(connection);

        let wait = store
            .get_task(dated)
            .unwrap()
            .wait
            .expect("the dated hold is typed");
        assert_eq!(wait.kind, TaskWaitKind::Date);
        assert_eq!(wait.resume_at, Some(held_until));
        assert_eq!(
            store.get_task(prose.id).unwrap().wait,
            None,
            "prose stays untyped"
        );
    }

    /// The tasks already parked on Queen by an earlier release: each open
    /// block's current wait becomes hers with its words unchanged, and it is in
    /// her queue. An outside party keeps its wait, and a block the task has
    /// since left stays as history (ADR 0166).
    #[test]
    fn the_migration_brings_the_blocks_parked_on_queen_into_her_queue() {
        let recheck =
            |store: &TaskStore| swarm_domain::format_rfc3339_seconds(now(store) + 2 * 86_400);
        let block_as_older_releases_did = |store: &TaskStore, task: TaskId, party: &str| {
            let note = format!(
                "Draft PR #12 is ready.\nWaiting on: Spectrum\nNeeds: the operator release, then her GO on the exact head\nRecheck: {}",
                recheck(store)
            );
            store
                .transition_task_with_note(task, TaskState::Blocked, &note)
                .unwrap();
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE task_waits SET party = ?2 WHERE task_id = ?1",
                    params![task.to_string(), party],
                )
                .unwrap();
        };
        let (store, parked) = active_task("Payments B1");
        block_as_older_releases_did(&store, parked, " queen ");
        assert_eq!(
            store.get_task(parked).unwrap().next_move_owner,
            NextMoveOwner::Blocked,
            "what an earlier release left it: nobody's"
        );
        let vendor = store.create_task("Spectrum outage", "/workspace").unwrap();
        store.transition_task(vendor.id, TaskState::Ready).unwrap();
        let worker = store.get_task(parked).unwrap().assigned_worker_id.unwrap();
        store.assign_task_to_worker(vendor.id, worker).unwrap();
        store.transition_task(vendor.id, TaskState::Active).unwrap();
        store
            .transition_task_with_note(
                vendor.id,
                TaskState::Blocked,
                &format!(
                    "Waiting on: Spectrum\nNeeds: X4 back to Available\nRecheck: {}",
                    recheck(&store)
                ),
            )
            .unwrap();
        let note_before = store.get_task(parked).unwrap().blocked_note;

        let mut connection = store.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_queen_waits(&transaction, crate::QUEEN_WAITS_SCHEMA_MARKER).unwrap();
        transaction.commit().unwrap();
        drop(connection);

        let moved = store.get_task(parked).unwrap();
        let wait = moved.wait.clone().expect("the wait is kept");
        assert_eq!(wait.kind.as_str(), "queen");
        assert_eq!(
            wait.needs,
            "the operator release, then her GO on the exact head"
        );
        assert_eq!((wait.party, wait.resume_at), (None, None));
        assert_eq!(moved.next_move_owner, NextMoveOwner::Queen);
        assert_eq!(moved.blocked_note, note_before, "its words are unchanged");
        let said: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT note FROM task_activity WHERE task_id = ?1 AND kind = 'noted'
                 ORDER BY sequence DESC LIMIT 1",
                [parked.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(said.contains("in her queue"), "{said}");
        let outside = store.get_task(vendor.id).unwrap().wait.unwrap();
        assert_eq!(outside.kind, TaskWaitKind::Outside);
        assert_eq!(outside.party.as_deref(), Some("Spectrum"));
        assert!(
            store
                .resume_cleared_waits(i64::MAX / 2)
                .unwrap()
                .resumed
                .iter()
                .all(|resumed| *resumed != parked),
            "it no longer comes back at its old Recheck"
        );
    }
}
