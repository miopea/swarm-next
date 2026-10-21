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

/// The activity entry that put task `t` into its current state.
macro_rules! wait_block_entry {
    () => {
        "(SELECT max(entry.sequence) FROM task_activity entry
          WHERE entry.task_id = t.id AND entry.kind = 'state_changed')"
    };
}
pub(crate) use wait_block_entry;

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
                OR (w.kind = 'operator_action' AND w.done_at IS NOT NULL)
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
    let decision: Option<String> = transaction
        .query_row(
            "SELECT d.title FROM task_decision_membership m
             JOIN decision_requests d ON d.id = m.decision_id
             WHERE m.task_id = ?1 AND d.state = 'pending'
             ORDER BY d.created_at LIMIT 1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(decision.map(|title| {
        (
            TaskWaitKind::Decision,
            bounded(&format!("The operator answers \"{title}\".")),
            None,
            None,
        )
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
    ///
    /// Runs in the coordination pass: every 30 seconds, and straight after a
    /// decision is answered or Done is pressed.
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
                     OR (w.kind = 'operator_action' AND w.done_at IS NOT NULL)
                     OR (w.kind IN ('outside','date') AND w.resume_at <= ?1))
                 ORDER BY t.position, t.id LIMIT ?2",
                gates = wait_gates_clear!(),
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
}
