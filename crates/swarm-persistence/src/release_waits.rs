//! Merged work that waits for its release (ADR 0180).
//!
//! A task in Awaiting Release names no person: its next move belongs to "the
//! release", an event. So nothing told anyone when merged work had waited too
//! long, and the work queued behind it waited too.
//!
//! Three parts, kept apart on purpose:
//!
//! - **What git says.** The API reads each stay's merge from its workspace's
//!   own refs and records it here. Persistence never runs git.
//! - **When it is too long.** The rules live in `swarm_domain::release_waits`.
//! - **What it raises.** One attention row per workspace for Queen, on the
//!   task that has waited longest, and one reminder from Swarm to whoever
//!   releases it, once per stay. The operator is never told by this: Queen
//!   decides that (ADR 0061, ADR 0082).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::str::FromStr;

use rusqlite::params;
use swarm_domain::{
    ReleaseWaitTrigger, TaskId, UNMEASURED_TAIL_SECONDS, WorkerId, WorkerSessionId,
    release_wait_duration, release_wait_slow_tail_seconds, release_wait_started_at,
    release_wait_trigger,
};
use uuid::Uuid;

use crate::messages::MessageEnd;
use crate::{TaskStore, TaskStoreError};

/// How far back measured waits set a repository's slow tail, in seconds.
pub const RELEASE_WAIT_WINDOW_SECONDS: i64 = 30 * 24 * 60 * 60;

/// The most stays still in Awaiting Release read from git in one sweep.
const MAX_LIVE_READS: i64 = 64;

/// The most ended stays read in one sweep to fill the measured history.
///
/// A Hive taking this release has about 600 ended stays in its window and
/// none recorded. At 32 a sweep and a sweep every five minutes, the history
/// fills in under two hours, and no sweep runs more than a few hundred git
/// commands.
const MAX_BACKFILL_READS: i64 = 32;

/// The most recorded commits read for one stay.
const MAX_COMMITS_PER_READ: i64 = 64;

/// The most tasks one row names as waiting on the release, or merged beside it.
const MAX_NAMED_TASKS: usize = 5;

/// Every stay in Awaiting Release: the activity row that parked it, and when
/// it ended, which is the next state change and NULL while it lasts.
const RELEASE_STAYS: &str = "(SELECT entry.task_id AS task_id, entry.sequence AS parked_sequence,
              entry.occurred_at AS parked_at,
              (SELECT exit.occurred_at FROM task_activity exit
               WHERE exit.task_id = entry.task_id AND exit.sequence > entry.sequence
                 AND exit.kind = 'state_changed'
               ORDER BY exit.sequence LIMIT 1) AS left_at
       FROM task_activity entry
       WHERE entry.kind = 'state_changed' AND entry.to_state = 'awaiting_release')";

/// A stay to read from git: its task, when it was parked and left, where its
/// work is, and the commits it recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseWaitRead {
    pub task_id: TaskId,
    pub parked_sequence: i64,
    pub parked_at: i64,
    pub left_at: Option<i64>,
    pub workspace: String,
    pub commits: Vec<String>,
}

/// What a workspace's own refs said about a stay's commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReleaseWaitMerge {
    /// Every recorded commit is on the default branch. The time is the latest
    /// of their merges, and the commit is the one that brought that one in.
    Merged {
        merged_at: i64,
        merge_commit: String,
    },
    /// At least one recorded commit is not on the default branch.
    NotMerged,
}

/// Merged work that has waited long enough to name, before its row is raised.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergedWorkWaitingCandidate {
    pub task_id: TaskId,
    pub task_title: String,
    pub parked_sequence: i64,
    pub task_revision: i64,
    pub workspace: String,
    /// Who releases it: the worker Queen named, or else the assignee.
    pub releaser: WorkerId,
    pub releaser_name: String,
    pub releaser_named: bool,
    /// The releaser's open session, or their latest one.
    pub session_id: WorkerSessionId,
    pub merge_commit: String,
    pub waited_seconds: i64,
    pub trigger: ReleaseWaitTrigger,
    /// The slow-tail threshold, and whether it was measured in this repository.
    pub slow_tail_seconds: i64,
    pub slow_tail_measured: bool,
    /// The other merged tasks in this workspace that also wait.
    pub also_waiting: Vec<TaskId>,
    pub reminded: bool,
    pub card: ReleaseWaitCard,
}

/// Whether a pending card is linked to the task, and whose it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseWaitCard {
    None,
    Queen,
    Operator,
}

/// What raising one candidate did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MergedWorkWaitingRaise {
    /// A new row reached Queen.
    pub raised: bool,
    /// Swarm's reminder was written to the releaser.
    pub reminded: bool,
    /// The reminder was due and could not be written: the message queue was
    /// full. It is tried again on the next pass.
    pub reminder_deferred: bool,
}

/// Adds Swarm as a message sender, the merged-work attention kind, the record
/// of each stay's merge, and who releases each task (schema 232).
///
/// Two tables are rebuilt, because `SQLite` cannot widen a CHECK in place.
/// `coordinator_actions` has no table holding a key into it.
/// `task_messages` has two, `task_message_deliveries` and
/// `task_returned_reviews`, and `legacy_alter_table` keeps their keys naming
/// `task_messages`, so they point at the new table when it takes the name.
/// Foreign keys are off for the migration and checked after it commits.
pub(crate) fn migrate_release_waits(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    let has =
        |sql: &str| -> rusqlite::Result<bool> { transaction.query_row(sql, [], |row| row.get(0)) };
    let foreign_tables = has("SELECT COUNT(*) = 2 FROM sqlite_master
         WHERE type = 'table' AND name IN ('tasks', 'worker_profiles')")?;
    // A narrow historical fixture holds only the tables its own step tests.
    // It cannot hold coordinator rows or messages, so it only advances.
    if !foreign_tables {
        return transaction.pragma_update(None, "user_version", version);
    }
    widen_coordinator_actions(transaction, &has)?;
    widen_task_messages(transaction, &has)?;
    create_release_wait_tables(transaction)?;
    transaction.pragma_update(None, "user_version", version)
}

/// Adds the merged-work attention kind to `coordinator_actions`, once.
fn widen_coordinator_actions(
    transaction: &rusqlite::Transaction<'_>,
    has: &dyn Fn(&str) -> rusqlite::Result<bool>,
) -> rusqlite::Result<()> {
    let actions_widened = has("SELECT NOT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'coordinator_actions')
         OR EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table'
                   AND name = 'coordinator_actions' AND sql LIKE '%merged_work_waiting_attention%')")?;
    if !actions_widened {
        transaction.execute_batch(
            "PRAGMA legacy_alter_table = ON;
             ALTER TABLE coordinator_actions RENAME TO coordinator_actions_v231;
             CREATE TABLE coordinator_actions (
                 id TEXT PRIMARY KEY,
                 idempotency_key TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention','blocked_work_unattended_attention','evidenced_work_not_closed_attention','worker_cannot_start_attention','merged_work_waiting_attention')),
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
                 session_id TEXT,
                 evidence_revision INTEGER,
                 observed_age_seconds INTEGER,
                 state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                 reason TEXT NOT NULL,
                 attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                 attempted_at INTEGER,
                 finished_at INTEGER,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 -- ⚠️ ONLY the worker-scoped kind may omit a task. Without this a
                 -- task-scoped attention could silently lose its task and read as a
                 -- worker-level condition, which is the opposite of the fix.
                 CHECK (task_id IS NOT NULL OR kind = 'worker_cannot_start_attention')
             );
             INSERT INTO coordinator_actions (
                 id, idempotency_key, kind, worker_id, task_id, session_id,
                 evidence_revision, observed_age_seconds, state, reason,
                 attempts, attempted_at, finished_at, created_at, updated_at
             ) SELECT id, idempotency_key, kind, worker_id, task_id, session_id,
                      evidence_revision, observed_age_seconds, state, reason,
                      attempts, attempted_at, finished_at, created_at, updated_at
               FROM coordinator_actions_v231;
             DROP TABLE coordinator_actions_v231;
             CREATE INDEX coordinator_actions_queue
                 ON coordinator_actions(state, created_at, id);
             CREATE INDEX coordinator_actions_by_task
                 ON coordinator_actions(task_id, kind);
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    Ok(())
}

/// Adds Swarm as a sender to `task_messages`, once, writing only to a worker.
fn widen_task_messages(
    transaction: &rusqlite::Transaction<'_>,
    has: &dyn Fn(&str) -> rusqlite::Result<bool>,
) -> rusqlite::Result<()> {
    let messages_widened = has("SELECT NOT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'task_messages')
         OR EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table'
                   AND name = 'task_messages' AND sql LIKE '%''swarm''%')")?;
    if !messages_widened {
        transaction.execute_batch(
            "PRAGMA legacy_alter_table = ON;
             ALTER TABLE task_messages RENAME TO task_messages_v231;
             CREATE TABLE task_messages (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 sender TEXT NOT NULL CHECK (sender IN ('queen','worker','operator','swarm')),
                 recipient TEXT NOT NULL CHECK (recipient IN ('queen','worker')),
                 sender_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
                 recipient_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
                 body TEXT NOT NULL,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 delivered_at INTEGER,
                 delivered_session_id TEXT,
                 CHECK (NOT (sender = 'worker' AND recipient = 'worker')),
                 -- Swarm's reminder goes to a worker and to nobody else.
                 CHECK (sender <> 'swarm' OR recipient = 'worker')
             );
             INSERT INTO task_messages (
                 id, task_id, sender, recipient, sender_worker_id, recipient_worker_id,
                 body, created_at, delivered_at, delivered_session_id
             ) SELECT id, task_id, sender, recipient, sender_worker_id, recipient_worker_id,
                      body, created_at, delivered_at, delivered_session_id
               FROM task_messages_v231;
             DROP TABLE task_messages_v231;
             CREATE INDEX task_messages_by_task
                 ON task_messages(task_id, created_at);
             CREATE INDEX task_messages_undelivered
                 ON task_messages(recipient_worker_id) WHERE delivered_at IS NULL;
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    Ok(())
}

/// The record of each stay's merge, and who releases each parked task.
fn create_release_wait_tables(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "-- One row per stay in Awaiting Release whose merge git was asked about.
         -- A stay is named by the activity row that parked it, so a task parked
         -- twice has two rows and the second does not inherit the first's
         -- reminder.
         CREATE TABLE IF NOT EXISTS release_waits (
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             parked_sequence INTEGER NOT NULL,
             parked_at INTEGER NOT NULL,
             workspace TEXT NOT NULL,
             merged_at INTEGER,
             merge_commit TEXT,
             left_at INTEGER,
             read_at INTEGER NOT NULL,
             reminded_at INTEGER,
             PRIMARY KEY (task_id, parked_sequence),
             CHECK ((merged_at IS NULL) = (merge_commit IS NULL)),
             CHECK (merge_commit IS NULL OR length(merge_commit) BETWEEN 7 AND 64)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS release_waits_measured
             ON release_waits(workspace, left_at) WHERE merged_at IS NOT NULL;
         -- Who releases a task in Awaiting Release, when Queen named someone
         -- other than its assignee. Cleared whenever the task enters or leaves
         -- that state, so a name never outlives the stay it was given for.
         CREATE TABLE IF NOT EXISTS task_releasers (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             named_at INTEGER NOT NULL
         ) WITHOUT ROWID;",
    )
}

/// Names the worker who releases a task as Queen parks it, inside the
/// transition that parks it, or clears the name when it enters or leaves
/// Awaiting Release without one.
pub(crate) fn name_releaser(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    releaser: Option<WorkerId>,
    now: i64,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "DELETE FROM task_releasers WHERE task_id = ?1",
        [task_id.to_string()],
    )?;
    let Some(worker_id) = releaser else {
        return Ok(());
    };
    let eligible: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_profiles
         WHERE id = ?1 AND archived_at IS NULL AND role <> 'queen')",
        [worker_id.to_string()],
        |row| row.get(0),
    )?;
    if !eligible {
        return Err(TaskStoreError::ReleaserNotAWorker);
    }
    transaction.execute(
        "INSERT INTO task_releasers(task_id, worker_id, named_at) VALUES (?1, ?2, ?3)",
        params![task_id.to_string(), worker_id.to_string(), now],
    )?;
    Ok(())
}

/// Whether this worker is named to release this task, which is in Awaiting
/// Release now.
pub(crate) fn is_named_releaser(
    connection: &rusqlite::Connection,
    task_id: TaskId,
    worker_id: WorkerId,
) -> Result<bool, TaskStoreError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_releasers releaser
         JOIN tasks task ON task.id = releaser.task_id
         WHERE releaser.task_id = ?1 AND releaser.worker_id = ?2
           AND task.state = 'awaiting_release' AND task.removed_at IS NULL)",
        params![task_id.to_string(), worker_id.to_string()],
        |row| row.get(0),
    )?)
}

impl TaskStore {
    /// The stays the next sweep should read from git: every stay still in
    /// Awaiting Release, then a bounded batch of ended stays the measured
    /// history lacks.
    ///
    /// Only stays whose task recorded commits Swarm found. One with none has
    /// nothing git can place on a branch.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn release_wait_reads(&self, now: i64) -> Result<Vec<ReleaseWaitRead>, TaskStoreError> {
        let connection = self.connection()?;
        let live = format!(
            "SELECT stay.task_id, stay.parked_sequence, stay.parked_at, stay.left_at, task.workspace
             FROM {RELEASE_STAYS} stay
             JOIN tasks task ON task.id = stay.task_id AND task.removed_at IS NULL
             WHERE stay.left_at IS NULL AND task.state = 'awaiting_release'
               AND EXISTS(SELECT 1 FROM task_commits c
                          WHERE c.task_id = stay.task_id AND c.verdict = 'present')
             ORDER BY stay.parked_sequence LIMIT ?1"
        );
        // An ended stay is read again when its last read came before it ended:
        // its merge may have landed between the last live read and the release.
        let ended = format!(
            "SELECT stay.task_id, stay.parked_sequence, stay.parked_at, stay.left_at, task.workspace
             FROM {RELEASE_STAYS} stay
             JOIN tasks task ON task.id = stay.task_id AND task.removed_at IS NULL
             LEFT JOIN release_waits wait
               ON wait.task_id = stay.task_id AND wait.parked_sequence = stay.parked_sequence
             WHERE stay.left_at IS NOT NULL AND stay.left_at >= ?2 - {RELEASE_WAIT_WINDOW_SECONDS}
               AND (wait.task_id IS NULL OR wait.read_at < stay.left_at)
               AND EXISTS(SELECT 1 FROM task_commits c
                          WHERE c.task_id = stay.task_id AND c.verdict = 'present')
             ORDER BY stay.left_at DESC, stay.parked_sequence LIMIT ?1"
        );
        let stay = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, String>(4)?,
            ))
        };
        let mut stays = connection
            .prepare(&live)?
            .query_map([MAX_LIVE_READS], stay)?
            .collect::<Result<Vec<_>, _>>()?;
        stays.extend(
            connection
                .prepare(&ended)?
                .query_map([MAX_BACKFILL_READS, now], stay)?
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut reads = Vec::with_capacity(stays.len());
        for (task_id, parked_sequence, parked_at, left_at, workspace) in stays {
            let task_id = TaskId::from_str(&task_id).map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid task id in a release stay".into())
            })?;
            let mut commits = connection.prepare(
                "SELECT sha FROM task_commits WHERE task_id = ?1 AND verdict = 'present'
                 ORDER BY recorded_at, sha LIMIT ?2",
            )?;
            let commits = commits
                .query_map(params![task_id.to_string(), MAX_COMMITS_PER_READ], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            reads.push(ReleaseWaitRead {
                task_id,
                parked_sequence,
                parked_at,
                left_at,
                workspace,
                commits,
            });
        }
        Ok(reads)
    }

    /// Records what git said about each stay, in one transaction.
    ///
    /// The end of each stay is read from the activity trail here, not taken
    /// from the read: a task released between the read and this write would
    /// otherwise be recorded as still waiting and never measured. Stays that
    /// ended before the measured window are dropped, which is what bounds this
    /// table.
    ///
    /// # Errors
    /// Returns a persistence error. Nothing is written when one fails.
    pub fn record_release_waits(
        &self,
        observed: &[(ReleaseWaitRead, ReleaseWaitMerge)],
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let stay_end = "(SELECT exit.occurred_at FROM task_activity exit
              WHERE exit.task_id = release_waits.task_id
                AND exit.sequence > release_waits.parked_sequence
                AND exit.kind = 'state_changed'
              ORDER BY exit.sequence LIMIT 1)";
        for (read, merge) in observed {
            let (merged_at, merge_commit) = match merge {
                ReleaseWaitMerge::Merged {
                    merged_at,
                    merge_commit,
                } => (Some(*merged_at), Some(merge_commit.as_str())),
                ReleaseWaitMerge::NotMerged => (None, None),
            };
            transaction.execute(
                "INSERT INTO release_waits
                     (task_id, parked_sequence, parked_at, workspace, merged_at, merge_commit,
                      read_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (task_id, parked_sequence) DO UPDATE SET
                     workspace = excluded.workspace, merged_at = excluded.merged_at,
                     merge_commit = excluded.merge_commit, read_at = excluded.read_at",
                params![
                    read.task_id.to_string(),
                    read.parked_sequence,
                    read.parked_at,
                    read.workspace,
                    merged_at,
                    merge_commit,
                    now,
                ],
            )?;
        }
        transaction.execute(
            &format!("UPDATE release_waits SET left_at = {stay_end} WHERE left_at IS NULL"),
            [],
        )?;
        transaction.execute(
            "DELETE FROM release_waits WHERE left_at < ?1",
            [now - RELEASE_WAIT_WINDOW_SECONDS],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Merged work that has waited long enough to name: at most one per
    /// workspace, the one that has waited longest.
    ///
    /// A workspace whose row Queen already holds is skipped, unless that row's
    /// reminder is still owed, so a full message queue delays the reminder
    /// rather than losing it.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn merged_work_waiting_candidates(
        &self,
        now: i64,
    ) -> Result<Vec<MergedWorkWaitingCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        let tails = slow_tails(&connection, now)?;
        let rows = waiting_rows(&connection)?;
        let mut by_workspace: HashMap<String, Vec<(WaitingRow, ReleaseWaitTrigger, i64)>> =
            HashMap::new();
        for row in rows {
            let waited = now - release_wait_started_at(row.merged_at, row.parked_at);
            let (tail, _) = tails
                .get(&row.workspace)
                .copied()
                .unwrap_or((UNMEASURED_TAIL_SECONDS, false));
            if let Some(trigger) = release_wait_trigger(waited, row.others_waiting, tail) {
                by_workspace
                    .entry(row.workspace.clone())
                    .or_default()
                    .push((row, trigger, waited));
            }
        }
        let mut candidates = Vec::new();
        for (workspace, mut waiting) in by_workspace {
            // Longest first; the earlier park breaks a tie.
            waiting.sort_by(|a, b| {
                b.2.cmp(&a.2)
                    .then(a.0.parked_sequence.cmp(&b.0.parked_sequence))
            });
            let also_waiting = waiting
                .iter()
                .skip(1)
                .filter_map(|(row, _, _)| TaskId::from_str(&row.task_id).ok())
                .collect::<Vec<_>>();
            let Some((row, trigger, waited)) = waiting.into_iter().next() else {
                continue;
            };
            // One row per workspace. The task that holds it is raised again
            // only while its reminder is still owed.
            if let Some(holder) = &row.workspace_held_by
                && (holder != &row.task_id || row.reminded)
            {
                continue;
            }
            let Some(session) = row.session_id.as_deref() else {
                // A releaser who has never had a session cannot be reached.
                // Rare enough that the row would only report it; skipping
                // leaves Queen's ordinary view of the task to show it.
                continue;
            };
            let invalid = |what: &str| {
                TaskStoreError::IntegrityFailure(format!("invalid {what} in a release wait"))
            };
            let (slow_tail_seconds, slow_tail_measured) = tails
                .get(&workspace)
                .copied()
                .unwrap_or((UNMEASURED_TAIL_SECONDS, false));
            candidates.push(MergedWorkWaitingCandidate {
                task_id: TaskId::from_str(&row.task_id).map_err(|_| invalid("task id"))?,
                task_title: row.task_title,
                parked_sequence: row.parked_sequence,
                task_revision: row.task_revision,
                workspace,
                releaser: WorkerId::from_str(&row.releaser).map_err(|_| invalid("worker id"))?,
                releaser_name: row.releaser_name,
                releaser_named: row.releaser_named,
                session_id: WorkerSessionId::from_str(session)
                    .map_err(|_| invalid("session id"))?,
                merge_commit: row.merge_commit,
                waited_seconds: waited,
                trigger,
                slow_tail_seconds,
                slow_tail_measured,
                also_waiting,
                reminded: row.reminded,
                card: match row.card.as_str() {
                    "queen" => ReleaseWaitCard::Queen,
                    "operator" => ReleaseWaitCard::Operator,
                    _ => ReleaseWaitCard::None,
                },
            });
        }
        candidates.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        Ok(candidates)
    }

    /// Raises one candidate's row for Queen and, once per stay, Swarm's
    /// reminder to whoever releases it, in one transaction that first checks
    /// the stay is still the one the candidate read.
    ///
    /// # Errors
    /// Returns a persistence error. A full message queue is not one: the row
    /// is still raised and the reminder waits for the next pass.
    pub fn record_merged_work_waiting_attention(
        &self,
        candidate: &MergedWorkWaitingCandidate,
        now: i64,
    ) -> Result<MergedWorkWaitingRaise, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let still_waiting: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks task
             JOIN release_waits wait ON wait.task_id = task.id
             WHERE task.id = ?1 AND task.state = 'awaiting_release' AND task.removed_at IS NULL
               AND task.updated_at = ?2 AND wait.parked_sequence = ?3
               AND wait.left_at IS NULL AND wait.merge_commit = ?4)",
            params![
                candidate.task_id.to_string(),
                candidate.task_revision,
                candidate.parked_sequence,
                candidate.merge_commit,
            ],
            |row| row.get(0),
        )?;
        if !still_waiting {
            transaction.commit()?;
            return Ok(MergedWorkWaitingRaise::default());
        }
        let mut outcome = MergedWorkWaitingRaise::default();
        if !candidate.reminded {
            let reminder = reminder_text(candidate);
            match TaskStore::insert_task_message(
                &transaction,
                candidate.task_id,
                MessageEnd::swarm(),
                MessageEnd::worker(candidate.releaser),
                &reminder,
                now,
            ) {
                Ok(_) => {
                    transaction.execute(
                        "UPDATE release_waits SET reminded_at = ?3
                         WHERE task_id = ?1 AND parked_sequence = ?2",
                        params![
                            candidate.task_id.to_string(),
                            candidate.parked_sequence,
                            now
                        ],
                    )?;
                    outcome.reminded = true;
                }
                Err(TaskStoreError::TaskMessageQueueFull) => outcome.reminder_deferred = true,
                Err(error) => return Err(error),
            }
        }
        let waiting_on_it = tasks_waiting_on(&transaction, candidate.task_id)?;
        let reason = attention_reason(candidate, &waiting_on_it, outcome);
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO coordinator_actions
                 (id, idempotency_key, kind, worker_id, task_id, session_id,
                  evidence_revision, observed_age_seconds, state, reason,
                  finished_at, updated_at)
             VALUES (?1, ?2, 'merged_work_waiting_attention', ?3, ?4, ?5, ?6, ?7,
                     'completed', ?8, ?9, ?9)",
            params![
                Uuid::now_v7().to_string(),
                format!(
                    "merged-work-waiting:{}:{}:{}",
                    candidate.task_id, candidate.parked_sequence, candidate.task_revision
                ),
                candidate.releaser.to_string(),
                candidate.task_id.to_string(),
                candidate.session_id.to_string(),
                candidate.task_revision,
                candidate.waited_seconds,
                reason,
                now,
            ],
        )?;
        outcome.raised = changed == 1;
        transaction.commit()?;
        Ok(outcome)
    }
}

struct WaitingRow {
    task_id: String,
    task_title: String,
    parked_sequence: i64,
    task_revision: i64,
    workspace: String,
    releaser: String,
    releaser_name: String,
    releaser_named: bool,
    session_id: Option<String>,
    merge_commit: String,
    merged_at: i64,
    parked_at: i64,
    others_waiting: bool,
    reminded: bool,
    workspace_held_by: Option<String>,
    card: String,
}

/// Each workspace's slow-tail threshold and whether it was measured there.
fn slow_tails(
    connection: &rusqlite::Connection,
    now: i64,
) -> Result<HashMap<String, (i64, bool)>, TaskStoreError> {
    // A stay that ended before its merge did not wait on a release.
    let mut statement = connection.prepare(
        "SELECT workspace, left_at - MAX(merged_at, parked_at) FROM release_waits
         WHERE merged_at IS NOT NULL AND left_at IS NOT NULL AND merged_at <= left_at
           AND left_at >= ?1",
    )?;
    let mut waits: HashMap<String, Vec<i64>> = HashMap::new();
    for row in statement.query_map([now - RELEASE_WAIT_WINDOW_SECONDS], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (workspace, wait) = row?;
        waits.entry(workspace).or_default().push(wait);
    }
    Ok(waits
        .into_iter()
        .map(|(workspace, waits)| {
            let measured = waits.len() >= swarm_domain::MIN_MEASURED_WAITS;
            (
                workspace,
                (release_wait_slow_tail_seconds(&waits), measured),
            )
        })
        .collect())
}

/// The unfinished tasks recorded as waiting on this one, by id and title.
fn tasks_waiting_on(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<Vec<(String, String)>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT waiting.id, waiting.title FROM task_prerequisites link
         JOIN tasks waiting ON waiting.id = link.task_id
         WHERE link.prerequisite_id = ?1 AND waiting.removed_at IS NULL
           AND waiting.state NOT IN ('completed', 'abandoned')
         ORDER BY waiting.id LIMIT ?2",
    )?;
    let rows = statement
        .query_map(
            params![
                task_id.to_string(),
                i64::try_from(MAX_NAMED_TASKS + 1).unwrap_or(i64::MAX)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

/// Every live stay whose work has merged, with what deciding on it needs.
fn waiting_rows(connection: &rusqlite::Connection) -> rusqlite::Result<Vec<WaitingRow>> {
    let mut statement = connection.prepare(concat!(
        "SELECT task.id, task.title, wait.parked_sequence, task.updated_at, wait.workspace,
                releaser.id, releaser.name, named_worker.id IS NOT NULL,
                (SELECT session.session_id FROM worker_sessions session
                 WHERE session.worker_id = releaser.id
                 ORDER BY session.ended_at IS NULL DESC, session.started_at DESC
                 LIMIT 1),
                wait.merge_commit, wait.merged_at, wait.parked_at,
                EXISTS(SELECT 1 FROM task_prerequisites link
                       JOIN tasks waiting ON waiting.id = link.task_id
                       WHERE link.prerequisite_id = task.id AND waiting.removed_at IS NULL
                         AND waiting.state NOT IN ('completed', 'abandoned')),
                wait.reminded_at IS NOT NULL,
                (SELECT held.id FROM coordinator_actions action
                 JOIN tasks held ON held.id = action.task_id
                 JOIN release_waits held_wait ON held_wait.task_id = held.id
                 WHERE action.kind = 'merged_work_waiting_attention'
                   AND held.state = 'awaiting_release' AND held.removed_at IS NULL
                   AND action.evidence_revision = held.updated_at
                   AND held_wait.workspace = wait.workspace
                   AND held_wait.left_at IS NULL
                 ORDER BY action.id LIMIT 1),
                CASE
                    WHEN EXISTS(SELECT 1 FROM decision_requests d
                         WHERE d.id IN (SELECT decision_id FROM task_decision_membership
                                        WHERE task_id = task.id)
                           AND d.state = 'pending' AND ",
        crate::queen_grants::queen_card!("d"),
        ") THEN 'queen'
                    WHEN EXISTS(SELECT 1 FROM decision_requests d
                         WHERE d.id IN (SELECT decision_id FROM task_decision_membership
                                        WHERE task_id = task.id)
                           AND d.state = 'pending') THEN 'operator'
                    ELSE 'none' END
         FROM release_waits wait
         JOIN tasks task ON task.id = wait.task_id
         LEFT JOIN task_releasers named ON named.task_id = task.id
         LEFT JOIN worker_profiles named_worker
           ON named_worker.id = named.worker_id AND named_worker.archived_at IS NULL
         JOIN worker_profiles releaser
           ON releaser.id = COALESCE(named_worker.id, task.assigned_worker_id)
         WHERE wait.left_at IS NULL AND wait.merged_at IS NOT NULL
           AND task.state = 'awaiting_release' AND task.removed_at IS NULL
           AND wait.parked_sequence = (
               SELECT MAX(latest.sequence) FROM task_activity latest
               WHERE latest.task_id = task.id AND latest.kind = 'state_changed')
         ORDER BY wait.workspace, task.id"
    ))?;
    statement
        .query_map([], |row| {
            Ok(WaitingRow {
                task_id: row.get(0)?,
                task_title: row.get(1)?,
                parked_sequence: row.get(2)?,
                task_revision: row.get(3)?,
                workspace: row.get(4)?,
                releaser: row.get(5)?,
                releaser_name: row.get(6)?,
                releaser_named: row.get(7)?,
                session_id: row.get(8)?,
                merge_commit: row.get(9)?,
                merged_at: row.get(10)?,
                parked_at: row.get(11)?,
                others_waiting: row.get(12)?,
                reminded: row.get(13)?,
                workspace_held_by: row.get(14)?,
                card: row.get(15)?,
            })
        })?
        .collect()
}

/// What the row tells Queen. Everything in it is a stored fact; it says
/// nothing about CI on the merge or her go, because neither is stored.
fn attention_reason(
    candidate: &MergedWorkWaitingCandidate,
    waiting_on_it: &[(String, String)],
    outcome: MergedWorkWaitingRaise,
) -> String {
    let mut reason = format!(
        "Merged work has waited {} for its release: commit {} is on the default branch and the task is still in Awaiting release. ",
        release_wait_duration(candidate.waited_seconds),
        short(&candidate.merge_commit),
    );
    match candidate.trigger {
        ReleaseWaitTrigger::HoldingOthers => reason.push_str(
            "Raised because unfinished work waits on it and 30 minutes have passed since the merge. ",
        ),
        ReleaseWaitTrigger::SlowTail if candidate.slow_tail_measured => {
            let _ = write!(
                reason,
                "Raised because it has waited longer than 90% of this repository's release waits in the last 30 days ({}). ",
                release_wait_duration(candidate.slow_tail_seconds)
            );
        }
        ReleaseWaitTrigger::SlowTail => reason.push_str(
            "Raised because it has waited more than a day; this repository has too few measured releases to set its own threshold. ",
        ),
    }
    let _ = write!(
        reason,
        "Releases it: {} ({}). ",
        candidate.releaser_name,
        if candidate.releaser_named {
            "you named them"
        } else {
            "its assignee; you named nobody else"
        }
    );
    reason.push_str(if outcome.reminded {
        "Swarm reminded them once, as Swarm, not as you; the reminder authorises nothing. "
    } else if outcome.reminder_deferred {
        "Swarm's reminder to them is waiting: the message queue is full. "
    } else {
        "Swarm already reminded them during this stay. "
    });
    if !waiting_on_it.is_empty() {
        let named = waiting_on_it
            .iter()
            .take(MAX_NAMED_TASKS)
            .map(|(id, title)| format!("{id} \"{title}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let more = if waiting_on_it.len() > MAX_NAMED_TASKS {
            " and more"
        } else {
            ""
        };
        let _ = write!(reason, "Waiting on it: {named}{more}. ");
    }
    if !candidate.also_waiting.is_empty() {
        let named = candidate
            .also_waiting
            .iter()
            .take(MAX_NAMED_TASKS)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let more = if candidate.also_waiting.len() > MAX_NAMED_TASKS {
            " and more"
        } else {
            ""
        };
        let _ = write!(
            reason,
            "Also merged and waiting in this repository: {named}{more}. "
        );
    }
    reason.push_str(match candidate.card {
        ReleaseWaitCard::None => "No pending card is linked to it. ",
        ReleaseWaitCard::Queen => "A pending card linked to it is yours to grant. ",
        ReleaseWaitCard::Operator => "A pending card linked to it waits on the operator. ",
    });
    reason.push_str("Swarm has not told the operator; whether to is yours.");
    reason
}

/// Swarm's one reminder to whoever releases the task.
fn reminder_text(candidate: &MergedWorkWaitingCandidate) -> String {
    format!(
        "Swarm reminder, not an instruction. Task {} \"{}\" is merged (commit {}) and has waited {} for its release in Awaiting release. You are {}. This message authorises nothing. If Queen's go covers this release, release it the way its repository releases. If it does not, or something holds the release, tell Queen on this task with swarm_message_queen what it waits on. Swarm sends this once per stay.",
        candidate.task_id,
        candidate.task_title,
        short(&candidate.merge_commit),
        release_wait_duration(candidate.waited_seconds),
        if candidate.releaser_named {
            "the worker Queen named to release it"
        } else {
            "its assignee, and Queen named nobody else to release it"
        },
    )
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, ProviderKind, TaskActivityActor, TaskCommit,
        TaskState, WorkerRole,
    };

    use super::*;
    use crate::MessageParty;

    const WORKSPACE: &str = "/workspace/petal";
    /// A fixed clock, so a wait is exactly what the test says it is.
    const PARKED: i64 = 2_000_000_000;
    const MERGED: i64 = PARKED + 600;
    const MERGE_COMMIT: &str = "5c45d04c1a2b3c4d5e6f";

    struct Hive {
        store: TaskStore,
        worker: WorkerId,
        session: WorkerSessionId,
    }

    fn hive() -> Hive {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, WORKSPACE, false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        Hive {
            store,
            worker: worker.id,
            session,
        }
    }

    fn another_worker(hive: &Hive, name: &str, workspace: &str) -> (WorkerId, WorkerSessionId) {
        let worker = hive
            .store
            .create_worker(name, ProviderKind::ClaudeCode, workspace, false, 2)
            .unwrap();
        let session = WorkerSessionId::new();
        hive.store.bind_worker_session(worker.id, session).unwrap();
        (worker.id, session)
    }

    /// Reviewed work by the worker on `session`, whose workspace the task
    /// takes when it is assigned.
    fn reviewed_task_by(hive: &Hive, session: WorkerSessionId, title: &str) -> TaskId {
        let task = hive.store.create_task(title, WORKSPACE).unwrap();
        hive.store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        let task = hive.store.assign_task(task.id, session).unwrap();
        let workspace = task.workspace.as_str();
        hive.store
            .transition_task(task.id, TaskState::Active)
            .unwrap();
        hive.store
            .transition_task(task.id, TaskState::Review)
            .unwrap();
        hive.store
            .record_task_commits(
                task.id,
                workspace,
                CommitRepositoryState::Read,
                &[TaskCommit {
                    sha: format!("{:0>12}", title.len()),
                    verdict: CommitVerdict::Present,
                    subject: title.to_owned(),
                    changed_paths: vec!["src/lib.rs".to_owned()],
                    found_in: None,
                }],
                PARKED,
            )
            .unwrap();
        task.id
    }

    fn reviewed_task(hive: &Hive, title: &str) -> TaskId {
        reviewed_task_by(hive, hive.session, title)
    }

    /// Moves the most recent state change of a task to a fixed time.
    fn at(hive: &Hive, task: TaskId, time: i64) {
        hive.store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_activity SET occurred_at = ?2 WHERE sequence = (
                     SELECT MAX(sequence) FROM task_activity
                     WHERE task_id = ?1 AND kind = 'state_changed')",
                params![task.to_string(), time],
            )
            .unwrap();
    }

    fn park(hive: &Hive, title: &str, parked_at: i64) -> TaskId {
        park_by(hive, hive.session, title, parked_at)
    }

    fn park_by(hive: &Hive, session: WorkerSessionId, title: &str, parked_at: i64) -> TaskId {
        let task = reviewed_task_by(hive, session, title);
        hive.store
            .transition_task(task, TaskState::AwaitingRelease)
            .unwrap();
        at(hive, task, parked_at);
        task
    }

    fn record_merge(hive: &Hive, task: TaskId, merged_at: i64, now: i64) {
        let read = hive
            .store
            .release_wait_reads(now)
            .unwrap()
            .into_iter()
            .find(|read| read.task_id == task)
            .expect("the stay is read");
        hive.store
            .record_release_waits(
                &[(
                    read,
                    ReleaseWaitMerge::Merged {
                        merged_at,
                        merge_commit: MERGE_COMMIT.to_owned(),
                    },
                )],
                now,
            )
            .unwrap();
    }

    fn waits_on(hive: &Hive, waiting: TaskId, prerequisite: TaskId) {
        hive.store
            .add_task_prerequisite(
                waiting,
                prerequisite,
                "it builds on this",
                &TaskActivityActor::operator(),
                PARKED,
            )
            .unwrap();
    }

    fn swarm_messages(hive: &Hive, task: TaskId) -> Vec<crate::TaskMessage> {
        hive.store
            .task_messages(task)
            .unwrap()
            .into_iter()
            .filter(|message| message.sender == MessageParty::Swarm)
            .collect()
    }

    fn raise_all(hive: &Hive, now: i64) -> Vec<MergedWorkWaitingRaise> {
        hive.store
            .merged_work_waiting_candidates(now)
            .unwrap()
            .iter()
            .map(|candidate| {
                hive.store
                    .record_merged_work_waiting_attention(candidate, now)
                    .unwrap()
            })
            .collect()
    }

    fn waiting_rows(hive: &Hive, now: i64) -> Vec<crate::CoordinatorAttention> {
        hive.store
            .current_coordinator_attention(now)
            .unwrap()
            .into_iter()
            .filter(|row| row.kind == "merged_work_waiting_attention")
            .collect()
    }

    #[test]
    fn a_stay_is_read_while_it_lasts_and_once_more_after_it_ends() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let reads = hive.store.release_wait_reads(PARKED + 60).unwrap();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].left_at, None);
        assert_eq!(reads[0].parked_at, PARKED);
        assert_eq!(reads[0].commits, vec!["000000000012".to_owned()]);
        record_merge(&hive, task, MERGED, PARKED + 60);
        assert_eq!(
            hive.store.release_wait_reads(PARKED + 120).unwrap().len(),
            1,
            "a live stay is read on every sweep"
        );

        hive.store.transition_task(task, TaskState::Active).unwrap();
        at(&hive, task, PARKED + 900);
        let reads = hive.store.release_wait_reads(PARKED + 1_000).unwrap();
        assert_eq!(reads.len(), 1, "its last read came before it ended");
        assert_eq!(reads[0].left_at, Some(PARKED + 900));
        hive.store
            .record_release_waits(
                &[(reads[0].clone(), ReleaseWaitMerge::NotMerged)],
                PARKED + 1_000,
            )
            .unwrap();
        assert!(
            hive.store
                .release_wait_reads(PARKED + 1_100)
                .unwrap()
                .is_empty(),
            "an ended stay read after it ended is final"
        );
        let left: Option<i64> = hive
            .store
            .connection()
            .unwrap()
            .query_row("SELECT left_at FROM release_waits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, Some(PARKED + 900));
    }

    #[test]
    fn a_stay_released_between_the_read_and_the_write_is_still_closed() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let read = hive
            .store
            .release_wait_reads(PARKED + 60)
            .unwrap()
            .remove(0);
        hive.store.transition_task(task, TaskState::Active).unwrap();
        at(&hive, task, PARKED + 90);
        hive.store
            .record_release_waits(
                &[(
                    read,
                    ReleaseWaitMerge::Merged {
                        merged_at: MERGED,
                        merge_commit: MERGE_COMMIT.to_owned(),
                    },
                )],
                PARKED + 60,
            )
            .unwrap();
        let left: Option<i64> = hive
            .store
            .connection()
            .unwrap()
            .query_row("SELECT left_at FROM release_waits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            left,
            Some(PARKED + 90),
            "the end comes from the trail, not the read"
        );
    }

    #[test]
    fn stays_that_ended_before_the_window_are_dropped() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        record_merge(&hive, task, MERGED, PARKED + 60);
        hive.store.transition_task(task, TaskState::Active).unwrap();
        at(&hive, task, PARKED + 900);
        hive.store
            .record_release_waits(&[], PARKED + 900 + RELEASE_WAIT_WINDOW_SECONDS + 1)
            .unwrap();
        let rows: i64 = hive
            .store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM release_waits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn work_others_wait_on_is_named_thirty_minutes_after_its_merge_and_not_before() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
        waits_on(&hive, waiting.id, task);
        record_merge(&hive, task, MERGED, MERGED + 60);

        assert!(
            hive.store
                .merged_work_waiting_candidates(MERGED + 29 * 60)
                .unwrap()
                .is_empty(),
            "the clock runs from the merge, not from the park ten minutes earlier"
        );
        let now = MERGED + 31 * 60;
        let raised = raise_all(&hive, now);
        assert_eq!(
            raised,
            vec![MergedWorkWaitingRaise {
                raised: true,
                reminded: true,
                reminder_deferred: false,
            }]
        );
        let rows = waiting_rows(&hive, now);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_id, Some(task));
        assert_eq!(rows[0].worker_id, hive.worker);
        assert!(
            rows[0].reason.contains("unfinished work waits on it"),
            "{}",
            rows[0].reason
        );
        assert!(
            rows[0].reason.contains(&waiting.id.to_string()),
            "{}",
            rows[0].reason
        );
        assert!(
            rows[0].reason.contains("5c45d04c1a2b"),
            "{}",
            rows[0].reason
        );
        assert!(rows[0].reason.contains("Swarm has not told the operator"));

        let reminders = swarm_messages(&hive, task);
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].recipient, MessageParty::Worker);
        assert_eq!(
            reminders[0].recipient_worker_id,
            Some(hive.worker.to_string())
        );
        assert!(reminders[0].body.contains("authorises nothing"));

        assert!(
            raise_all(&hive, now + 600)
                .iter()
                .all(|raise| !raise.raised && !raise.reminded),
            "a second pass raises nothing"
        );
        assert_eq!(
            swarm_messages(&hive, task).len(),
            1,
            "one reminder per stay"
        );
    }

    #[test]
    fn merged_work_nothing_waits_on_is_named_only_past_the_slow_tail() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        record_merge(&hive, task, MERGED, MERGED + 60);
        assert!(
            hive.store
                .merged_work_waiting_candidates(MERGED + 23 * 3_600)
                .unwrap()
                .is_empty(),
            "an unmeasured repository waits a day"
        );
        let candidates = hive
            .store
            .merged_work_waiting_candidates(MERGED + 24 * 3_600 + 1)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].trigger, ReleaseWaitTrigger::SlowTail);
        assert!(!candidates[0].slow_tail_measured);
    }

    #[test]
    fn a_measured_repository_uses_its_own_ninetieth_percentile() {
        let hive = hive();
        // Twenty ended stays that waited 1..=20 minutes after their merge.
        let earlier = (1..=20_i64)
            .map(|minutes| {
                (
                    minutes,
                    hive.store.create_task("Earlier", WORKSPACE).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        {
            let connection = hive.store.connection().unwrap();
            for (minutes, task) in earlier {
                connection
                    .execute(
                        "INSERT INTO release_waits (task_id, parked_sequence, parked_at, workspace,
                             merged_at, merge_commit, left_at, read_at)
                         VALUES (?1, ?2, ?3, ?4, ?3, 'abcdef1234', ?5, ?5)",
                        params![
                            task.id.to_string(),
                            minutes,
                            PARKED - 86_400,
                            WORKSPACE,
                            PARKED - 86_400 + minutes * 60,
                        ],
                    )
                    .unwrap();
            }
        }
        let task = park(&hive, "Ship the fix", PARKED);
        record_merge(&hive, task, MERGED, MERGED + 60);
        assert!(
            hive.store
                .merged_work_waiting_candidates(MERGED + 18 * 60)
                .unwrap()
                .is_empty(),
            "18 minutes is this repository's 90th percentile, not past it"
        );
        let candidates = hive
            .store
            .merged_work_waiting_candidates(MERGED + 18 * 60 + 1)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].slow_tail_measured);
        assert_eq!(candidates[0].slow_tail_seconds, 18 * 60);
    }

    #[test]
    fn one_row_per_workspace_names_the_longest_wait_and_the_others() {
        let hive = hive();
        let first = park(&hive, "First", PARKED);
        let second = park(&hive, "Second fix", PARKED + 60);
        let (_, other_session) = another_worker(&hive, "Wren", "/workspace/other");
        let elsewhere = park_by(&hive, other_session, "Elsewhere", PARKED);
        for task in [first, second, elsewhere] {
            let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
            waits_on(&hive, waiting.id, task);
        }
        record_merge(&hive, second, MERGED, MERGED + 60);
        record_merge(&hive, first, MERGED + 300, MERGED + 360);
        record_merge(&hive, elsewhere, MERGED, MERGED + 60);
        let now = MERGED + 3_600;
        let candidates = hive.store.merged_work_waiting_candidates(now).unwrap();
        assert_eq!(candidates.len(), 2, "one per workspace: {candidates:?}");
        let here = candidates
            .iter()
            .find(|candidate| candidate.workspace == WORKSPACE)
            .unwrap();
        assert_eq!(
            here.task_id, second,
            "merged earlier, so it has waited longer"
        );
        assert_eq!(here.also_waiting, vec![first]);
        raise_all(&hive, now);
        assert_eq!(waiting_rows(&hive, now).len(), 2);
        assert!(
            hive.store
                .merged_work_waiting_candidates(now + 60)
                .unwrap()
                .is_empty(),
            "a workspace with a row is not raised again"
        );
        assert!(
            swarm_messages(&hive, first).is_empty(),
            "one reminder per release line"
        );
    }

    #[test]
    fn the_release_clears_the_row_and_cancels_a_reminder_still_queued() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
        waits_on(&hive, waiting.id, task);
        record_merge(&hive, task, MERGED, MERGED + 60);
        let now = MERGED + 3_600;
        raise_all(&hive, now);
        assert_eq!(waiting_rows(&hive, now).len(), 1);

        hive.store.transition_task(task, TaskState::Active).unwrap();
        assert!(
            waiting_rows(&hive, now).is_empty(),
            "leaving Awaiting release clears it"
        );
        hive.store.claim_task_messages(now + 1).unwrap();
        let reminder = &swarm_messages(&hive, task)[0];
        let state: String = hive
            .store
            .connection()
            .unwrap()
            .query_row(
                "SELECT state FROM task_message_deliveries WHERE message_id = ?1",
                [&reminder.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "cancelled", "its reason passed before it arrived");
    }

    #[test]
    fn a_reminder_is_not_a_message_that_holds_settlement() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
        waits_on(&hive, waiting.id, task);
        record_merge(&hive, task, MERGED, MERGED + 60);
        raise_all(&hive, MERGED + 3_600);
        let owed: bool = hive
            .store
            .connection()
            .unwrap()
            .query_row(
                concat!(
                    "SELECT EXISTS(SELECT 1 FROM tasks task WHERE task.id = ?1 AND ",
                    crate::message_delivery::message_owed_to_assignee!(),
                    ")"
                ),
                [task.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!owed);
    }

    #[test]
    fn a_full_message_queue_defers_the_reminder_and_the_next_pass_sends_it_once() {
        let hive = hive();
        let task = park(&hive, "Ship the fix", PARKED);
        let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
        waits_on(&hive, waiting.id, task);
        record_merge(&hive, task, MERGED, MERGED + 60);
        let filler = hive.store.create_task("Busy", WORKSPACE).unwrap();
        // The whole queue, written at once: sending 4,096 one by one counts
        // the queue 4,096 times.
        hive.store
            .connection()
            .unwrap()
            .execute_batch(&format!(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {limit})
                 INSERT INTO task_messages (id, task_id, sender, recipient, recipient_worker_id, body)
                     SELECT 'filler-' || i, '{task}', 'queen', 'worker', '{worker}', 'question'
                     FROM n;
                 INSERT INTO task_message_deliveries (message_id, state, updated_at)
                     SELECT id, 'queued', 0 FROM task_messages WHERE id LIKE 'filler-%';",
                limit = crate::TASK_MESSAGE_QUEUE_LIMIT,
                task = filler.id,
                worker = hive.worker,
            ))
            .unwrap();
        let queued = ["filler-1".to_owned()];
        let now = MERGED + 3_600;
        assert_eq!(
            raise_all(&hive, now),
            vec![MergedWorkWaitingRaise {
                raised: true,
                reminded: false,
                reminder_deferred: true,
            }]
        );
        assert_eq!(waiting_rows(&hive, now).len(), 1, "Queen is told anyway");
        assert!(swarm_messages(&hive, task).is_empty());

        hive.store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_message_deliveries SET state = 'delivered' WHERE message_id = ?1",
                [&queued[0]],
            )
            .unwrap();
        let retried = raise_all(&hive, now + 60);
        assert_eq!(
            retried,
            vec![MergedWorkWaitingRaise {
                raised: false,
                reminded: true,
                reminder_deferred: false,
            }]
        );
        raise_all(&hive, now + 120);
        assert_eq!(swarm_messages(&hive, task).len(), 1);
    }

    #[test]
    fn a_reopened_store_raises_nothing_twice() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let task;
        {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker("Petal", ProviderKind::ClaudeCode, WORKSPACE, false, 1)
                .unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            let hive = Hive {
                store,
                worker: worker.id,
                session,
            };
            task = park(&hive, "Ship the fix", PARKED);
            let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
            waits_on(&hive, waiting.id, task);
            record_merge(&hive, task, MERGED, MERGED + 60);
            raise_all(&hive, MERGED + 3_600);
        }
        let store = TaskStore::open(&path).unwrap();
        for candidate in store
            .merged_work_waiting_candidates(MERGED + 3_700)
            .unwrap()
        {
            let raise = store
                .record_merged_work_waiting_attention(&candidate, MERGED + 3_700)
                .unwrap();
            assert!(!raise.raised && !raise.reminded);
        }
        let reminders = store
            .task_messages(task)
            .unwrap()
            .into_iter()
            .filter(|message| message.sender == MessageParty::Swarm)
            .count();
        assert_eq!(reminders, 1);
    }

    #[test]
    fn queen_names_the_releaser_who_is_reminded_past_their_active_work_and_may_answer() {
        let hive = hive();
        let (releaser, releaser_session) = another_worker(&hive, "Lark", WORKSPACE);
        let task = reviewed_task(&hive, "Ship the fix");
        hive.store
            .transition_task_naming_releaser(
                task,
                TaskState::AwaitingRelease,
                "Lark releases this line",
                Some(releaser),
                &TaskActivityActor::system(),
            )
            .unwrap();
        at(&hive, task, PARKED);
        // The releaser is busy on work of its own.
        let own = hive
            .store
            .create_task("Lark's own work", WORKSPACE)
            .unwrap();
        hive.store
            .transition_task(own.id, TaskState::Ready)
            .unwrap();
        hive.store.assign_task(own.id, releaser_session).unwrap();
        hive.store
            .transition_task(own.id, TaskState::Active)
            .unwrap();
        let waiting = hive.store.create_task("Builds on it", WORKSPACE).unwrap();
        waits_on(&hive, waiting.id, task);
        record_merge(&hive, task, MERGED, MERGED + 60);

        let now = MERGED + 3_600;
        raise_all(&hive, now);
        let rows = waiting_rows(&hive, now);
        assert_eq!(rows[0].worker_id, releaser);
        assert!(
            rows[0].reason.contains("Lark (you named them)"),
            "{}",
            rows[0].reason
        );
        let reminder = &swarm_messages(&hive, task)[0];
        assert_eq!(reminder.recipient_worker_id, Some(releaser.to_string()));
        assert!(
            hive.store
                .pending_task_message_dispatches()
                .unwrap()
                .iter()
                .any(|dispatch| dispatch.message_id == reminder.id
                    && dispatch.sender_name == "Swarm"),
            "not held behind the releaser's own Active task, and signed Swarm"
        );
        hive.store
            .message_queen_from_worker(task, releaser, "It waits on the tag build.", None, now)
            .expect("the named releaser may tell Queen what it waits on");

        hive.store.transition_task(task, TaskState::Active).unwrap();
        assert!(
            hive.store
                .message_queen_from_worker(task, releaser, "Still waiting.", None, now)
                .is_err(),
            "the name lasts one stay"
        );
        let named: i64 = hive
            .store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM task_releasers", [], |row| row.get(0))
            .unwrap();
        assert_eq!(named, 0);
    }

    #[test]
    fn a_releaser_is_named_only_when_parking_and_only_a_worker() {
        let hive = hive();
        let task = reviewed_task(&hive, "Ship the fix");
        assert!(matches!(
            hive.store.transition_task_naming_releaser(
                task,
                TaskState::Active,
                "",
                Some(hive.worker),
                &TaskActivityActor::system(),
            ),
            Err(TaskStoreError::ReleaserOnlyWhenParking)
        ));
        let queen = hive.store.ensure_queen(WORKSPACE).unwrap();
        assert_eq!(queen.role, WorkerRole::Queen);
        assert!(matches!(
            hive.store.transition_task_naming_releaser(
                task,
                TaskState::AwaitingRelease,
                "",
                Some(queen.id),
                &TaskActivityActor::system(),
            ),
            Err(TaskStoreError::ReleaserNotAWorker)
        ));
        assert_eq!(
            hive.store.get_task(task).unwrap().state,
            TaskState::Review,
            "a refused name parks nothing"
        );
    }

    #[test]
    fn swarm_writes_only_to_a_worker() {
        let hive = hive();
        let task = hive.store.create_task("Anything", WORKSPACE).unwrap();
        for (from, to) in [
            (MessageEnd::swarm(), MessageEnd::queen()),
            (MessageEnd::queen(), MessageEnd::swarm()),
            (MessageEnd::worker(hive.worker), MessageEnd::swarm()),
        ] {
            assert!(matches!(
                hive.store
                    .send_task_message(task.id, from, to, "hello", PARKED),
                Err(TaskStoreError::InvalidTaskMessage { .. })
            ));
        }
        let refused = hive.store.connection().unwrap().execute(
            "INSERT INTO task_messages (id, task_id, sender, recipient, body)
             VALUES ('m', ?1, 'swarm', 'queen', 'hello')",
            [task.id.to_string()],
        );
        assert!(refused.is_err(), "the schema refuses it too");
    }
}
