//! Paths a task's work must not touch (ADR 0172).
//!
//! One row per task that has any. Every change writes an attributed activity
//! row in the same transaction, so who set the patterns, and who changed them
//! later, is on the task's record. Who may change them is the application's
//! rule; this module stores what it is handed.

use std::collections::HashMap;

use rusqlite::{OptionalExtension as _, params};
use swarm_domain::{
    ControlRoomEventKind, MustNotTouchPatterns, Task, TaskActivityActor, TaskId, TaskPriority,
    validate_must_not_touch,
};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

pub(crate) fn migrate_must_not_touch(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_must_not_touch (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             patterns TEXT NOT NULL CHECK (length(patterns) BETWEEN 2 AND 20000),
             set_by_kind TEXT NOT NULL,
             set_by_id TEXT,
             set_at INTEGER NOT NULL
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// What became of a request to change a task's patterns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MustNotTouchChange {
    Changed {
        before: MustNotTouchPatterns,
        after: MustNotTouchPatterns,
    },
    /// The task already carried exactly these patterns. Nothing was written.
    Unchanged,
    /// The task is finished, so nothing will be compared with them again.
    Closed,
}

/// How much of a pattern list one activity note spells out. The full list is
/// in the table; the note says how many it left out.
const LISTED_BYTES: usize = 1_000;

fn listed(patterns: &MustNotTouchPatterns) -> String {
    let all = patterns.as_slice();
    if all.is_empty() {
        return "none".to_owned();
    }
    let mut text = String::new();
    for (shown, pattern) in all.iter().enumerate() {
        if !text.is_empty() && text.len() + 2 + pattern.len() > LISTED_BYTES {
            return format!("{text} and {} more", all.len() - shown);
        }
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(pattern);
    }
    text
}

/// Keeps a note within what every other activity note is held to.
fn within_note_limit(mut note: String) -> String {
    let limit = crate::MAX_TASK_ACTIVITY_NOTE_BYTES;
    if note.len() > limit {
        let mut end = limit - '…'.len_utf8();
        while !note.is_char_boundary(end) {
            end -= 1;
        }
        note.truncate(end);
        note.push('…');
    }
    note
}

fn write_patterns(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    patterns: &MustNotTouchPatterns,
    actor: &TaskActivityActor,
    now: i64,
) -> Result<(), TaskStoreError> {
    if patterns.is_empty() {
        transaction.execute(
            "DELETE FROM task_must_not_touch WHERE task_id = ?1",
            [task_id.to_string()],
        )?;
        return Ok(());
    }
    let encoded = serde_json::to_string(patterns.as_slice())
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
    transaction.execute(
        "INSERT INTO task_must_not_touch (task_id, patterns, set_by_kind, set_by_id, set_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(task_id) DO UPDATE SET
             patterns = excluded.patterns,
             set_by_kind = excluded.set_by_kind,
             set_by_id = excluded.set_by_id,
             set_at = excluded.set_at",
        params![
            task_id.to_string(),
            encoded,
            actor.kind.to_string(),
            actor.id.as_deref(),
            now
        ],
    )?;
    Ok(())
}

fn note(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    note: &str,
    actor: &TaskActivityActor,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
         VALUES (?1, 'noted', ?2, ?3, ?4)",
        params![
            task_id.to_string(),
            within_note_limit(note.to_owned()),
            actor.kind.to_string(),
            actor.id.as_deref()
        ],
    )?;
    insert_control_room_event(transaction, ControlRoomEventKind::TasksChanged)?;
    Ok(())
}

fn read_patterns(
    connection: &rusqlite::Connection,
    task_id: TaskId,
) -> Result<MustNotTouchPatterns, TaskStoreError> {
    let encoded: Option<String> = connection
        .query_row(
            "SELECT patterns FROM task_must_not_touch WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(encoded) = encoded else {
        return Ok(MustNotTouchPatterns::default());
    };
    decode(&encoded)
}

fn decode(encoded: &str) -> Result<MustNotTouchPatterns, TaskStoreError> {
    let raw: Vec<String> = serde_json::from_str(encoded).map_err(|error| {
        TaskStoreError::IntegrityFailure(format!("stored must-not-touch patterns: {error}"))
    })?;
    validate_must_not_touch(&raw).map_err(|reason| {
        TaskStoreError::IntegrityFailure(format!("stored must-not-touch patterns: {reason}"))
    })
}

impl TaskStore {
    /// Files a draft that carries must-not-touch patterns from the start. The
    /// task and its patterns are written together, so a filed task never
    /// exists without the limits its filer gave it. They are dated with the
    /// task's own creation time.
    ///
    /// # Errors
    /// Returns an error for invalid task content or unavailable persistence.
    pub fn create_task_with_must_not_touch_as(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
        actor: &TaskActivityActor,
        patterns: &MustNotTouchPatterns,
    ) -> Result<Task, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = crate::insert_draft(
            &transaction,
            hive_id,
            title,
            description,
            priority,
            workspace,
            actor,
        )?;
        if !patterns.is_empty() {
            let created_at: i64 = transaction.query_row(
                "SELECT created_at FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )?;
            write_patterns(&transaction, id, patterns, actor, created_at)?;
            note(
                &transaction,
                id,
                &format!("Must not touch: {}.", listed(patterns)),
                actor,
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Replaces a task's patterns. An empty list removes them.
    ///
    /// # Errors
    /// Returns an error when the task does not exist or persistence is
    /// unavailable.
    pub fn set_must_not_touch(
        &self,
        task_id: TaskId,
        patterns: &MustNotTouchPatterns,
        reason: &str,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<MustNotTouchChange, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(state) = state else {
            return Err(TaskStoreError::NotFound);
        };
        if state == "completed" || state == "abandoned" {
            return Ok(MustNotTouchChange::Closed);
        }
        let before = read_patterns(&transaction, task_id)?;
        if &before == patterns {
            return Ok(MustNotTouchChange::Unchanged);
        }
        write_patterns(&transaction, task_id, patterns, actor, now)?;
        note(
            &transaction,
            task_id,
            &format!(
                "Must not touch changed to: {} (was: {}). {}",
                listed(patterns),
                listed(&before),
                reason.trim()
            ),
            actor,
        )?;
        transaction.commit()?;
        Ok(MustNotTouchChange::Changed {
            before,
            after: patterns.clone(),
        })
    }

    /// Patterns for each of these tasks that has any, in one read, for
    /// listings that show many tasks at once.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a stored row no
    /// longer reads as valid patterns.
    pub fn must_not_touch_for_tasks(
        &self,
        ids: &[TaskId],
    ) -> Result<HashMap<TaskId, MustNotTouchPatterns>, TaskStoreError> {
        let mut found = HashMap::new();
        if ids.is_empty() {
            return Ok(found);
        }
        let connection = self.connection()?;
        let placeholders = vec!["?"; ids.len()].join(",");
        let mut statement = connection.prepare(&format!(
            "SELECT task_id, patterns FROM task_must_not_touch WHERE task_id IN ({placeholders})"
        ))?;
        let bound = ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (task_id, encoded) = row?;
            let task_id = crate::parse_domain_id::<TaskId>(&task_id)?;
            found.insert(task_id, decode(&encoded)?);
        }
        Ok(found)
    }

    /// The task's patterns, empty when it has none.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a stored row no
    /// longer reads as valid patterns.
    pub fn task_must_not_touch(
        &self,
        task_id: TaskId,
    ) -> Result<MustNotTouchPatterns, TaskStoreError> {
        let connection = self.connection()?;
        read_patterns(&connection, task_id)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        MustNotTouchPatterns, TaskActivityActor, TaskPriority, TaskState, WorkerId,
        validate_must_not_touch,
    };

    use super::MustNotTouchChange;
    use crate::{TaskStore, TaskStoreError};

    fn patterns(raw: &[&str]) -> MustNotTouchPatterns {
        let raw: Vec<String> = raw.iter().map(|pattern| (*pattern).to_owned()).collect();
        validate_must_not_touch(&raw).unwrap()
    }

    fn notes(store: &TaskStore, task: swarm_domain::TaskId) -> Vec<(String, String)> {
        let connection = store.connection().unwrap();
        let mut statement = connection
            .prepare(
                "SELECT note, actor_kind FROM task_activity
                  WHERE task_id = ?1 AND kind = 'noted' ORDER BY sequence",
            )
            .unwrap();
        statement
            .query_map([task.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn a_filed_task_carries_its_patterns_and_says_who_set_them() {
        let store = TaskStore::in_memory().unwrap();
        let filer = TaskActivityActor::worker(WorkerId::new());

        let task = store
            .create_task_with_must_not_touch_as(
                "Tidy the board",
                "",
                TaskPriority::Normal,
                "/work/petal",
                &filer,
                &patterns(&["crates/swarm-terminal/**", "web/node_modules/**"]),
            )
            .unwrap();

        assert_eq!(task.state, TaskState::Draft);
        assert_eq!(
            store.task_must_not_touch(task.id).unwrap(),
            patterns(&["crates/swarm-terminal/**", "web/node_modules/**"])
        );
        assert_eq!(
            notes(&store, task.id),
            [(
                "Must not touch: crates/swarm-terminal/**, web/node_modules/**.".to_owned(),
                "worker".to_owned()
            )]
        );
    }

    #[test]
    fn a_task_without_patterns_has_no_row_and_no_note() {
        let store = TaskStore::in_memory().unwrap();
        let plain = store.create_task("Plain", "/work/petal").unwrap();
        let filed = store
            .create_task_with_must_not_touch_as(
                "Filed without limits",
                "",
                TaskPriority::Normal,
                "/work/petal",
                &TaskActivityActor::operator(),
                &MustNotTouchPatterns::default(),
            )
            .unwrap();

        for task in [plain.id, filed.id] {
            assert!(store.task_must_not_touch(task).unwrap().is_empty());
            assert!(notes(&store, task).is_empty());
        }
        let rows: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM task_must_not_touch", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn a_change_is_recorded_with_before_after_and_reason_and_empty_clears() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Scoped", "/work/petal").unwrap();
        let queen = TaskActivityActor::worker(WorkerId::new());

        let set = store
            .set_must_not_touch(task.id, &patterns(&["web/**"]), "Routing.", &queen, 20)
            .unwrap();
        assert_eq!(
            set,
            MustNotTouchChange::Changed {
                before: MustNotTouchPatterns::default(),
                after: patterns(&["web/**"])
            }
        );
        assert_eq!(
            store
                .set_must_not_touch(task.id, &patterns(&["web/**"]), "Again.", &queen, 21)
                .unwrap(),
            MustNotTouchChange::Unchanged
        );
        store
            .set_must_not_touch(
                task.id,
                &MustNotTouchPatterns::default(),
                "The worker asked; the web change is needed.",
                &queen,
                22,
            )
            .unwrap();

        assert!(store.task_must_not_touch(task.id).unwrap().is_empty());
        let written: Vec<String> = notes(&store, task.id).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            written,
            [
                "Must not touch changed to: web/** (was: none). Routing.",
                "Must not touch changed to: none (was: web/**). The worker asked; the web change is needed."
            ]
        );
    }

    #[test]
    fn a_closed_or_missing_task_is_not_changed() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Done already", "/work/petal").unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET state = 'abandoned' WHERE id = ?1",
                [task.id.to_string()],
            )
            .unwrap();
        let actor = TaskActivityActor::operator();

        assert_eq!(
            store
                .set_must_not_touch(task.id, &patterns(&["web/**"]), "Late.", &actor, 30)
                .unwrap(),
            MustNotTouchChange::Closed
        );
        assert!(store.task_must_not_touch(task.id).unwrap().is_empty());
        assert!(matches!(
            store.set_must_not_touch(
                swarm_domain::TaskId::new(),
                &patterns(&["web/**"]),
                "Nowhere.",
                &actor,
                30
            ),
            Err(TaskStoreError::NotFound)
        ));
    }

    /// A row that no longer reads as valid patterns is reported, not shown as
    /// "no limits": a silent empty would hide a crossing from Queen.
    #[test]
    fn a_damaged_row_is_an_integrity_failure_not_an_empty_list() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Damaged", "/work/petal").unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO task_must_not_touch (task_id, patterns, set_by_kind, set_at)
                 VALUES (?1, '[\"../escape\"]', 'operator', 1)",
                [task.id.to_string()],
            )
            .unwrap();

        assert!(matches!(
            store.task_must_not_touch(task.id),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
    }

    #[test]
    fn a_listing_reads_every_task_s_patterns_at_once() {
        let store = TaskStore::in_memory().unwrap();
        let scoped = store.create_task("Scoped", "/work/petal").unwrap();
        let plain = store.create_task("Plain", "/work/petal").unwrap();
        store
            .set_must_not_touch(
                scoped.id,
                &patterns(&["web/**"]),
                "Routing.",
                &TaskActivityActor::operator(),
                5,
            )
            .unwrap();

        let found = store
            .must_not_touch_for_tasks(&[scoped.id, plain.id])
            .unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[&scoped.id], patterns(&["web/**"]));
        assert!(store.must_not_touch_for_tasks(&[]).unwrap().is_empty());
    }

    /// The full lists stay in the table; the note that records a change stays
    /// within the limit every other activity note is held to.
    #[test]
    fn a_change_of_many_long_patterns_writes_a_bounded_note() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Wide", "/work/petal").unwrap();
        let long: Vec<String> = (0..swarm_domain::MAX_MUST_NOT_TOUCH_PATTERNS)
            .map(|n| format!("{n:02}/{}/**", "x".repeat(240)))
            .collect();
        let wide = validate_must_not_touch(&long).unwrap();

        store
            .set_must_not_touch(
                task.id,
                &wide,
                // The longest reason Queen may give, in four-byte characters.
                &"🌸".repeat(1_000),
                &TaskActivityActor::operator(),
                5,
            )
            .unwrap();

        assert_eq!(store.task_must_not_touch(task.id).unwrap(), wide);
        let (written, _) = notes(&store, task.id).pop().unwrap();
        assert!(
            written.len() <= crate::MAX_TASK_ACTIVITY_NOTE_BYTES,
            "{}",
            written.len()
        );
        assert!(written.contains(" more (was: none)"), "{written}");
    }

    /// Recovery: the patterns are durable state, not a cache, and come back
    /// from the database file after the store is reopened.
    #[test]
    fn patterns_survive_reopening_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let task_id = {
            let store = TaskStore::open(&path).unwrap();
            let task = store.create_task("Durable", "/work/petal").unwrap();
            store
                .set_must_not_touch(
                    task.id,
                    &patterns(&["packaging/**"]),
                    "Routing.",
                    &TaskActivityActor::operator(),
                    40,
                )
                .unwrap();
            task.id
        };

        let reopened = TaskStore::open(&path).unwrap();
        assert_eq!(
            reopened.task_must_not_touch(task_id).unwrap(),
            patterns(&["packaging/**"])
        );
    }
}
