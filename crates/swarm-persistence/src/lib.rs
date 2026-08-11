use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{Task, TaskDetailsUpdate, TaskId, TaskPriority, TaskState, WorkerSessionId};
use thiserror::Error;
use uuid::Uuid;

mod workers;
const MAX_TASK_TITLE_BYTES: usize = 240;
const MAX_TASK_DESCRIPTION_BYTES: usize = 10_000;
const MAX_WORKSPACE_BYTES: usize = 4096;
const CURRENT_SCHEMA_VERSION: i64 = 3;

#[derive(Clone)]
pub struct TaskStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Error)]
pub enum TaskStoreError {
    #[error("task persistence filesystem failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("task persistence failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("task persistence lock was poisoned")]
    LockPoisoned,
    #[error("task was not found")]
    NotFound,
    #[error("task title must contain 1 to {MAX_TASK_TITLE_BYTES} bytes")]
    InvalidTitle,
    #[error("task description must not exceed {MAX_TASK_DESCRIPTION_BYTES} bytes")]
    InvalidDescription,
    #[error("task details update must contain at least one field")]
    EmptyTaskDetailsUpdate,
    #[error("workspace must contain 1 to {MAX_WORKSPACE_BYTES} bytes")]
    InvalidWorkspace,
    #[error("task cannot move from {from} to {to}")]
    InvalidTransition { from: TaskState, to: TaskState },
    #[error("completed tasks cannot be assigned")]
    CompletedTask,
    #[error("worker was not found")]
    WorkerNotFound,
    #[error("worker name is invalid")]
    InvalidWorkerName,
    #[error("worker name already exists")]
    DuplicateWorkerName,
    #[error("the Queen profile already exists")]
    QueenAlreadyExists,
    #[error("worker already has an active session")]
    WorkerAlreadyRunning,
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("database integrity check failed: {0}")]
    IntegrityFailure(String),
}

impl TaskStore {
    /// Opens, migrates, and integrity-checks a file-backed task database.
    ///
    /// # Errors
    /// Returns an error when the path, schema, migration, or integrity check is invalid.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    /// Opens a migrated in-memory store for isolated tests and ephemeral runtimes.
    ///
    /// # Errors
    /// Returns an error when `SQLite` initialization or migration fails.
    pub fn in_memory() -> Result<Self, TaskStoreError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, TaskStoreError> {
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match schema_version {
            0 => {
                let transaction = connection.transaction()?;
                transaction.execute_batch(
                    "
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                workspace TEXT NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed')),
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS task_assignments (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                worker_session_id TEXT NOT NULL,
                assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                released_at INTEGER
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_active_assignment_per_task
                ON task_assignments(task_id) WHERE released_at IS NULL;
            CREATE TABLE IF NOT EXISTS task_activity (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                from_state TEXT,
                to_state TEXT,
                occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            ",
                )?;
                migrate_worker_roster(&transaction)?;
                migrate_task_details(&transaction)?;
                transaction.commit()?;
            }
            1 => {
                let transaction = connection.transaction()?;
                migrate_worker_roster(&transaction)?;
                migrate_task_details(&transaction)?;
                transaction.commit()?;
            }
            2 => {
                let transaction = connection.transaction()?;
                migrate_task_details(&transaction)?;
                transaction.commit()?;
            }
            CURRENT_SCHEMA_VERSION => {}
            found => {
                return Err(TaskStoreError::UnsupportedSchemaVersion {
                    found,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
        }
        let integrity: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if integrity != "ok" {
            return Err(TaskStoreError::IntegrityFailure(integrity));
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Creates a validated draft and its first activity event atomically.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task(&self, title: &str, workspace: &str) -> Result<Task, TaskStoreError> {
        self.create_task_with_details(title, "", TaskPriority::Normal, workspace)
    }

    /// Creates a validated draft with operator-facing context and priority.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task_with_details(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, TaskStoreError> {
        let title = title.trim();
        let description = description.trim();
        let workspace = workspace.trim();
        validate_text(title, workspace)?;
        validate_description(description)?;
        let id = TaskId::new();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO tasks (id, title, description, priority, workspace, state)
             VALUES (?1, ?2, ?3, ?4, ?5, 'draft')",
            params![
                id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state) VALUES (?1, 'created', 'draft')",
            [id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Lists tasks with their current active assignment.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT t.id, t.title, t.description, t.priority, t.workspace, t.state, a.worker_session_id,
                   t.created_at, t.updated_at
            FROM tasks t
            LEFT JOIN task_assignments a
              ON a.task_id = t.id AND a.released_at IS NULL
            ORDER BY CASE t.state WHEN 'completed' THEN 1 ELSE 0 END,
                     CASE t.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1
                                     WHEN 'normal' THEN 2 ELSE 3 END,
                     t.updated_at DESC, t.id DESC
            ",
        )?;
        statement
            .query_map([], task_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Loads one task and its current active assignment.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown task or a persistence error.
    pub fn get_task(&self, id: TaskId) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT t.id, t.title, t.description, t.priority, t.workspace, t.state, a.worker_session_id,
                       t.created_at, t.updated_at
                FROM tasks t
                LEFT JOIN task_assignments a
                  ON a.task_id = t.id AND a.released_at IS NULL
                WHERE t.id = ?1
                ",
                [id.to_string()],
                task_from_row,
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)
    }

    /// Replaces the supplied task details and records one atomic activity event.
    ///
    /// # Errors
    /// Returns an error for an empty update, invalid content, an unknown task, or unavailable persistence.
    pub fn update_task_details(
        &self,
        id: TaskId,
        update: &TaskDetailsUpdate,
    ) -> Result<Task, TaskStoreError> {
        if update.title.is_none()
            && update.description.is_none()
            && update.priority.is_none()
            && update.workspace.is_none()
        {
            return Err(TaskStoreError::EmptyTaskDetailsUpdate);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT title, description, priority, workspace FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        let title = update
            .title
            .as_deref()
            .map_or(current.0.as_str(), str::trim);
        let description = update
            .description
            .as_deref()
            .map_or(current.1.as_str(), str::trim);
        let priority = update.priority.unwrap_or(
            TaskPriority::from_str(&current.2)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
        );
        let workspace = update
            .workspace
            .as_deref()
            .map_or(current.3.as_str(), str::trim);
        validate_text(title, workspace)?;
        validate_description(description)?;
        transaction.execute(
            "UPDATE tasks
             SET title = ?2, description = ?3, priority = ?4, workspace = ?5,
                 updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind) VALUES (?1, 'details_updated')",
            [id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Writes a consistent online backup to a separate `SQLite` file.
    ///
    /// # Errors
    /// Returns an error when the destination or `SQLite` backup operation fails.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = self.connection()?;
        connection.backup("main", path, None)?;
        Ok(())
    }

    /// Runs `SQLite`'s quick integrity check against the live database.
    ///
    /// # Errors
    /// Returns an integrity or persistence error when the check is not successful.
    pub fn verify_integrity(&self) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        let result: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(TaskStoreError::IntegrityFailure(result))
        }
    }

    /// Applies one permitted task state transition and records its activity atomically.
    ///
    /// # Errors
    /// Returns an error for an unknown task, rejected transition, or persistence failure.
    pub fn transition_task(&self, id: TaskId, target: TaskState) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let current = current.ok_or(TaskStoreError::NotFound)?;
        let current = TaskState::from_str(&current)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        if !current.can_transition_to(target) {
            return Err(TaskStoreError::InvalidTransition {
                from: current,
                to: target,
            });
        }
        transaction.execute(
            "UPDATE tasks SET state = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id.to_string(), target.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, from_state, to_state)
             VALUES (?1, 'state_changed', ?2, ?3)",
            params![id.to_string(), current.to_string(), target.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Replaces the current assignment with a running immutable worker-session identity.
    ///
    /// # Errors
    /// Returns an error for an unknown or completed task or unavailable persistence.
    pub fn assign_task(
        &self,
        id: TaskId,
        session_id: WorkerSessionId,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let state = state.ok_or(TaskStoreError::NotFound)?;
        if state == TaskState::Completed.to_string() {
            return Err(TaskStoreError::CompletedTask);
        }
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_assignments (id, task_id, worker_session_id)
             VALUES (?1, ?2, ?3)",
            params![
                Uuid::now_v7().to_string(),
                id.to_string(),
                session_id.to_string()
            ],
        )?;
        transaction.execute(
            "UPDATE tasks SET updated_at = unixepoch() WHERE id = ?1",
            [id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind) VALUES (?1, 'assigned')",
            [id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Ends every active assignment owned by one stopped worker session.
    ///
    /// # Errors
    /// Returns an error when the assignment history cannot be updated atomically.
    pub fn release_session_assignments(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut task_ids = {
            let mut statement = transaction.prepare(
                "SELECT task_id FROM task_assignments
                 WHERE worker_session_id = ?1 AND released_at IS NULL",
            )?;
            statement
                .query_map([session_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        task_ids.sort_unstable();
        for task_id in &task_ids {
            transaction.execute(
                "UPDATE task_assignments SET released_at = unixepoch()
                 WHERE task_id = ?1 AND worker_session_id = ?2 AND released_at IS NULL",
                params![task_id, session_id.to_string()],
            )?;
            transaction.execute(
                "UPDATE tasks SET updated_at = unixepoch() WHERE id = ?1",
                [task_id],
            )?;
            transaction.execute(
                "INSERT INTO task_activity (task_id, kind) VALUES (?1, 'unassigned')",
                [task_id],
            )?;
        }
        transaction.commit()?;
        Ok(task_ids.len())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
}

fn migrate_worker_roster(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS worker_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE UNIQUE,
            role TEXT NOT NULL CHECK (role IN ('queen','worker')),
            provider TEXT NOT NULL CHECK (provider IN ('claude_code','codex')),
            workspace TEXT NOT NULL,
            autostart INTEGER NOT NULL CHECK (autostart IN (0,1)),
            position INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_queen_profile
            ON worker_profiles(role) WHERE role = 'queen';
        CREATE TABLE IF NOT EXISTS worker_sessions (
            session_id TEXT PRIMARY KEY,
            worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
            started_at INTEGER NOT NULL DEFAULT (unixepoch()),
            ended_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_session_per_worker
            ON worker_sessions(worker_id) WHERE ended_at IS NULL;
        PRAGMA user_version = 2;
        ",
    )
}

fn migrate_task_details(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
         ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal'
             CHECK (priority IN ('low','normal','high','urgent'));
         PRAGMA user_version = 3;",
    )
}

fn validate_text(title: &str, workspace: &str) -> Result<(), TaskStoreError> {
    if title.is_empty() || title.len() > MAX_TASK_TITLE_BYTES {
        return Err(TaskStoreError::InvalidTitle);
    }
    if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(())
}

fn validate_description(description: &str) -> Result<(), TaskStoreError> {
    if description.len() > MAX_TASK_DESCRIPTION_BYTES {
        return Err(TaskStoreError::InvalidDescription);
    }
    Ok(())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get(0)?;
    let priority: String = row.get(3)?;
    let state: String = row.get(5)?;
    let assigned_session_id: Option<String> = row.get(6)?;
    Ok(Task {
        id: TaskId::from_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        title: row.get(1)?,
        description: row.get(2)?,
        priority: TaskPriority::from_str(&priority).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        workspace: row.get(4)?,
        state: TaskState::from_str(&state).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        assigned_session_id: assigned_session_id
            .map(|value| WorkerSessionId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_task_lifecycle_and_assignment() {
        let store = TaskStore::in_memory().unwrap();
        let created = store.create_task("Fix reload", "/workspace").unwrap();
        assert_eq!(created.state, TaskState::Draft);

        let ready = store.transition_task(created.id, TaskState::Ready).unwrap();
        let session_id = WorkerSessionId::new();
        let assigned = store.assign_task(ready.id, session_id).unwrap();
        assert_eq!(assigned.assigned_session_id, Some(session_id));
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        store.transition_task(ready.id, TaskState::Active).unwrap();
        store.transition_task(ready.id, TaskState::Review).unwrap();
        let completed = store
            .transition_task(ready.id, TaskState::Completed)
            .unwrap();
        assert_eq!(completed.state, TaskState::Completed);
        assert!(matches!(
            store.assign_task(ready.id, WorkerSessionId::new()),
            Err(TaskStoreError::CompletedTask)
        ));
    }

    #[test]
    fn updates_only_supplied_task_details_and_records_activity() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task_with_details(
                "Polish task cards",
                "Make priority visible",
                TaskPriority::High,
                "/workspace",
            )
            .unwrap();
        let updated = store
            .update_task_details(
                task.id,
                &TaskDetailsUpdate {
                    title: Some("Polish the task board".into()),
                    priority: Some(TaskPriority::Urgent),
                    ..TaskDetailsUpdate::default()
                },
            )
            .unwrap();

        assert_eq!(updated.title, "Polish the task board");
        assert_eq!(updated.description, "Make priority visible");
        assert_eq!(updated.priority, TaskPriority::Urgent);
        assert_eq!(updated.workspace, "/workspace");
        assert!(matches!(
            store.update_task_details(task.id, &TaskDetailsUpdate::default()),
            Err(TaskStoreError::EmptyTaskDetailsUpdate)
        ));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM task_activity WHERE task_id = ?1 AND kind = 'details_updated'",
                    [task.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn stopping_a_session_releases_its_assignments() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Assigned work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let session_id = WorkerSessionId::new();
        store.assign_task(task.id, session_id).unwrap();

        assert_eq!(store.release_session_assignments(session_id).unwrap(), 1);
        assert_eq!(store.get_task(task.id).unwrap().assigned_session_id, None);
        assert_eq!(store.release_session_assignments(session_id).unwrap(), 0);
    }

    #[test]
    fn rejects_skipped_transitions_and_invalid_content() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.create_task("", "/workspace"),
            Err(TaskStoreError::InvalidTitle)
        ));
        let task = store.create_task("A task", "/workspace").unwrap();
        assert!(matches!(
            store.transition_task(task.id, TaskState::Completed),
            Err(TaskStoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn reopens_file_database_without_losing_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let id = {
            let store = TaskStore::open(&path).unwrap();
            store
                .create_task("Persistent task", "/workspace")
                .unwrap()
                .id
        };
        let reopened = TaskStore::open(path).unwrap();
        assert_eq!(reopened.get_task(id).unwrap().title, "Persistent task");
    }

    #[test]
    fn migrates_the_task_only_schema_to_the_worker_roster() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                PRAGMA user_version = 1;
                ",
            )
            .unwrap();
        let store = TaskStore::from_connection(connection).unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        assert_eq!(queen.role, swarm_domain::WorkerRole::Queen);
        let columns = store
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"description".to_owned()));
        assert!(columns.contains(&"priority".to_owned()));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn reopens_schema_v3_for_safe_rollback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn backup_is_consistent_and_reopenable() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.sqlite3");
        let backup = directory.path().join("backups").join("snapshot.sqlite3");
        let store = TaskStore::open(source).unwrap();
        let task = store.create_task("Backed up", "/workspace").unwrap();
        store.backup_to(&backup).unwrap();

        let restored = TaskStore::open(backup).unwrap();
        restored.verify_integrity().unwrap();
        assert_eq!(restored.get_task(task.id).unwrap().title, "Backed up");
    }
}
