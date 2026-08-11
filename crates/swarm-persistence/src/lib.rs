use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    ApiaryId, ControlRoomEvent, ControlRoomEventKind, ControlRoomEventPage, Hive, HiveId,
    HiveIdentity, Operator, OperatorId, Task, TaskDetailsUpdate, TaskId, TaskPriority, TaskState,
    WorkerSessionId,
};
use thiserror::Error;
use uuid::Uuid;

mod workers;
const MAX_TASK_TITLE_BYTES: usize = 240;
const MAX_TASK_DESCRIPTION_BYTES: usize = 10_000;
const MAX_WORKSPACE_BYTES: usize = 4096;
const CURRENT_SCHEMA_VERSION: i64 = 5;
const MAX_CONTROL_ROOM_EVENTS: i64 = 4096;
const MAX_CONTROL_ROOM_EVENT_PAGE: usize = 128;

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
                migrate_hive_identity(&transaction)?;
                migrate_control_room_events(&transaction)?;
                transaction.commit()?;
            }
            1 => {
                let transaction = connection.transaction()?;
                migrate_worker_roster(&transaction)?;
                migrate_task_details(&transaction)?;
                migrate_hive_identity(&transaction)?;
                migrate_control_room_events(&transaction)?;
                transaction.commit()?;
            }
            2 => {
                let transaction = connection.transaction()?;
                migrate_task_details(&transaction)?;
                migrate_hive_identity(&transaction)?;
                migrate_control_room_events(&transaction)?;
                transaction.commit()?;
            }
            3 => {
                let transaction = connection.transaction()?;
                migrate_hive_identity(&transaction)?;
                migrate_control_room_events(&transaction)?;
                transaction.commit()?;
            }
            4 => {
                let transaction = connection.transaction()?;
                migrate_control_room_events(&transaction)?;
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

    /// Returns the durable operator and Hive owned by this local installation.
    ///
    /// # Errors
    /// Returns an error when identity persistence is unavailable or invalid.
    pub fn local_hive_identity(&self) -> Result<HiveIdentity, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT o.id, o.display_name, h.id, h.name, h.apiary_id
                FROM local_hive_identity l
                JOIN hives h ON h.id = l.hive_id
                JOIN operators o ON o.id = h.operator_id
                WHERE l.singleton = 1
                ",
                [],
                |row| {
                    let operator_id = parse_domain_id::<OperatorId>(&row.get::<_, String>(0)?)?;
                    let hive_id = parse_domain_id::<HiveId>(&row.get::<_, String>(2)?)?;
                    let apiary_id = row
                        .get::<_, Option<String>>(4)?
                        .map(|value| parse_domain_id::<ApiaryId>(&value))
                        .transpose()?;
                    Ok(HiveIdentity {
                        operator: Operator {
                            id: operator_id,
                            display_name: row.get(1)?,
                        },
                        hive: Hive {
                            id: hive_id,
                            name: row.get(3)?,
                            operator_id,
                            apiary_id,
                        },
                    })
                },
            )
            .map_err(TaskStoreError::from)
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
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO tasks (id, hive_id, title, description, priority, workspace, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'draft')",
            params![
                id.to_string(),
                hive_id.to_string(),
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
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
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
            SELECT t.id, t.hive_id, t.title, t.description, t.priority, t.workspace, t.state, a.worker_session_id,
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
                SELECT t.id, t.hive_id, t.title, t.description, t.priority, t.workspace, t.state, a.worker_session_id,
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
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
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
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
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
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
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
        if !task_ids.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(task_ids.len())
    }

    /// Appends one content-free invalidation event and enforces the durable event bound.
    ///
    /// # Errors
    /// Returns an error when the event cannot be committed atomically.
    pub fn record_control_room_event(
        &self,
        kind: ControlRoomEventKind,
    ) -> Result<ControlRoomEvent, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let event = insert_control_room_event(&transaction, kind)?;
        transaction.commit()?;
        Ok(event)
    }

    /// Reads a bounded resumable page of content-free control-room invalidations.
    ///
    /// A cursor from an evicted or replaced database requests a full snapshot reset.
    ///
    /// # Errors
    /// Returns an error when the event page cannot be read or decoded.
    pub fn list_control_room_events(
        &self,
        after: i64,
    ) -> Result<ControlRoomEventPage, TaskStoreError> {
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let (earliest, latest) = connection.query_row(
            "SELECT MIN(sequence), MAX(sequence)
             FROM control_room_events WHERE hive_id = ?1",
            [identity.hive.id.to_string()],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?;
        let reset_required = after != 0
            && match (earliest, latest) {
                (Some(first), Some(last)) => after < first.saturating_sub(1) || after > last,
                _ => true,
            };
        let cursor = if reset_required { 0 } else { after.max(0) };
        let page_limit = i64::try_from(MAX_CONTROL_ROOM_EVENT_PAGE)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT sequence, hive_id, kind, occurred_at
             FROM control_room_events
             WHERE hive_id = ?1 AND sequence > ?2
             ORDER BY sequence ASC LIMIT ?3",
        )?;
        let events = statement
            .query_map(
                params![identity.hive.id.to_string(), cursor, page_limit],
                control_room_event_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = events.last().map_or(cursor, |event| event.sequence);
        Ok(ControlRoomEventPage {
            events,
            next_cursor,
            reset_required,
        })
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

fn migrate_hive_identity(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let operator_id = OperatorId::new();
    let hive_id = HiveId::new();
    transaction.execute_batch(
        "
        CREATE TABLE operators (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE apiaries (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            keeper_operator_id TEXT NOT NULL REFERENCES operators(id),
            shared_work_backend TEXT NOT NULL
                CHECK (shared_work_backend IN ('jira','native')),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE hives (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            operator_id TEXT NOT NULL UNIQUE REFERENCES operators(id),
            apiary_id TEXT REFERENCES apiaries(id),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE local_hive_identity (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            hive_id TEXT NOT NULL UNIQUE REFERENCES hives(id)
        );
        ",
    )?;
    transaction.execute(
        "INSERT INTO operators (id, display_name) VALUES (?1, 'Operator')",
        [operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'My Hive', ?2)",
        params![hive_id.to_string(), operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO local_hive_identity (singleton, hive_id) VALUES (1, ?1)",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        ALTER TABLE tasks ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ALTER TABLE worker_profiles ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ",
    )?;
    transaction.execute(
        "UPDATE tasks SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute(
        "UPDATE worker_profiles SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        CREATE INDEX tasks_by_hive ON tasks(hive_id);
        CREATE INDEX worker_profiles_by_hive ON worker_profiles(hive_id);
        CREATE TRIGGER tasks_require_hive_insert
            BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER tasks_require_hive_update
            BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_insert
            BEFORE INSERT ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_update
            BEFORE UPDATE OF hive_id ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER immutable_apiary_backend
            BEFORE UPDATE OF shared_work_backend ON apiaries
            BEGIN SELECT RAISE(ABORT, 'Apiary shared-work backend is immutable'); END;
        PRAGMA user_version = 4;
        ",
    )
}

fn insert_control_room_event(
    transaction: &rusqlite::Transaction<'_>,
    kind: ControlRoomEventKind,
) -> rusqlite::Result<ControlRoomEvent> {
    transaction.execute(
        "INSERT INTO control_room_events (hive_id, kind)
         SELECT hive_id, ?1 FROM local_hive_identity WHERE singleton = 1",
        [kind.to_string()],
    )?;
    let sequence = transaction.last_insert_rowid();
    transaction.execute(
        "DELETE FROM control_room_events
         WHERE sequence <= (SELECT MAX(sequence) - ?1 FROM control_room_events)",
        [MAX_CONTROL_ROOM_EVENTS],
    )?;
    transaction.query_row(
        "SELECT sequence, hive_id, kind, occurred_at
         FROM control_room_events WHERE sequence = ?1",
        [sequence],
        control_room_event_from_row,
    )
}

fn control_room_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ControlRoomEvent> {
    let kind = ControlRoomEventKind::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(ControlRoomEvent {
        sequence: row.get(0)?,
        hive_id: parse_domain_id::<HiveId>(&row.get::<_, String>(1)?)?,
        kind,
        occurred_at: row.get(3)?,
    })
}

fn migrate_control_room_events(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE control_room_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            hive_id TEXT NOT NULL REFERENCES hives(id),
            kind TEXT NOT NULL CHECK (
                kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed')
            ),
            occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE INDEX control_room_events_by_hive_sequence
            ON control_room_events(hive_id, sequence);
        PRAGMA user_version = 5;
        ",
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
    let hive_id: String = row.get(1)?;
    let priority: String = row.get(4)?;
    let state: String = row.get(6)?;
    let assigned_session_id: Option<String> = row.get(7)?;
    Ok(Task {
        id: TaskId::from_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        hive_id: parse_domain_id::<HiveId>(&hive_id)?,
        title: row.get(2)?,
        description: row.get(3)?,
        priority: TaskPriority::from_str(&priority).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        workspace: row.get(5)?,
        state: TaskState::from_str(&state).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        assigned_session_id: assigned_session_id
            .map(|value| WorkerSessionId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn parse_domain_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
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
    fn reopens_current_schema_without_replacing_hive_identity() {
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
    fn migrates_v3_tasks_and_workers_into_one_durable_hive() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    description TEXT NOT NULL DEFAULT '',
                    priority TEXT NOT NULL DEFAULT 'normal'
                );
                CREATE TABLE task_assignments (
                    id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    worker_session_id TEXT NOT NULL,
                    assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    released_at INTEGER
                );
                CREATE TABLE task_activity (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL,
                    from_state TEXT,
                    to_state TEXT,
                    occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_profiles (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    role TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    autostart INTEGER NOT NULL,
                    position INTEGER NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_sessions (
                    session_id TEXT PRIMARY KEY,
                    worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
                    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    ended_at INTEGER
                );
                INSERT INTO tasks (id, title, workspace, state)
                    VALUES ('018f0000-0000-7000-8000-000000000001', 'Existing task', '/repo', 'ready');
                INSERT INTO worker_profiles
                    (id, name, role, provider, workspace, autostart, position)
                    VALUES ('018f0000-0000-7000-8000-000000000002', 'Existing worker', 'worker', 'claude_code', '/repo', 0, 1);
                PRAGMA user_version = 3;
                ",
            )
            .unwrap();

        let store = TaskStore::from_connection(connection).unwrap();
        let identity = store.local_hive_identity().unwrap();
        assert_eq!(store.list_tasks().unwrap()[0].hive_id, identity.hive.id);
        assert_eq!(
            store.list_worker_profiles().unwrap()[0].hive_id,
            identity.hive.id
        );
        store.verify_integrity().unwrap();
    }

    #[test]
    fn hive_ownership_and_apiary_backend_constraints_fail_closed() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let connection = store.connection().unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO tasks (id, title, workspace, state, description, priority)
                     VALUES (?1, 'Orphan', '/repo', 'draft', '', 'normal')",
                    [TaskId::new().to_string()],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO worker_profiles
                     (id, name, role, provider, workspace, autostart, position)
                     VALUES (?1, 'Orphan', 'worker', 'claude_code', '/repo', 0, 1)",
                    [swarm_domain::WorkerId::new().to_string()],
                )
                .is_err()
        );

        let apiary_id = ApiaryId::new();
        connection
            .execute(
                "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                 VALUES (?1, 'Test Apiary', ?2, 'jira')",
                params![apiary_id.to_string(), identity.operator.id.to_string()],
            )
            .unwrap();
        assert!(
            connection
                .execute(
                    "UPDATE apiaries SET shared_work_backend = 'native' WHERE id = ?1",
                    [apiary_id.to_string()],
                )
                .is_err()
        );
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

    #[test]
    fn task_and_worker_mutations_emit_typed_content_free_events() {
        let store = TaskStore::in_memory().unwrap();
        assert!(store.list_control_room_events(0).unwrap().events.is_empty());

        let task = store.create_task("Secret task text", "/workspace").unwrap();
        let worker = store
            .create_worker(
                "Private worker name",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();

        let page = store.list_control_room_events(0).unwrap();
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                ControlRoomEventKind::TasksChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::SessionsChanged,
            ]
        );
        assert!(
            page.events
                .iter()
                .all(|event| event.hive_id == task.hive_id)
        );
        let serialized = serde_json::to_string(&page).unwrap();
        assert!(!serialized.contains("Secret task text"));
        assert!(!serialized.contains("Private worker name"));
    }

    #[test]
    fn control_room_event_log_is_bounded_and_stale_cursors_reset() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
            .unwrap();
        for _ in 0..=MAX_CONTROL_ROOM_EVENTS {
            store
                .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
                .unwrap();
        }

        let connection = store.connection().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM control_room_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            MAX_CONTROL_ROOM_EVENTS
        );
        drop(connection);

        let stale = store.list_control_room_events(first.sequence).unwrap();
        assert!(stale.reset_required);
        assert_eq!(stale.events.len(), MAX_CONTROL_ROOM_EVENT_PAGE);
        let future = store.list_control_room_events(i64::MAX).unwrap();
        assert!(future.reset_required);
    }

    #[test]
    fn migrates_schema_v4_without_losing_existing_hive_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let (task_id, hive_id) = {
            let store = TaskStore::open(&path).unwrap();
            let task = store.create_task("Existing v4 task", "/workspace").unwrap();
            let hive_id = store.local_hive_identity().unwrap().hive.id;
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TABLE control_room_events; PRAGMA user_version = 4;")
                .unwrap();
            (task.id, hive_id)
        };

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(migrated.get_task(task_id).unwrap().hive_id, hive_id);
        assert!(
            migrated
                .list_control_room_events(0)
                .unwrap()
                .events
                .is_empty()
        );
        migrated.verify_integrity().unwrap();
    }
    #[test]
    fn fresh_store_owns_tasks_and_workers_in_one_durable_hive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let (hive_id, operator_id) = {
            let store = TaskStore::open(&path).unwrap();
            let identity = store.local_hive_identity().unwrap();
            assert_eq!(identity.operator.display_name, "Operator");
            assert_eq!(identity.hive.name, "My Hive");
            assert_eq!(identity.hive.operator_id, identity.operator.id);

            let task = store.create_task("Hive-owned task", "/workspace").unwrap();
            let worker = store
                .create_worker(
                    "Violet",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace",
                    false,
                    1,
                )
                .unwrap();
            assert_eq!(task.hive_id, identity.hive.id);
            assert_eq!(worker.hive_id, identity.hive.id);
            (identity.hive.id, identity.operator.id)
        };

        let reopened = TaskStore::open(path).unwrap();
        let identity = reopened.local_hive_identity().unwrap();
        assert_eq!(identity.hive.id, hive_id);
        assert_eq!(identity.operator.id, operator_id);
        assert_eq!(reopened.list_tasks().unwrap()[0].hive_id, hive_id);
        assert_eq!(reopened.list_worker_profiles().unwrap()[0].hive_id, hive_id);
    }

    #[test]
    fn current_schema_requires_hive_ownership_columns() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        for table in ["tasks", "worker_profiles"] {
            let sql =
                format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'hive_id'");
            assert_eq!(
                connection
                    .query_row(&sql, [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}
