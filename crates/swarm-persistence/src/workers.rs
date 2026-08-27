use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, HiveId, PresenceDeviceId, ProviderConversationId, ProviderKind, WorkerId,
    WorkerProfile, WorkerRole, WorkerSessionId,
};
use uuid::Uuid;

use super::{
    MAX_WORKSPACE_BYTES, TaskStore, TaskStoreError, WORKER_REVIVAL_INTENT_SCHEMA_VERSION,
    insert_control_room_event,
};

/// Records which workers a worker-engine replacement unloaded, so they can be
/// brought back by whoever is running next rather than only by the request that
/// stopped them.
///
/// A forward step, and guarded on `worker_profiles`, because a database old
/// enough to predate the roster passes through here on its way up.
///
/// # Errors
/// Returns an error when the step cannot be applied.
pub(super) fn migrate_worker_revival_intents(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let workers_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    if workers_exist {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS worker_revival_intents (
                worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
                recorded_at INTEGER NOT NULL
            );",
        )?;
    }
    transaction.pragma_update(None, "user_version", WORKER_REVIVAL_INTENT_SCHEMA_VERSION)
}

/// One live worker session, and what it is running.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveWorkerSession {
    pub worker_id: WorkerId,
    pub provider: ProviderKind,
    pub started_at: i64,
}

const MAX_WORKER_NAME_BYTES: usize = 80;
const MAX_WORKER_DESCRIPTION_BYTES: usize = 2_000;

/// How many size requests are kept per terminal. Enough to see a fight and its
/// shape; not enough for a fight to grow the database.
const GEOMETRY_EVENTS_KEPT_PER_SESSION: i64 = 200;

/// What the ledger says about one terminal over a window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryContention {
    pub requests: usize,
    pub devices: usize,
    /// Granted claims that moved the size to a device that did not hold it.
    /// One is a handover. Repeated ones are a fight.
    pub handovers: usize,
    pub refused: usize,
    pub distinct_sizes: usize,
}

/// How a profile starts life: awake or not, where in the roster, and whether it
/// is temporary.
///
/// A struct rather than three more parameters because `insert_profile` already
/// carried as many as it should. Grouping them also stops the two booleans
/// being passed in the wrong order, which a pair of bare `bool`s invites.
#[derive(Clone, Copy)]
struct ProfileStartup {
    autostart: bool,
    position: i64,
    /// Temporary workers are created only by `create_temporary_worker`.
    ephemeral: bool,
}

impl ProfileStartup {
    /// An ordinary worker, which is every caller but one.
    const fn permanent(autostart: bool, position: i64) -> Self {
        Self {
            autostart,
            position,
            ephemeral: false,
        }
    }
}

impl TaskStore {
    /// Returns the singleton Queen profile, creating it on first start.
    ///
    /// # Errors
    /// Returns an error when the workspace is invalid or persistence is unavailable.
    pub fn ensure_queen(&self, workspace: &str) -> Result<WorkerProfile, TaskStoreError> {
        if let Some(profile) = self.profile_by_role(WorkerRole::Queen)? {
            return Ok(profile);
        }
        self.insert_profile(
            "Queen",
            "",
            WorkerRole::Queen,
            ProviderKind::ClaudeCode,
            workspace,
            ProfileStartup::permanent(true, 0),
        )
    }

    /// Promotes the exact existing `Project Root` worker at the configured
    /// projects root into managed Scout without replacing its durable identity.
    ///
    /// # Errors
    /// Returns an error for invalid workspace, conflicting name, corrupt identity,
    /// or unavailable persistence.
    pub fn promote_project_root_to_scout(
        &self,
        workspace: &str,
    ) -> Result<Option<WorkerProfile>, TaskStoreError> {
        let workspace = normalize_workspace(workspace)?;
        let workspace = workspace.as_str();
        if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
            return Err(TaskStoreError::InvalidWorkspace);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(id) = transaction
            .query_row(
                "SELECT id FROM worker_profiles
                 WHERE system_role = 'scout' AND archived_at IS NULL",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            transaction.commit()?;
            drop(connection);
            return self
                .get_worker_profile(parse_worker_identity(&id)?)
                .map(Some);
        }
        let Some(id) = transaction
            .query_row(
                "SELECT id FROM worker_profiles
                 WHERE role = 'worker' AND archived_at IS NULL
                   AND workspace = ?1 AND name = 'Project Root' COLLATE NOCASE",
                [workspace],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        else {
            transaction.commit()?;
            return Ok(None);
        };
        let duplicate_name = transaction
            .query_row(
                "SELECT 1 FROM worker_profiles
                 WHERE id != ?1 AND name = 'Scout' COLLATE NOCASE AND archived_at IS NULL",
                [&id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate_name {
            return Err(TaskStoreError::DuplicateWorkerName);
        }
        transaction.execute(
            "UPDATE worker_profiles
             SET name = 'Scout', system_role = 'scout', autostart = 0, position = 0,
                 updated_at = unixepoch()
             WHERE id = ?1",
            [&id],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_worker_profile(parse_worker_identity(&id)?)
            .map(Some)
    }

    /// Returns the stable managed Scout identity when configured.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or contains invalid identity data.
    pub fn scout_worker_id(&self) -> Result<Option<WorkerId>, TaskStoreError> {
        self.connection()?
            .query_row(
                "SELECT id FROM worker_profiles
                 WHERE system_role = 'scout' AND archived_at IS NULL",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|id| parse_worker_identity(&id))
            .transpose()
    }

    /// Creates one durable worker profile without starting a process.
    ///
    /// # Errors
    /// Returns an error for invalid or duplicate input or unavailable persistence.
    pub fn create_worker(
        &self,
        name: &str,
        provider: ProviderKind,
        workspace: &str,
        autostart: bool,
        position: i64,
    ) -> Result<WorkerProfile, TaskStoreError> {
        self.insert_profile(
            name,
            "",
            WorkerRole::Worker,
            provider,
            workspace,
            ProfileStartup::permanent(autostart, position),
        )
    }

    /// Creates one durable worker with operator-visible routing context without starting a process.
    ///
    /// # Errors
    /// Returns an error for invalid or duplicate input or unavailable persistence.
    pub fn create_worker_with_description(
        &self,
        name: &str,
        description: &str,
        provider: ProviderKind,
        workspace: &str,
        autostart: bool,
        position: i64,
    ) -> Result<WorkerProfile, TaskStoreError> {
        self.insert_profile(
            name,
            description,
            WorkerRole::Worker,
            provider,
            workspace,
            ProfileStartup::permanent(autostart, position),
        )
    }

    /// Updates operator-owned worker preferences without changing repository or conversation identity.
    ///
    /// # Errors
    /// Rejects the managed Queen, empty updates, invalid or duplicate names, and unknown workers.
    pub fn update_worker_profile(
        &self,
        worker_id: WorkerId,
        name: Option<&str>,
        description: Option<&str>,
        provider: Option<ProviderKind>,
        autostart: Option<bool>,
        workspace: Option<&str>,
    ) -> Result<WorkerProfile, TaskStoreError> {
        if name.is_none()
            && description.is_none()
            && provider.is_none()
            && autostart.is_none()
            && workspace.is_none()
        {
            return Err(TaskStoreError::EmptyWorkerUpdate);
        }
        let name = name.map(str::trim);
        if let Some(name) = name {
            validate_worker_name(name)?;
        }
        let description = description.map(str::trim);
        if let Some(description) = description {
            validate_worker_description(description)?;
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (current_name, role, system_role, current_provider, running) = transaction
            .query_row(
                "SELECT name, role, system_role, provider,
                        EXISTS(SELECT 1 FROM worker_sessions
                               WHERE worker_id = worker_profiles.id AND ended_at IS NULL)
                 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, bool>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)?;
        if role == WorkerRole::Queen.to_string() {
            return Err(TaskStoreError::QueenProfileImmutable);
        }
        if system_role.as_deref() == Some("scout") && name.is_some_and(|name| name != current_name)
        {
            return Err(TaskStoreError::ScoutIdentityImmutable);
        }
        if let Some(name) = name {
            let duplicate = transaction
                .query_row(
                    "SELECT 1 FROM worker_profiles WHERE id != ?1 AND name = ?2 COLLATE NOCASE",
                    params![worker_id.to_string(), name],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if duplicate {
                return Err(TaskStoreError::DuplicateWorkerName);
            }
            transaction.execute(
                "UPDATE worker_profiles SET name = ?2 WHERE id = ?1",
                params![worker_id.to_string(), name],
            )?;
        }
        if let Some(description) = description {
            transaction.execute(
                "UPDATE worker_profiles SET description = ?2 WHERE id = ?1",
                params![worker_id.to_string(), description],
            )?;
        }
        if provider == Some(ProviderKind::Unsupported) {
            // Belt and braces. from_str is strict so no API caller can produce
            // this, but writing it would overwrite a real stored provider with
            // "unsupported" and destroy the only record of what the worker was.
            // Losing a row is recoverable; silently rewriting it is not.
            return Err(TaskStoreError::IntegrityFailure(
                "refusing to store an unsupported provider".into(),
            ));
        }
        if let Some(provider) = provider
            && provider.to_string() != current_provider
        {
            if running {
                return Err(TaskStoreError::WorkerMustBeSleeping);
            }
            transaction.execute(
                "UPDATE worker_profiles SET provider = ?2 WHERE id = ?1",
                params![worker_id.to_string(), provider.to_string()],
            )?;
        }
        if let Some(autostart) = autostart {
            transaction.execute(
                "UPDATE worker_profiles SET autostart = ?2 WHERE id = ?1",
                params![worker_id.to_string(), autostart],
            )?;
        }
        move_worker_repository(&transaction, worker_id, workspace, running)?;
        transaction.execute(
            "UPDATE worker_profiles SET updated_at = unixepoch() WHERE id = ?1",
            [worker_id.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_worker_profile(worker_id)
    }

    /// Creates a TEMPORARY worker beside another, on any provider.
    ///
    /// A real worker row rather than an anonymous session, because it holds the
    /// full tool surface and anything that writes to the durable record must
    /// stay attributable. The flag is the only difference: it is otherwise an
    /// ordinary worker, which is what makes adoption a flag change rather than a
    /// re-creation.
    ///
    /// # Errors
    /// Returns an error for an invalid name or workspace, or a duplicate name.
    pub fn create_temporary_worker(
        &self,
        name: &str,
        provider: ProviderKind,
        workspace: &str,
        position: i64,
    ) -> Result<WorkerProfile, TaskStoreError> {
        self.insert_profile(
            name,
            "",
            WorkerRole::Worker,
            provider,
            workspace,
            ProfileStartup {
                autostart: false,
                position,
                ephemeral: true,
            },
        )
    }

    /// Adopts a temporary worker into the Hive under a permanent name.
    ///
    /// A FLAG CHANGE, deliberately, not a re-creation. The worker keeps its id,
    /// so its session history, its conversation and every board write it already
    /// made continue to point at the same worker. Re-creating it would leave the
    /// record naming a worker that no longer exists.
    ///
    /// # Errors
    /// Returns an error when the worker is unknown, is not temporary, or the
    /// name is invalid or taken.
    pub fn adopt_worker(
        &self,
        worker_id: WorkerId,
        name: &str,
    ) -> Result<WorkerProfile, TaskStoreError> {
        let name = name.trim();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let ephemeral: bool = transaction
            .query_row(
                "SELECT ephemeral FROM worker_profiles
                 WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |row| row.get::<_, i64>(0).map(|value| value != 0),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)?;
        if !ephemeral {
            // Not an error worth inventing a variant for, but not a silent
            // success either: adopting something already permanent means the
            // caller believes something false about it.
            return Err(TaskStoreError::WorkerNotFound);
        }
        let taken = transaction
            .query_row(
                "SELECT 1 FROM worker_profiles
                 WHERE name = ?1 COLLATE NOCASE AND id != ?2 AND archived_at IS NULL",
                params![name, worker_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if taken {
            return Err(TaskStoreError::DuplicateWorkerName);
        }
        transaction.execute(
            "UPDATE worker_profiles
             SET name = ?2, ephemeral = 0, updated_at = unixepoch()
             WHERE id = ?1",
            params![worker_id.to_string(), name],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_worker_profile(worker_id)
    }

    /// Removes a sleeping worker from the active roster while retaining historical identity.
    ///
    /// # Errors
    /// Rejects Queen, running workers, workers that still own open tasks, and unknown workers.
    pub fn archive_worker_profile(&self, worker_id: WorkerId) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (name, role, system_role, running, owns_open_tasks) = transaction
            .query_row(
                "SELECT name, role, system_role,
                        EXISTS(SELECT 1 FROM worker_sessions
                               WHERE worker_id = worker_profiles.id AND ended_at IS NULL),
                        EXISTS(SELECT 1 FROM tasks
                               WHERE assigned_worker_id = worker_profiles.id
                                 AND state != 'completed' AND removed_at IS NULL)
                 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, bool>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)?;
        if role == WorkerRole::Queen.to_string() {
            return Err(TaskStoreError::QueenProfileImmutable);
        }
        if system_role.as_deref() == Some("scout") {
            return Err(TaskStoreError::ScoutIdentityImmutable);
        }
        if running {
            return Err(TaskStoreError::WorkerMustBeSleeping);
        }
        if owns_open_tasks {
            return Err(TaskStoreError::WorkerOwnsOpenTasks);
        }
        let archived_name = format!("{name} (removed {})", &worker_id.to_string()[..8]);
        transaction.execute(
            "UPDATE worker_profiles
             SET name = ?2, autostart = 0, archived_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?1",
            params![worker_id.to_string(), archived_name],
        )?;
        transaction.execute(
            "DELETE FROM worker_agent_credentials WHERE worker_id = ?1",
            [worker_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM worker_engagements WHERE worker_id = ?1",
            [worker_id.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces the stable operator order of every non-Queen worker.
    ///
    /// # Errors
    /// Rejects incomplete, duplicate, or foreign worker IDs.
    pub fn reorder_workers(
        &self,
        worker_ids: &[WorkerId],
    ) -> Result<Vec<WorkerProfile>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let expected = {
            let mut statement = transaction.prepare(
                "SELECT id FROM worker_profiles
                 WHERE role != 'queen' AND system_role IS NULL AND archived_at IS NULL
                 ORDER BY position, created_at, id",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let supplied = worker_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let supplied_set = supplied.iter().collect::<HashSet<_>>();
        let expected_set = expected.iter().collect::<HashSet<_>>();
        if supplied.len() != expected.len()
            || supplied_set.len() != supplied.len()
            || supplied_set != expected_set
        {
            return Err(TaskStoreError::InvalidWorkerOrder);
        }
        for (position, worker_id) in supplied.iter().enumerate() {
            let position = i64::try_from(position)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
            transaction.execute(
                "UPDATE worker_profiles SET position = ?2, updated_at = unixepoch() WHERE id = ?1",
                params![worker_id, position],
            )?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.list_worker_profiles()
    }

    /// Lists the roster in stable operator order with its current session binding.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or contains invalid data.
    pub fn list_worker_profiles(&self) -> Result<Vec<WorkerProfile>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT p.id, p.hive_id, p.name, p.role, p.provider, p.workspace, p.autostart,
                   p.position, s.session_id, p.provider_conversation_id,
                   (EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id)
                    OR p.provider_conversation_resume = 1),
                   e.expires_at,
                   p.created_at, p.updated_at, p.description, p.ephemeral
            FROM worker_profiles p
            LEFT JOIN worker_sessions s
              ON s.worker_id = p.id AND s.ended_at IS NULL
            LEFT JOIN worker_engagements e
              ON e.worker_id = p.id AND e.session_id = s.session_id
             AND e.expires_at > unixepoch()
            WHERE p.archived_at IS NULL
            ORDER BY CASE
                         WHEN p.role = 'queen' THEN 0
                         WHEN p.system_role = 'scout' THEN 1
                         ELSE 2
                     END,
                     p.position, p.created_at, p.id
            ",
        )?;
        statement
            .query_map([], profile_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Loads one durable worker profile.
    ///
    /// # Errors
    /// Why this worker's most recent session ended, when anyone recorded it.
    ///
    /// Read separately rather than widened into `WorkerProfile`, which is
    /// carried through a great deal of code that has no use for it. A resting
    /// worker is the only place this matters.
    ///
    /// # Errors
    /// Returns an error when the session history cannot be read.
    pub fn last_session_end_reason(
        &self,
        worker_id: WorkerId,
    ) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT ended_reason FROM worker_sessions
                 WHERE worker_id = ?1 AND ended_at IS NOT NULL AND ended_reason IS NOT NULL
                 ORDER BY ended_at DESC, session_id DESC LIMIT 1",
                [worker_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// # Errors
    /// Returns `WorkerNotFound` when the identity is unknown.
    pub fn get_worker_profile(&self, id: WorkerId) -> Result<WorkerProfile, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT p.id, p.hive_id, p.name, p.role, p.provider, p.workspace, p.autostart,
                       p.position, s.session_id, p.provider_conversation_id,
                       (EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id)
                        OR p.provider_conversation_resume = 1),
                       e.expires_at,
                       p.created_at, p.updated_at, p.description, p.ephemeral
                FROM worker_profiles p
                LEFT JOIN worker_sessions s
                  ON s.worker_id = p.id AND s.ended_at IS NULL
                LEFT JOIN worker_engagements e
                  ON e.worker_id = p.id AND e.session_id = s.session_id
                 AND e.expires_at > unixepoch()
                WHERE p.id = ?1 AND p.archived_at IS NULL
                ",
                [id.to_string()],
                profile_from_row,
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)
    }

    /// Returns the configured provider for one currently active process session.
    ///
    /// # Errors
    /// Returns `WorkerSessionNotActive` when the session is no longer bound or
    /// a data-integrity error when its provider value is invalid.
    pub fn provider_for_active_session(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<ProviderKind, TaskStoreError> {
        let connection = self.connection()?;
        let provider = connection
            .query_row(
                "SELECT profile.provider
                 FROM worker_sessions session
                 JOIN worker_profiles profile ON profile.id = session.worker_id
                 WHERE session.session_id = ?1 AND session.ended_at IS NULL
                   AND profile.archived_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        // from_stored, not from_str: a provider this build does not know is a
        // rollback, not corruption, and refusing to read it takes down callers
        // that only wanted to know which session was active.
        Ok(ProviderKind::from_stored(&provider))
    }

    /// Maps each engaged worker to the device currently holding input and
    /// terminal geometry, with that device's class.
    ///
    /// Geometry follows the engaged device, so a desktop rendering at phone
    /// width is correct behaviour that looks like a fault. Naming the owner is
    /// what makes it explainable.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn engaged_devices_by_worker(
        &self,
        now: i64,
    ) -> Result<HashMap<WorkerId, (String, String)>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT engagement.worker_id, engagement.owner_device_id, device.device_class
             FROM worker_engagements engagement
             JOIN operator_presence_devices device ON device.id = engagement.owner_device_id
             WHERE engagement.expires_at > ?1 AND engagement.owner_device_id IS NOT NULL",
        )?;
        let rows = statement.query_map([now], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut engaged = HashMap::new();
        for row in rows {
            let (worker_id, device_id, device_class) = row?;
            let worker_id = WorkerId::from_str(&worker_id).map_err(|_| {
                TaskStoreError::IntegrityFailure("invalid engaged worker identity".into())
            })?;
            engaged.insert(worker_id, (device_id, device_class));
        }
        Ok(engaged)
    }

    /// Atomically binds a new immutable process session to a stable worker profile.
    ///
    /// # Errors
    /// Returns an error when the worker is unknown or already has an active session.
    pub fn bind_worker_session(
        &self,
        worker_id: WorkerId,
        session_id: WorkerSessionId,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(TaskStoreError::WorkerNotFound);
        }
        let active = transaction
            .query_row(
                "SELECT 1 FROM worker_sessions WHERE worker_id = ?1 AND ended_at IS NULL",
                [worker_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if active {
            return Err(TaskStoreError::WorkerAlreadyRunning);
        }
        transaction.execute(
            "INSERT INTO worker_sessions (session_id, worker_id) VALUES (?1, ?2)",
            params![session_id.to_string(), worker_id.to_string()],
        )?;
        let owned_tasks = {
            let mut statement = transaction.prepare(
                "SELECT task.id, task.state
                 FROM tasks task
                 WHERE task.assigned_worker_id = ?1 AND task.state != 'completed'
                   AND NOT EXISTS (
                       SELECT 1 FROM task_assignments assignment
                       WHERE assignment.task_id = task.id AND assignment.released_at IS NULL
                   )
                 ORDER BY task.position, task.id",
            )?;
            statement
                .query_map([worker_id.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for (task_id, task_state) in &owned_tasks {
            let assignment_id = Uuid::now_v7().to_string();
            transaction.execute(
                "INSERT INTO task_assignments (id, task_id, worker_session_id)
                 VALUES (?1, ?2, ?3)",
                params![assignment_id, task_id, session_id.to_string()],
            )?;
            let previously_briefed: bool = transaction.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM task_dispatches
                     WHERE task_id = ?1 AND worker_id = ?2
                       AND state IN ('dispatching','delivered','uncertain')
                 )",
                params![task_id, worker_id.to_string()],
                |row| row.get(0),
            )?;
            if !previously_briefed && task_state == "ready" {
                let queued: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
                    [],
                    |row| row.get(0),
                )?;
                if queued >= 256 {
                    return Err(TaskStoreError::TaskDispatchQueueFull);
                }
                transaction.execute(
                    "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
                     VALUES (?1, ?2, ?3, 'queued')",
                    params![assignment_id, task_id, worker_id.to_string()],
                )?;
            }
        }
        transaction.execute(
            "UPDATE worker_profiles SET updated_at = unixepoch() WHERE id = ?1",
            [worker_id.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        insert_control_room_event(&transaction, ControlRoomEventKind::SessionsChanged)?;
        if !owned_tasks.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Assigns a stable provider conversation to a profile that has never launched.
    ///
    /// # Errors
    /// Returns an error when the worker is unknown, already has history, or persistence fails.
    pub fn assign_provider_conversation(
        &self,
        worker_id: WorkerId,
    ) -> Result<ProviderConversationId, TaskStoreError> {
        let session_id = ProviderConversationId::new();
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE worker_profiles SET provider_conversation_id = ?1, updated_at = unixepoch()
             WHERE id = ?2 AND provider_conversation_id IS NULL AND archived_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM worker_sessions WHERE worker_id = worker_profiles.id
               )",
            params![session_id.to_string(), worker_id.to_string()],
        )?;
        if updated == 1 {
            return Ok(session_id);
        }
        let exists = connection
            .query_row(
                "SELECT 1 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            return Err(TaskStoreError::ProviderConversationUnavailable);
        }
        Err(TaskStoreError::WorkerNotFound)
    }

    /// Creates or renews the bounded operator engagement lease for an active session.
    /// Returns whether durable state changed enough to require control-room invalidation.
    ///
    /// # Errors
    /// Returns an error when the session is not actively bound or persistence fails.
    pub fn renew_worker_engagement(
        &self,
        session_id: WorkerSessionId,
        owner_device_id: Option<PresenceDeviceId>,
        now: i64,
        lease_seconds: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let worker_id = transaction
            .query_row(
                "SELECT worker_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        let current = transaction
            .query_row(
                "SELECT expires_at, owner_device_id FROM worker_engagements WHERE worker_id = ?1",
                [&worker_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        let renewal_threshold = now.saturating_add(lease_seconds / 2);
        let owner_device_id = owner_device_id.map(|device_id| device_id.to_string());
        // An operator is in one place. Engaging a worker therefore ends this
        // device's engagement everywhere else, rather than leaving the worker
        // it just left holding a claim until the lease runs out. Without this
        // the roster shows several workers "with you" at once, and each stale
        // claim also holds back the coordination those workers are owed.
        //
        // Only a device that identifies itself can be swept: engagements with
        // no owner cannot be told apart, so they are left alone.
        let released_elsewhere = match &owner_device_id {
            Some(device_id) => transaction.execute(
                "DELETE FROM worker_engagements
                 WHERE owner_device_id = ?1 AND worker_id <> ?2",
                params![device_id, worker_id],
            )?,
            None => 0,
        };
        if released_elsewhere == 0
            && current.is_some_and(|(expiry, current_owner)| {
                expiry >= renewal_threshold && current_owner == owner_device_id
            })
        {
            return Ok(false);
        }
        let expires_at = now.saturating_add(lease_seconds);
        transaction.execute(
            "INSERT INTO worker_engagements
             (worker_id, session_id, engaged_at, renewed_at, expires_at, owner_device_id)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5)
             ON CONFLICT(worker_id) DO UPDATE SET
                 session_id = excluded.session_id,
                 renewed_at = excluded.renewed_at,
                 expires_at = excluded.expires_at,
                 owner_device_id = excluded.owner_device_id",
            params![
                worker_id,
                session_id.to_string(),
                now,
                expires_at,
                owner_device_id
            ],
        )?;
        transaction.execute(
            "UPDATE worker_sessions
             SET geometry_owner_device_id = ?2
             WHERE session_id = ?1 AND ended_at IS NULL",
            params![session_id.to_string(), owner_device_id],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(true)
    }

    /// Whether a person currently holds any worker terminal.
    ///
    /// Presence answers "is the operator at the Hive", which can be Away while
    /// somebody is still typing in a terminal on a device that is no longer
    /// heartbeating as present. Anything that would interrupt a live session —
    /// restarting the App and API, for one — needs this stronger question as
    /// well as that one.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn operator_holds_any_terminal(&self, now: i64) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_engagements WHERE expires_at > ?1)",
            params![now],
            |row| row.get(0),
        )?)
    }

    /// Releases an operator engagement only when the requesting device still owns it.
    /// Returns whether an engagement was released.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn release_worker_engagement(
        &self,
        session_id: WorkerSessionId,
        owner_device_id: PresenceDeviceId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let released = transaction.execute(
            "DELETE FROM worker_engagements
             WHERE session_id = ?1 AND owner_device_id = ?2",
            params![session_id.to_string(), owner_device_id.to_string()],
        )? == 1;
        if released {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(released)
    }

    /// Returns whether this exact device most recently supplied operator input to
    /// a live worker session, regardless of whether its attention lease expired.
    ///
    /// Terminal geometry is shared by every viewer of one server-owned PTY.
    /// Resize authority therefore follows the device that most recently sent
    /// operator input; passive desktop and mobile viewers must not resize the
    /// same provider process back and forth.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn device_owns_worker_geometry(
        &self,
        session_id: WorkerSessionId,
        owner_device_id: Option<PresenceDeviceId>,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let owner_device_id = owner_device_id.map(|device_id| device_id.to_string());
        connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM worker_sessions
                     WHERE session_id = ?1 AND ended_at IS NULL
                       AND geometry_owner_device_id IS ?2
                 )",
                params![session_id.to_string(), owner_device_id],
                |row| row.get(0),
            )
            .map_err(TaskStoreError::from)
    }

    /// Records one request to set a terminal's size, granted or not.
    ///
    /// # Errors
    /// Returns an error when the ledger cannot be written.
    pub fn record_geometry_request(
        &self,
        session_id: WorkerSessionId,
        device_id: Option<PresenceDeviceId>,
        size: (u16, u16),
        claimed: bool,
        granted: bool,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        let owner_before: Option<String> = connection
            .query_row(
                "SELECT geometry_owner_device_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        connection.execute(
            "INSERT INTO terminal_geometry_events
             (session_id, device_id, rows, columns, claimed, granted, owner_before, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id.to_string(),
                device_id.map(|device| device.to_string()),
                i64::from(size.0),
                i64::from(size.1),
                i64::from(claimed),
                i64::from(granted),
                owner_before,
                now
            ],
        )?;
        // Bounded on write. A quiet terminal writes one of these an hour; a
        // fight writes several a second, and it is the fight that must not grow
        // the database without limit.
        connection.execute(
            "DELETE FROM terminal_geometry_events
             WHERE session_id = ?1 AND id NOT IN (
                 SELECT id FROM terminal_geometry_events
                 WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2
             )",
            params![session_id.to_string(), GEOMETRY_EVENTS_KEPT_PER_SESSION],
        )?;
        Ok(())
    }

    /// How much the terminal's size has been argued over recently.
    ///
    /// # Errors
    /// Returns an error when the ledger cannot be read.
    pub fn geometry_contention(
        &self,
        session_id: WorkerSessionId,
        since: i64,
    ) -> Result<GeometryContention, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT device_id, rows, columns, claimed, granted, owner_before, at
             FROM terminal_geometry_events
             WHERE session_id = ?1 AND at >= ?2
             ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![session_id.to_string(), since], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, i64>(4)? != 0,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut devices = std::collections::BTreeSet::new();
        let mut handovers = 0;
        let mut refused = 0;
        let mut sizes = std::collections::BTreeSet::new();
        for (device, rows_count, columns, _claimed, granted, _owner_before, _) in &rows {
            if let Some(device) = device {
                devices.insert(device.clone());
            }
            if !granted {
                refused += 1;
            }
            sizes.insert((*rows_count, *columns));
        }

        // A handover is the size passing to a device that was not the last one
        // granted it. Read from the sequence rather than from the owner
        // recorded beside each row: that owner is read after the claim has
        // already been applied, so it never differs from the claimant and
        // counted nothing.
        //
        // Repeated grants to the same device are ordinary resizing — dragging a
        // window edge must not read as a fight.
        let mut holder: Option<&String> = None;
        for (device, .., granted, _, _) in &rows {
            if !*granted {
                continue;
            }
            let Some(device) = device.as_ref() else {
                continue;
            };
            if holder.is_some_and(|held| held != device) {
                handovers += 1;
            }
            holder = Some(device);
        }
        Ok(GeometryContention {
            requests: rows.len(),
            devices: devices.len(),
            handovers,
            refused,
            distinct_sizes: sizes.len(),
        })
    }

    /// Claims geometry authority for a live session when no device owns it yet,
    /// or confirms that the requesting device already owns it.
    ///
    /// A freshly started always-active worker has no operator input history. Its
    /// first identified viewer must therefore be allowed to fit the PTY to the
    /// available viewport. Once claimed, passive viewers cannot take geometry
    /// authority away; a later operator input still transfers it explicitly.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn claim_unowned_worker_geometry(
        &self,
        session_id: WorkerSessionId,
        owner_device_id: PresenceDeviceId,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let owner_device_id = owner_device_id.to_string();
        let claimed = connection.execute(
            "UPDATE worker_sessions
             SET geometry_owner_device_id = ?2
             WHERE session_id = ?1 AND ended_at IS NULL
               AND (geometry_owner_device_id IS NULL OR geometry_owner_device_id = ?2)",
            params![session_id.to_string(), owner_device_id],
        )? == 1;
        Ok(claimed)
    }

    /// Makes an explicitly selected foreground viewer the geometry owner.
    ///
    /// Unlike passive resize observation, opening or refreshing the selected
    /// terminal is an operator action. It must be able to replace geometry
    /// left by a previous desktop or mobile viewport. Background viewers never
    /// call this operation, and subsequent passive resizes remain owner-bound.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn claim_worker_geometry(
        &self,
        session_id: WorkerSessionId,
        owner_device_id: PresenceDeviceId,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        // The device holding the worker decides its size, and nothing else does.
        //
        // Two viewers of one worker both believed they were the foreground —
        // one browser's idea of focus says nothing about another machine — so
        // each claimed geometry, restored at the other's size, re-fitted to its
        // own and claimed again. The operator watched a desktop and a phone
        // resize a terminal at each other indefinitely.
        //
        // Engagement already names one device and the operator can move it by
        // taking the worker, so it is the arbiter. A claim from anywhere else is
        // refused rather than queued: the other viewer then accepts the
        // canonical size instead of arguing with it.
        let claimed = connection.execute(
            "UPDATE worker_sessions
             SET geometry_owner_device_id = ?2
             WHERE session_id = ?1 AND ended_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements engagement
                   WHERE engagement.worker_id = worker_sessions.worker_id
                     AND engagement.expires_at > unixepoch()
                     AND engagement.owner_device_id IS NOT NULL
                     AND engagement.owner_device_id <> ?2
               )",
            params![session_id.to_string(), owner_device_id.to_string()],
        )? == 1;
        Ok(claimed)
    }

    /// Returns whether coordination may inject into a worker at this instant.
    ///
    /// # Errors
    /// Returns an error when the worker is unknown or persistence fails.
    pub fn worker_accepts_injection(
        &self,
        worker_id: WorkerId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [worker_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(TaskStoreError::WorkerNotFound);
        }
        let engaged = connection
            .query_row(
                "SELECT 1 FROM worker_engagements
                 WHERE worker_id = ?1 AND expires_at > ?2",
                params![worker_id.to_string(), now],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let takeover = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM worker_profiles p
                 JOIN local_federation_steward_takeover_leases lease
                   ON lease.target_hive_id = p.hive_id
                 WHERE p.id = ?1 AND p.role = 'queen'
                   AND lease.state IN ('requested','active') AND lease.expires_at > ?2
             )",
            params![worker_id.to_string(), now],
            |row| row.get::<_, bool>(0),
        )?;
        Ok(!engaged && !takeover)
    }

    /// Returns the currently bound Queen session, if Queen is running.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence contains an invalid session identity.
    pub fn active_queen_session_id(&self) -> Result<Option<WorkerSessionId>, TaskStoreError> {
        self.connection()?
            .query_row(
                "SELECT s.session_id FROM worker_profiles p
                 JOIN worker_sessions s ON s.worker_id = p.id AND s.ended_at IS NULL
                 WHERE p.role = 'queen' AND p.archived_at IS NULL LIMIT 1",
                [],
                |row| {
                    WorkerSessionId::from_str(&row.get::<_, String>(0)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Releases a session binding after its process exits or is stopped.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn release_worker_session(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<bool, TaskStoreError> {
        self.release_worker_session_because(session_id, None)
    }

    /// Ends a session and records WHY, and who ended it.
    ///
    /// A worker that is simply not running is the failure this fleet keeps
    /// rediscovering: the state is visible and the reason is not. Standing one
    /// down deliberately must not become another silent state — a resting
    /// worker should be distinguishable from a crashed one.
    ///
    /// `None` is honest rather than lazy: most sessions end without anyone
    /// recording why, and an absent reason means "not recorded" instead of a
    /// backfilled guess.
    ///
    /// # Errors
    /// Returns an error when the session cannot be released.
    pub fn release_worker_session_because(
        &self,
        session_id: WorkerSessionId,
        ended: Option<(&str, &str)>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let released = transaction.execute(
            "UPDATE worker_sessions SET ended_at = unixepoch(),
                 ended_reason = ?2, ended_by = ?3
             WHERE session_id = ?1 AND ended_at IS NULL",
            params![
                session_id.to_string(),
                ended.map(|(reason, _)| reason),
                ended.map(|(_, actor)| actor)
            ],
        )? == 1;
        if released {
            transaction.execute(
                "DELETE FROM worker_engagements WHERE session_id = ?1",
                [session_id.to_string()],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
            insert_control_room_event(&transaction, ControlRoomEventKind::SessionsChanged)?;
        }
        transaction.commit()?;
        Ok(released)
    }

    /// The live worker sessions, with the provider each runs and when it began.
    ///
    /// A provider process executes the release it started with, so the start
    /// time is what decides whether a worker is still running something that
    /// has since been replaced on disk.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or holds an invalid ID.
    pub fn active_worker_sessions(&self) -> Result<Vec<ActiveWorkerSession>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT p.id, p.provider, s.started_at
             FROM worker_sessions s
             JOIN worker_profiles p ON p.id = s.worker_id
             WHERE s.ended_at IS NULL
             ORDER BY s.started_at, p.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, provider, started_at)| {
                Ok(ActiveWorkerSession {
                    worker_id: WorkerId::from_str(&id)
                        .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
                    provider: ProviderKind::from_stored(&provider),
                    started_at,
                })
            })
            .collect()
    }

    /// Remembers the workers a worker-engine replacement is about to unload.
    ///
    /// Recorded before anything is stopped and kept in the database rather than
    /// in the request that stops them, because the operator is promised those
    /// workers back and the request can die — on a timeout, behind a proxy, or
    /// with the process itself — after the workers are already gone.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn record_worker_revival_intents(
        &self,
        worker_ids: &[WorkerId],
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for worker_id in worker_ids {
            transaction.execute(
                "INSERT INTO worker_revival_intents (worker_id, recorded_at)
                 VALUES (?1, ?2)
                 ON CONFLICT(worker_id) DO UPDATE SET recorded_at = excluded.recorded_at",
                params![worker_id.to_string(), now],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// The workers still owed a revival, discarding any older than `max_age_seconds`.
    ///
    /// Ageing them out matters: an intent that outlives the maintenance it
    /// belongs to would wake workers the operator has since chosen to leave
    /// asleep.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn worker_revival_intents(
        &self,
        now: i64,
        max_age_seconds: i64,
    ) -> Result<Vec<WorkerId>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM worker_revival_intents WHERE recorded_at + ?2 <= ?1",
            params![now, max_age_seconds],
        )?;
        let intents = transaction
            .prepare(
                "SELECT worker_id FROM worker_revival_intents
                 ORDER BY recorded_at, worker_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(intents
            .into_iter()
            .filter_map(|id| WorkerId::from_str(&id).ok())
            .collect())
    }

    /// Forgets one revival intent, whether it was honoured or refused.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn clear_worker_revival_intent(&self, worker_id: WorkerId) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "DELETE FROM worker_revival_intents WHERE worker_id = ?1",
            [worker_id.to_string()],
        )?;
        Ok(())
    }

    /// Releases database bindings that are absent from the terminal host snapshot.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or contains an invalid ID.
    pub fn release_missing_worker_sessions(
        &self,
        live_sessions: &HashSet<WorkerSessionId>,
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let stale = {
            let mut statement = transaction
                .prepare("SELECT session_id FROM worker_sessions WHERE ended_at IS NULL")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|value| {
                    WorkerSessionId::from_str(&value)
                        .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|session_id| !live_sessions.contains(session_id))
                .collect::<Vec<_>>()
        };
        for session_id in &stale {
            transaction.execute(
                "DELETE FROM worker_engagements WHERE session_id = ?1",
                [session_id.to_string()],
            )?;
            transaction.execute(
                "UPDATE worker_sessions SET ended_at = unixepoch()
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
            )?;
        }
        if !stale.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
            insert_control_room_event(&transaction, ControlRoomEventKind::SessionsChanged)?;
        }
        transaction.commit()?;
        Ok(stale.len())
    }

    /// Creates or rotates the digest used to authenticate one agent profile.
    ///
    /// # Errors
    /// Rejects non-SHA-256 digests, unknown workers, and unavailable persistence.
    pub fn replace_worker_agent_credential(
        &self,
        worker_id: WorkerId,
        token_digest: &[u8],
    ) -> Result<(), TaskStoreError> {
        if token_digest.len() != 32 {
            return Err(TaskStoreError::InvalidAgentCredentialDigest);
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            "INSERT INTO worker_agent_credentials (worker_id, token_digest)
             SELECT id, ?2 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL
             ON CONFLICT(worker_id) DO UPDATE SET
                 token_digest = excluded.token_digest,
                 rotated_at = unixepoch()",
            params![worker_id.to_string(), token_digest],
        )?;
        if updated == 0 {
            return Err(TaskStoreError::WorkerNotFound);
        }
        Ok(())
    }

    /// Resolves a digest to its durable worker profile without accepting caller-supplied identity.
    ///
    /// # Errors
    /// Rejects non-SHA-256 digests and propagates persistence failures.
    pub fn authenticate_worker_agent(
        &self,
        token_digest: &[u8],
    ) -> Result<Option<WorkerProfile>, TaskStoreError> {
        if token_digest.len() != 32 {
            return Err(TaskStoreError::InvalidAgentCredentialDigest);
        }
        let worker_id = {
            let connection = self.connection()?;
            connection
                .query_row(
                    "SELECT worker_id FROM worker_agent_credentials WHERE token_digest = ?1",
                    [token_digest],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        };
        worker_id
            .map(|value| {
                WorkerId::from_str(&value)
                    .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
                    .and_then(|id| self.get_worker_profile(id))
            })
            .transpose()
    }
    fn profile_by_role(&self, role: WorkerRole) -> Result<Option<WorkerProfile>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT p.id, p.hive_id, p.name, p.role, p.provider, p.workspace, p.autostart,
                       p.position, s.session_id, p.provider_conversation_id,
                       (EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id)
                        OR p.provider_conversation_resume = 1),
                       e.expires_at,
                       p.created_at, p.updated_at, p.description, p.ephemeral
                FROM worker_profiles p
                LEFT JOIN worker_sessions s
                  ON s.worker_id = p.id AND s.ended_at IS NULL
                LEFT JOIN worker_engagements e
                  ON e.worker_id = p.id AND e.session_id = s.session_id
                 AND e.expires_at > unixepoch()
                WHERE p.role = ?1 AND p.archived_at IS NULL
                ",
                [role.to_string()],
                profile_from_row,
            )
            .optional()
            .map_err(TaskStoreError::from)
    }

    fn insert_profile(
        &self,
        name: &str,
        description: &str,
        role: WorkerRole,
        provider: ProviderKind,
        workspace: &str,
        startup: ProfileStartup,
    ) -> Result<WorkerProfile, TaskStoreError> {
        let ProfileStartup {
            autostart,
            position,
            ephemeral,
        } = startup;
        let name = name.trim();
        let description = description.trim();
        let workspace = normalize_workspace(workspace)?;
        let workspace = workspace.as_str();
        validate_profile(name, workspace)?;
        validate_worker_description(description)?;
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let duplicate = transaction
            .query_row(
                "SELECT 1 FROM worker_profiles
                 WHERE name = ?1 COLLATE NOCASE AND archived_at IS NULL",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Err(TaskStoreError::DuplicateWorkerName);
        }
        if role == WorkerRole::Queen
            && transaction
                .query_row(
                    "SELECT 1 FROM worker_profiles
                     WHERE role = 'queen' AND archived_at IS NULL",
                    [],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
        {
            return Err(TaskStoreError::QueenAlreadyExists);
        }
        let id = WorkerId::new();
        let provider_conversation_id =
            (provider == ProviderKind::ClaudeCode).then(ProviderConversationId::new);
        transaction.execute(
            "INSERT INTO worker_profiles
             (id, hive_id, name, description, role, provider, workspace, autostart, position,
              provider_conversation_id, ephemeral)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id.to_string(),
                hive_id.to_string(),
                name,
                description,
                role.to_string(),
                provider.to_string(),
                workspace,
                autostart,
                position,
                provider_conversation_id.map(|value| value.to_string()),
                ephemeral
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_worker_profile(id)
    }
}

/// The absolute path a worker's workspace means.
///
/// EXPANDS rather than refuses. A profile was stored as `~/projects/...` on
/// 2026-08-16 and never started once: the tilde is not a filesystem concept, so
/// the directory was never found, so no session could spawn. The operator
/// plainly meant a real directory — the directory existed the whole time — so
/// refusing the input would have been correct and useless. Expanding gives them
/// what they meant.
///
/// A path that is still not absolute after expansion IS refused, and says why.
/// A relative workspace resolves against whatever directory a process happens
/// to start in, which is a different bug wearing the same clothes.
pub(crate) fn normalize_workspace(workspace: &str) -> Result<String, TaskStoreError> {
    let workspace = workspace.trim();
    let expanded = match workspace.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            match std::env::var("HOME").ok().filter(|home| !home.is_empty()) {
                Some(home) => format!("{}{rest}", home.trim_end_matches('/')),
                // No home to expand against. Refusing beats storing a tilde
                // that will never resolve.
                None => return Err(TaskStoreError::InvalidWorkspace),
            }
        }
        _ => workspace.to_owned(),
    };
    if !expanded.starts_with('/') {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(expanded)
}

fn validate_profile(name: &str, workspace: &str) -> Result<(), TaskStoreError> {
    validate_worker_name(name)?;
    if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(())
}

fn parse_worker_identity(value: &str) -> Result<WorkerId, TaskStoreError> {
    WorkerId::from_str(value).map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
}

pub(crate) fn validate_worker_name(name: &str) -> Result<(), TaskStoreError> {
    if name.is_empty() || name.len() > MAX_WORKER_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(TaskStoreError::InvalidWorkerName);
    }
    Ok(())
}

pub(crate) fn validate_worker_description(description: &str) -> Result<(), TaskStoreError> {
    if description.len() > MAX_WORKER_DESCRIPTION_BYTES || description.chars().any(char::is_control)
    {
        return Err(TaskStoreError::InvalidWorkerDescription);
    }
    Ok(())
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkerProfile> {
    let id =
        WorkerId::from_str(&row.get::<_, String>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let hive_id =
        HiveId::from_str(&row.get::<_, String>(1)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let role = WorkerRole::from_str(&row.get::<_, String>(3)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    // The row mapper every roster query runs through. Parsing strictly here is
    // what let ONE worker on an unrecognised provider fail the entire listing.
    let provider = ProviderKind::from_stored(&row.get::<_, String>(4)?);
    let session = row
        .get::<_, Option<String>>(8)?
        .map(|value| WorkerSessionId::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()?;
    let provider_conversation_id = row
        .get::<_, Option<String>>(9)?
        .map(|value| {
            ProviderConversationId::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery)
        })
        .transpose()?;
    Ok(WorkerProfile {
        id,
        hive_id,
        name: row.get(2)?,
        description: row.get(14)?,
        role,
        provider,
        workspace: row.get(5)?,
        autostart: row.get::<_, i64>(6)? != 0,
        position: row.get(7)?,
        active_session_id: session,
        provider_conversation_id,
        has_session_history: row.get::<_, i64>(10)? != 0,
        engagement_expires_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        ephemeral: row.get::<_, i64>(15)? != 0,
    })
}

/// Repoints a worker at another repository, when that is what changed.
///
/// Lifted out of `update_worker_profile` because that function had grown past
/// what one should hold. The rule is unchanged.
fn move_worker_repository(
    transaction: &rusqlite::Transaction<'_>,
    worker_id: WorkerId,
    workspace: Option<&str>,
    running: bool,
) -> Result<(), TaskStoreError> {
    let Some(workspace) = workspace else {
        return Ok(());
    };
    let current: String = transaction.query_row(
        "SELECT workspace FROM worker_profiles WHERE id = ?1",
        [worker_id.to_string()],
        |row| row.get(0),
    )?;
    if workspace == current {
        return Ok(());
    }
    if running {
        return Err(TaskStoreError::WorkerMustBeSleeping);
    }
    // A saved conversation belongs to the repository it happened in: the
    // provider keys its history by project path, so carrying the identity
    // across would resume the wrong thread in the wrong place. Moving a worker
    // starts it fresh where it now lives.
    transaction.execute(
        "UPDATE worker_profiles
         SET workspace = ?2, provider_conversation_id = NULL,
             provider_conversation_resume = 0
         WHERE id = ?1",
        params![worker_id.to_string(), workspace],
    )?;
    Ok(())
}

/// Why a worker session ended, and who ended it.
///
/// A worker that is simply not running is the failure this fleet keeps
/// rediscovering in other forms: the state is visible and the reason is not.
/// Standing a worker down deliberately must not become another silent state —
/// an operator looking at a resting worker should be able to tell "Queen stood
/// this down because the queue was empty" from "this crashed" from "nobody has
/// started it yet".
///
/// Nullable, because most sessions end without anyone recording why — a crash,
/// a restart, an operator pressing stop. An absent reason honestly means "not
/// recorded" rather than being backfilled with a guess.
pub(super) fn migrate_session_end_reason(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    // Guarded because the migration chain is re-run against databases whose
    // tables are already current — the migration tests rewind user_version
    // without rewinding the schema, which is exactly the shape of a database
    // that was restored or partly upgraded. An unguarded ADD COLUMN fails with
    // "duplicate column name" and takes the whole upgrade with it.
    // The table itself may not exist yet: pragma_table_info returns nothing for
    // a missing table, so guarding only on the column passes and the ALTER then
    // fails with "no such table". Both halves are needed, which is why
    // presence.rs checks both.
    let table_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'worker_sessions')",
        [],
        |row| row.get(0),
    )?;
    for column in ["ended_reason", "ended_by"] {
        if !table_exists {
            break;
        }
        let present: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_sessions') WHERE name = ?1)",
            [column],
            |row| row.get(0),
        )?;
        if !present {
            transaction.execute_batch(&format!(
                "ALTER TABLE worker_sessions ADD COLUMN {column} TEXT;"
            ))?;
        }
    }
    transaction.execute_batch("PRAGMA user_version = 92;")
}

#[cfg(test)]
mod tests {
    /// A worker stood down on purpose says so, and one that simply stopped does
    /// not pretend to.
    ///
    /// A worker that is merely not running is the failure this fleet keeps
    /// rediscovering: the state is visible and the reason is not. Sleeping must
    /// not become another silent state — a resting worker has to be
    /// distinguishable from a crashed one.
    #[test]
    fn a_session_ended_on_purpose_records_why_and_who() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        store
            .release_worker_session_because(session, Some(("the queue is empty", "queen")))
            .unwrap();

        assert_eq!(
            store.last_session_end_reason(worker.id).unwrap().as_deref(),
            Some("the queue is empty")
        );
    }

    /// An unrecorded ending stays unrecorded. "Not recorded" is the honest
    /// answer, and inventing one would make a crash look deliberate.
    #[test]
    fn a_session_that_just_ended_claims_no_reason() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        store.release_worker_session(session).unwrap();

        assert_eq!(store.last_session_end_reason(worker.id).unwrap(), None);
    }

    /// The newest ending wins, so a worker woken and stood down again reports
    /// why it is resting NOW rather than why it rested last time.
    #[test]
    fn the_most_recent_ending_is_the_one_reported() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        store
            .release_worker_session_because(first, Some(("first rest", "queen")))
            .unwrap();
        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        store
            .release_worker_session_because(second, Some(("second rest", "queen")))
            .unwrap();

        assert_eq!(
            store.last_session_end_reason(worker.id).unwrap().as_deref(),
            Some("second rest")
        );
    }

    /// A profile stored `~/projects/rcg/rcg-dev-install` on 2026-08-16 and never
    /// started once. The tilde is not a filesystem concept, the directory was
    /// never found, and no session could spawn — while the directory it meant
    /// existed the whole time. Three operator rulings were spent on it.
    #[test]
    fn a_tilde_workspace_is_expanded_rather_than_stored_and_hoped_for() {
        let home = std::env::var("HOME").expect("tests run with a home directory");
        assert_eq!(
            super::normalize_workspace("~/projects/rcg/rcg-dev-install").unwrap(),
            format!(
                "{}/projects/rcg/rcg-dev-install",
                home.trim_end_matches('/')
            ),
        );
        // A bare tilde is the home directory itself.
        assert_eq!(
            super::normalize_workspace("~").unwrap(),
            home.trim_end_matches('/')
        );
        // Not every tilde is a home reference: a directory may legitimately
        // start with one.
        assert!(super::normalize_workspace("~notauser/thing").is_err());
    }

    /// Expanding is the fix for a tilde. It is not a licence to accept anything
    /// that resolves differently depending on where a process happened to start.
    #[test]
    fn a_relative_workspace_is_refused_rather_than_guessed_at() {
        assert!(super::normalize_workspace("projects/rcg").is_err());
        assert!(super::normalize_workspace("./projects").is_err());
        assert!(super::normalize_workspace("../elsewhere").is_err());
        assert!(super::normalize_workspace("/home/somebody/projects").is_ok());
    }

    #[test]
    fn a_worker_created_with_a_tilde_gets_a_path_that_can_actually_start() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Installer",
                ProviderKind::ClaudeCode,
                "~/projects/thing",
                false,
                1,
            )
            .unwrap();
        assert!(
            worker.workspace.starts_with('/'),
            "stored workspace must be absolute, got {}",
            worker.workspace
        );
        assert!(!worker.workspace.contains('~'));
    }
    use super::*;

    #[test]
    fn queen_is_a_single_durable_autostart_profile() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let same = store.ensure_queen("/ignored").unwrap();
        assert_eq!(queen.id, same.id);
        assert_eq!(queen.name, "Queen");
        assert_eq!(queen.role, WorkerRole::Queen);
        assert!(queen.autostart);
    }

    #[test]
    fn exact_project_root_is_promoted_to_protected_sleeping_scout_in_place() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let project_root = store
            .create_worker_with_description(
                "Project Root",
                "Coordinates deliberate cross-repository changes.",
                ProviderKind::ClaudeCode,
                "/workspace/projects",
                true,
                9,
            )
            .unwrap();
        let conversation = project_root.provider_conversation_id;
        let ordinary = store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                "/workspace/daisy",
                false,
                10,
            )
            .unwrap();

        assert!(
            store
                .promote_project_root_to_scout("/workspace/other")
                .unwrap()
                .is_none()
        );
        let scout = store
            .promote_project_root_to_scout("/workspace/projects")
            .unwrap()
            .unwrap();
        assert_eq!(scout.id, project_root.id);
        assert_eq!(scout.name, "Scout");
        assert_eq!(scout.description, project_root.description);
        assert_eq!(scout.provider_conversation_id, conversation);
        assert_eq!(scout.role, WorkerRole::Worker);
        assert!(!scout.autostart);
        assert_eq!(store.scout_worker_id().unwrap(), Some(scout.id));
        assert!(matches!(
            store.update_worker_profile(scout.id, Some("Root"), None, None, None, None),
            Err(TaskStoreError::ScoutIdentityImmutable)
        ));
        assert!(matches!(
            store.archive_worker_profile(scout.id),
            Err(TaskStoreError::ScoutIdentityImmutable)
        ));
        let updated = store
            .update_worker_profile(
                scout.id,
                Some("Scout"),
                Some("Routes larger cross-repository work."),
                Some(ProviderKind::Codex),
                Some(false),
                None,
            )
            .unwrap();
        assert_eq!(updated.provider, ProviderKind::Codex);
        let reordered = store.reorder_workers(&[ordinary.id]).unwrap();
        assert_eq!(
            reordered
                .iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            vec![queen.id, updated.id, ordinary.id]
        );
    }

    #[test]
    fn profile_outlives_replaced_process_sessions() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Violet", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .active_session_id,
            Some(first)
        );
        assert!(matches!(
            store.bind_worker_session(worker.id, WorkerSessionId::new()),
            Err(TaskStoreError::WorkerAlreadyRunning)
        ));
        assert!(store.release_worker_session(first).unwrap());
        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        let current = store.get_worker_profile(worker.id).unwrap();
        assert_eq!(current.id, worker.id);
        assert_eq!(current.active_session_id, Some(second));
    }

    /// What an unplanned reboot did to the whole board.
    ///
    /// The startup sweep ends sessions whose process is gone, but leaves their
    /// assignments and queued briefings behind. Delivery requires the
    /// assignment's own session to be live, so every briefing was stranded, and
    /// the repair in `bind_worker_session` could not see those tasks either —
    /// it skips anything still holding an unreleased assignment. Measured
    /// 2026-08-24: seventeen of eighteen queued briefings, on four workers that
    /// were all running again.
    #[test]
    fn work_survives_a_worker_session_that_ended_without_being_released() {
        use swarm_domain::{TaskActivityActor, TaskPriority, TaskState};
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        let task = store
            .create_task_with_details(
                "Waiting on a reboot",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        // The machine goes down. Nothing releases anything; on the way back up
        // the sweep only observes that the process is gone.
        assert_eq!(
            store
                .release_missing_worker_sessions(&HashSet::new())
                .unwrap(),
            1
        );
        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        let claimed = store
            .claim_task_dispatches(1_000, &std::collections::HashSet::new())
            .unwrap();
        let delivered = claimed
            .iter()
            .find(|dispatch| dispatch.task_id == task.id)
            .expect("work assigned before a reboot must still be deliverable after it");
        assert_eq!(
            delivered.session_id, second,
            "and it must be delivered to the session the worker is actually running"
        );
        assert_eq!(
            claimed.iter().filter(|d| d.task_id == task.id).count(),
            1,
            "the stranded briefing must be cleared, not delivered twice"
        );
    }

    #[test]
    fn stale_host_bindings_are_released_without_deleting_profiles() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Rose", ProviderKind::ClaudeCode, "/workspace", true, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert_eq!(
            store
                .release_missing_worker_sessions(&HashSet::new())
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .active_session_id,
            None
        );
    }

    #[test]
    fn worker_order_is_exact_and_never_moves_the_queen() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let violet = store
            .create_worker(
                "Violet",
                ProviderKind::ClaudeCode,
                "/workspace/violet",
                false,
                1,
            )
            .unwrap();
        let poppy = store
            .create_worker(
                "Poppy",
                ProviderKind::ClaudeCode,
                "/workspace/poppy",
                false,
                2,
            )
            .unwrap();

        let reordered = store.reorder_workers(&[poppy.id, violet.id]).unwrap();

        assert_eq!(reordered[0].id, queen.id);
        assert_eq!(reordered[1].id, poppy.id);
        assert_eq!(reordered[2].id, violet.id);
        assert!(matches!(
            store.reorder_workers(&[violet.id]),
            Err(TaskStoreError::InvalidWorkerOrder)
        ));
        assert!(matches!(
            store.reorder_workers(&[violet.id, violet.id]),
            Err(TaskStoreError::InvalidWorkerOrder)
        ));
    }

    #[test]
    fn operator_can_rename_a_worker_and_choose_reboot_startup() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                "/workspace/daisy",
                false,
                1,
            )
            .unwrap();
        let conversation = worker.provider_conversation_id;

        let updated = store
            .update_worker_profile(
                worker.id,
                Some(" Clover "),
                Some("Owns subscriptions and billing."),
                None,
                Some(true),
                None,
            )
            .unwrap();

        assert_eq!(updated.name, "Clover");
        assert!(updated.autostart);
        assert_eq!(updated.description, "Owns subscriptions and billing.");
        assert_eq!(updated.workspace, worker.workspace);
        assert_eq!(updated.provider_conversation_id, conversation);
    }

    #[test]
    fn worker_maintenance_rejects_queen_duplicates_and_empty_updates() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let daisy = store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                "/workspace/daisy",
                false,
                1,
            )
            .unwrap();
        let poppy = store
            .create_worker(
                "Poppy",
                ProviderKind::ClaudeCode,
                "/workspace/poppy",
                false,
                2,
            )
            .unwrap();

        assert!(matches!(
            store.update_worker_profile(queen.id, Some("Empress"), None, None, None, None),
            Err(TaskStoreError::QueenProfileImmutable)
        ));
        assert!(matches!(
            store.update_worker_profile(daisy.id, Some("poppy"), None, None, None, None),
            Err(TaskStoreError::DuplicateWorkerName)
        ));
        assert!(matches!(
            store.update_worker_profile(poppy.id, None, None, None, None, None),
            Err(TaskStoreError::EmptyWorkerUpdate)
        ));
    }

    #[test]
    fn sleeping_worker_can_change_provider_without_losing_claude_conversation() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Aster",
                ProviderKind::ClaudeCode,
                "/workspace/aster",
                false,
                1,
            )
            .unwrap();
        let conversation = worker.provider_conversation_id;

        let updated = store
            .update_worker_profile(worker.id, None, None, Some(ProviderKind::Codex), None, None)
            .unwrap();
        assert_eq!(updated.provider, ProviderKind::Codex);
        assert_eq!(updated.provider_conversation_id, conversation);

        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(matches!(
            store.update_worker_profile(
                worker.id,
                None,
                None,
                Some(ProviderKind::ClaudeCode),
                None,
                None,
            ),
            Err(TaskStoreError::WorkerMustBeSleeping)
        ));
    }

    #[test]
    fn removal_is_sleeping_unassigned_and_history_preserving() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();

        assert!(matches!(
            store.archive_worker_profile(queen.id),
            Err(TaskStoreError::QueenProfileImmutable)
        ));
        let task = store
            .create_task("Owned work", worker.workspace.as_str())
            .unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        assert!(matches!(
            store.archive_worker_profile(worker.id),
            Err(TaskStoreError::WorkerOwnsOpenTasks)
        ));
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Active)
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Review)
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Completed)
            .unwrap();

        store.archive_worker_profile(worker.id).unwrap();
        assert!(matches!(
            store.get_worker_profile(worker.id),
            Err(TaskStoreError::WorkerNotFound)
        ));
        assert_eq!(store.list_worker_profiles().unwrap(), vec![queen]);
        let replacement = store
            .create_worker("Clover", ProviderKind::Codex, "/workspace/clover", false, 1)
            .unwrap();
        assert_ne!(replacement.id, worker.id);
    }

    #[test]
    fn new_profiles_keep_one_exact_provider_conversation_across_processes() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Iris", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let conversation_id = worker
            .provider_conversation_id
            .expect("new profiles receive an exact provider conversation");
        assert!(!worker.has_session_history);

        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        store.release_worker_session(first).unwrap();

        let recovered = store.get_worker_profile(worker.id).unwrap();
        assert_eq!(recovered.provider_conversation_id, Some(conversation_id));
        assert!(recovered.has_session_history);
    }

    #[test]
    fn codex_profiles_leave_thread_identity_to_the_provider() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Aster", ProviderKind::Codex, "/workspace", false, 1)
            .unwrap();
        assert_eq!(worker.provider_conversation_id, None);
        assert!(!worker.has_session_history);

        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        store.release_worker_session(first).unwrap();
        assert!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .has_session_history
        );
    }

    #[test]
    fn migrated_profiles_with_history_remain_eligible_for_workspace_continue() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store.release_worker_session(session).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_profiles SET provider_conversation_id = NULL WHERE id = ?1",
                [worker.id.to_string()],
            )
            .unwrap();

        let migrated = store.get_worker_profile(worker.id).unwrap();
        assert_eq!(migrated.provider_conversation_id, None);
        assert!(migrated.has_session_history);
        assert!(matches!(
            store.assign_provider_conversation(worker.id),
            Err(TaskStoreError::ProviderConversationUnavailable)
        ));
    }

    #[test]
    fn operator_engagement_is_exclusive_bounded_and_released_with_the_session() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Dahlia", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(store.worker_accepts_injection(worker.id, 100).unwrap());

        let desktop = PresenceDeviceId::new();
        assert!(
            store
                .renew_worker_engagement(session, Some(desktop), 100, 300)
                .unwrap()
        );
        assert!(!store.worker_accepts_injection(worker.id, 101).unwrap());
        assert!(
            !store
                .renew_worker_engagement(session, Some(desktop), 101, 300)
                .unwrap()
        );
        assert!(
            store
                .renew_worker_engagement(session, Some(desktop), 260, 300)
                .unwrap()
        );
        assert!(!store.worker_accepts_injection(worker.id, 559).unwrap());
        assert!(store.worker_accepts_injection(worker.id, 561).unwrap());

        store.release_worker_session(session).unwrap();
        assert!(store.worker_accepts_injection(worker.id, 261).unwrap());
        assert!(matches!(
            store.renew_worker_engagement(session, Some(desktop), 262, 300),
            Err(TaskStoreError::WorkerSessionNotActive)
        ));
    }

    #[test]
    fn engagement_release_is_owned_by_the_device_that_last_typed() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Clover", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        let desktop = PresenceDeviceId::new();
        let phone = PresenceDeviceId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        store
            .renew_worker_engagement(session, Some(desktop), 100, 300)
            .unwrap();
        assert!(!store.release_worker_engagement(session, phone).unwrap());
        assert!(!store.worker_accepts_injection(worker.id, 101).unwrap());

        assert!(
            store
                .renew_worker_engagement(session, Some(phone), 102, 300)
                .unwrap()
        );
        assert!(!store.release_worker_engagement(session, desktop).unwrap());
        assert!(!store.worker_accepts_injection(worker.id, 103).unwrap());
        assert!(store.release_worker_engagement(session, phone).unwrap());
        assert!(store.worker_accepts_injection(worker.id, 103).unwrap());
    }

    #[test]
    fn live_sessions_report_when_they_started_and_what_they_run() {
        // A provider process executes the release it started with, so the start
        // time is what decides whether a worker is running something disk has
        // moved past.
        let store = TaskStore::in_memory().unwrap();
        let claude = store
            .create_worker("Ginger", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let codex = store
            .create_worker("Hazel", ProviderKind::Codex, "/workspace", false, 2)
            .unwrap();
        let ended = store
            .create_worker("Ivy", ProviderKind::ClaudeCode, "/workspace", false, 3)
            .unwrap();
        let ended_session = WorkerSessionId::new();
        store
            .bind_worker_session(claude.id, WorkerSessionId::new())
            .unwrap();
        store
            .bind_worker_session(codex.id, WorkerSessionId::new())
            .unwrap();
        store.bind_worker_session(ended.id, ended_session).unwrap();
        store.release_worker_session(ended_session).unwrap();

        let live = store.active_worker_sessions().unwrap();

        // A worker that is asleep is not running an old release; it is not
        // running one at all.
        assert_eq!(live.len(), 2);
        assert!(
            live.iter()
                .any(|s| s.worker_id == claude.id && s.provider == ProviderKind::ClaudeCode)
        );
        assert!(
            live.iter()
                .any(|s| s.worker_id == codex.id && s.provider == ProviderKind::Codex)
        );
        assert!(live.iter().all(|s| s.started_at > 0));
    }

    #[test]
    fn workers_owed_a_return_outlive_the_request_that_stopped_them() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .create_worker("Elder", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let second = store
            .create_worker("Fennel", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();

        store
            .record_worker_revival_intents(&[first.id, second.id], 1_000)
            .unwrap();
        assert_eq!(
            store.worker_revival_intents(1_100, 900).unwrap(),
            vec![first.id, second.id]
        );

        // Honouring one leaves the other owed.
        store.clear_worker_revival_intent(first.id).unwrap();
        assert_eq!(
            store.worker_revival_intents(1_100, 900).unwrap(),
            vec![second.id]
        );

        // An intent that outlives its maintenance is dropped rather than
        // waking a worker the operator has since left asleep.
        assert!(store.worker_revival_intents(1_901, 900).unwrap().is_empty());
        assert!(store.worker_revival_intents(1_902, 900).unwrap().is_empty());
    }

    #[test]
    fn one_device_engages_one_worker_at_a_time() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .create_worker("Aster", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let second = store
            .create_worker("Borage", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        let first_session = WorkerSessionId::new();
        let second_session = WorkerSessionId::new();
        store.bind_worker_session(first.id, first_session).unwrap();
        store
            .bind_worker_session(second.id, second_session)
            .unwrap();
        let desktop = PresenceDeviceId::new();

        store
            .renew_worker_engagement(first_session, Some(desktop), 100, 300)
            .unwrap();
        assert!(!store.worker_accepts_injection(first.id, 101).unwrap());

        // Moving to another worker gives the first one back, well inside the
        // lease it was holding. Three workers reading "with you" at once is
        // what this prevents.
        store
            .renew_worker_engagement(second_session, Some(desktop), 102, 300)
            .unwrap();
        assert!(store.worker_accepts_injection(first.id, 103).unwrap());
        assert!(!store.worker_accepts_injection(second.id, 103).unwrap());

        // A second operator device is a separate claim, not a competing one.
        let phone = PresenceDeviceId::new();
        store
            .renew_worker_engagement(first_session, Some(phone), 104, 300)
            .unwrap();
        assert!(!store.worker_accepts_injection(first.id, 105).unwrap());
        assert!(!store.worker_accepts_injection(second.id, 105).unwrap());
    }

    #[test]
    fn an_unattributed_engagement_is_left_alone_by_the_sweep() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .create_worker("Cosmos", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let second = store
            .create_worker("Dill", ProviderKind::ClaudeCode, "/workspace", false, 2)
            .unwrap();
        let first_session = WorkerSessionId::new();
        let second_session = WorkerSessionId::new();
        store.bind_worker_session(first.id, first_session).unwrap();
        store
            .bind_worker_session(second.id, second_session)
            .unwrap();

        // Engagements with no device behind them cannot be told apart, so
        // clearing them on someone else's behalf would be a guess.
        store
            .renew_worker_engagement(first_session, None, 100, 300)
            .unwrap();
        store
            .renew_worker_engagement(second_session, None, 102, 300)
            .unwrap();
        assert!(!store.worker_accepts_injection(first.id, 103).unwrap());
        assert!(!store.worker_accepts_injection(second.id, 103).unwrap());
    }

    /// The measurement the operator asked for: "You need a way to measure the
    /// terminal fight so this isn't guesswork."
    ///
    /// A fight and a legitimate handover look identical in a screenshot. They
    /// do not look identical here: one device taking the size once is a
    /// handover, and two devices taking it from each other repeatedly is the
    /// thing being reported.
    #[test]
    fn the_ledger_tells_a_handover_apart_from_a_fight() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Clover", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        let desktop = PresenceDeviceId::new();
        let phone = PresenceDeviceId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        // The desktop is looking at it, then the phone takes it. Once.
        store
            .claim_unowned_worker_geometry(session, desktop)
            .unwrap();
        store
            .record_geometry_request(session, Some(desktop), (50, 150), false, true, 1_000)
            .unwrap();
        let granted = store.claim_worker_geometry(session, phone).unwrap();
        store
            .record_geometry_request(session, Some(phone), (60, 40), true, granted, 1_001)
            .unwrap();

        let handover = store.geometry_contention(session, 0).unwrap();
        assert_eq!(handover.handovers, 1, "taking it once is a handover");
        assert_eq!(handover.devices, 2);

        // Now they argue: each takes it back from the other, repeatedly.
        for round in 0..6 {
            let (device, size) = if round % 2 == 0 {
                (desktop, (50, 150))
            } else {
                (phone, (60, 40))
            };
            let granted = store.claim_worker_geometry(session, device).unwrap();
            store
                .record_geometry_request(session, Some(device), size, true, granted, 1_010 + round)
                .unwrap();
        }

        let fight = store.geometry_contention(session, 1_010).unwrap();
        assert_eq!(fight.devices, 2);
        assert_eq!(fight.distinct_sizes, 2, "two sizes, alternating");
        assert!(
            fight.handovers >= 5,
            "a fight is repeated handovers, not one: {fight:?}"
        );
    }

    /// One device resizing its own terminal is not contention, however often it
    /// does it — otherwise dragging a window edge would read as a fight.
    #[test]
    fn one_device_resizing_repeatedly_is_not_contention() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Clover", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        let desktop = PresenceDeviceId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        for step in 0..10 {
            let granted = store.claim_worker_geometry(session, desktop).unwrap();
            store
                .record_geometry_request(
                    session,
                    Some(desktop),
                    (40 + u16::try_from(step).unwrap(), 120),
                    true,
                    granted,
                    2_000 + step,
                )
                .unwrap();
        }

        let measured = store.geometry_contention(session, 0).unwrap();
        assert_eq!(measured.devices, 1);
        assert_eq!(
            measured.handovers, 0,
            "one device resizing its own terminal moves it to nobody"
        );
        assert_eq!(measured.requests, 10);
    }

    #[test]
    fn terminal_geometry_authority_follows_the_last_engaged_device() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Clover", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        let desktop = PresenceDeviceId::new();
        let phone = PresenceDeviceId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        assert!(
            !store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );
        assert!(
            store
                .claim_unowned_worker_geometry(session, desktop)
                .unwrap()
        );
        assert!(!store.claim_unowned_worker_geometry(session, phone).unwrap());
        assert!(
            store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );
        assert!(store.claim_worker_geometry(session, phone).unwrap());
        assert!(
            store
                .device_owns_worker_geometry(session, Some(phone))
                .unwrap()
        );
        assert!(
            !store
                .claim_unowned_worker_geometry(session, desktop)
                .unwrap()
        );
        store
            .renew_worker_engagement(session, Some(desktop), 100, 300)
            .unwrap();
        assert!(
            store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );
        assert!(
            !store
                .device_owns_worker_geometry(session, Some(phone))
                .unwrap()
        );
        assert!(
            store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );
        assert!(store.release_worker_engagement(session, desktop).unwrap());
        assert!(store.worker_accepts_injection(worker.id, 401).unwrap());
        assert!(
            store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );

        store
            .renew_worker_engagement(session, Some(phone), 102, 300)
            .unwrap();
        assert!(
            store
                .device_owns_worker_geometry(session, Some(phone))
                .unwrap()
        );
        assert!(
            !store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );
    }

    #[test]
    fn agent_credentials_are_digest_only_unique_and_rotatable() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .create_worker(
                "Violet",
                ProviderKind::ClaudeCode,
                "/workspace/violet",
                false,
                1,
            )
            .unwrap();
        let second = store
            .create_worker(
                "Pansy",
                ProviderKind::ClaudeCode,
                "/workspace/pansy",
                false,
                2,
            )
            .unwrap();
        let original = [7_u8; 32];
        let rotated = [9_u8; 32];

        store
            .replace_worker_agent_credential(first.id, &original)
            .unwrap();
        assert_eq!(
            store
                .authenticate_worker_agent(&original)
                .unwrap()
                .unwrap()
                .id,
            first.id
        );
        assert!(matches!(
            store.replace_worker_agent_credential(second.id, &original),
            Err(TaskStoreError::Sql(_))
        ));

        store
            .replace_worker_agent_credential(first.id, &rotated)
            .unwrap();
        assert!(
            store
                .authenticate_worker_agent(&original)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .authenticate_worker_agent(&rotated)
                .unwrap()
                .unwrap()
                .id,
            first.id
        );
        assert!(matches!(
            store.authenticate_worker_agent(&[1_u8; 31]),
            Err(TaskStoreError::InvalidAgentCredentialDigest)
        ));
    }
    /// The operator could not repoint a worker after Legacy vacated the folder
    /// it lived in, because a workspace was immutable once set — the field was
    /// display-only and the store had nowhere to put a new one.
    ///
    /// Moving a worker clears its saved conversation. The provider keys history
    /// by project path, so carrying that identity across would resume the wrong
    /// thread in the wrong repository.
    #[test]
    fn moving_a_worker_repoints_it_and_forgets_the_old_conversation() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Swarm (legacy)",
                ProviderKind::ClaudeCode,
                "/projects/old",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        assert!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id
                .is_some()
        );

        // A running worker keeps the repository it is running in.
        assert!(matches!(
            store.update_worker_profile(worker.id, None, None, None, None, Some("/projects/new")),
            Err(TaskStoreError::WorkerMustBeSleeping)
        ));

        store.release_worker_session(session).unwrap();
        let moved = store
            .update_worker_profile(worker.id, None, None, None, None, Some("/projects/new"))
            .unwrap();

        assert_eq!(moved.workspace, "/projects/new");
        assert!(
            moved.provider_conversation_id.is_none(),
            "a conversation belongs to the repository it happened in"
        );
    }

    /// A provider this build has never heard of is READ, not fatal.
    ///
    /// This is the rollback case. A provider is stored as a plain string with no
    /// CHECK constraint, so a Hive that rolls back to a release predating a
    /// provider reads a value it cannot parse. Rollback is routine here — the
    /// packaging lifecycle test exercises it and the release tooling restores
    /// the previous API automatically on a failed health check.
    ///
    /// The harm is not the one row. `list_worker_profiles` maps every row through
    /// one mapper, so a strict parse meant ONE worker adopted onto a newer
    /// provider took down the entire roster for every worker beside it.
    ///
    /// The ablation is the second half: swap `from_stored` back to `from_str` in
    /// `profile_from_row` and the listing assertion fails outright rather than
    /// returning a degraded row, which is the behaviour this exists to prevent.
    #[test]
    fn a_provider_from_a_newer_release_degrades_instead_of_failing_the_roster() {
        let store = TaskStore::in_memory().unwrap();
        let readable = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let rolled_back = store
            .create_worker(
                "Vicky",
                ProviderKind::ClaudeCode,
                "/workspace/vicky",
                false,
                1,
            )
            .unwrap();
        // Written the way a NEWER build would write it. Schema 96 dropped the
        // closed provider list, so this needs no constraint bypass -- and that
        // it does not is itself the point: an earlier version of this test had
        // to disable CHECK enforcement, which is how the constraint that
        // supposedly did not exist was discovered.
        //
        // THE NAME HAS TO BE ONE THIS BUILD GENUINELY DOES NOT KNOW. It was
        // "gemini" until Gemini shipped as a real provider, at which point this
        // test failed -- correctly, because its premise had quietly become
        // false and it was asserting tolerance while exercising a known value.
        // Any future provider added here must move this string again rather
        // than adjust the assertion.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_profiles SET provider = ?2 WHERE id = ?1",
                rusqlite::params![rolled_back.id.to_string(), "a_provider_from_the_future"],
            )
            .unwrap();

        // The whole roster still lists, which is the point.
        let roster = store.list_worker_profiles().unwrap();
        assert_eq!(
            roster.len(),
            2,
            "one unreadable row must not fail the listing"
        );
        let degraded = roster
            .iter()
            .find(|profile| profile.id == rolled_back.id)
            .expect("the worker on an unknown provider is still listed");
        assert_eq!(degraded.provider, ProviderKind::Unsupported);
        assert_eq!(
            roster
                .iter()
                .find(|profile| profile.id == readable.id)
                .expect("its neighbour is unaffected")
                .provider,
            ProviderKind::ClaudeCode
        );

        // Reading one directly degrades the same way rather than erroring.
        assert_eq!(
            store.get_worker_profile(rolled_back.id).unwrap().provider,
            ProviderKind::Unsupported
        );

        // And the stored value is NOT overwritten by the placeholder. Losing the
        // row is recoverable; rewriting "gemini" as "unsupported" would destroy
        // the only record of what the worker actually was.
        assert!(matches!(
            store.update_worker_profile(
                rolled_back.id,
                None,
                None,
                Some(ProviderKind::Unsupported),
                None,
                None
            ),
            Err(TaskStoreError::IntegrityFailure(_))
        ));
        let stored: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT provider FROM worker_profiles WHERE id = ?1",
                [rolled_back.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored, "a_provider_from_the_future",
            "the real provider survives being unreadable"
        );
    }

    /// A temporary worker is adopted by CHANGING A FLAG, not by re-creating it.
    ///
    /// The identity is the whole point. A temporary worker holds the full tool
    /// surface, so by the time anyone adopts it, it may already have transitioned
    /// tasks and filed new ones. Re-creating it under a permanent name would
    /// leave every one of those writes naming a worker that no longer exists —
    /// the same defect as an unreadable provider taking down the roster, one
    /// layer up.
    ///
    /// Releasing archives rather than deletes, for the same reason: the row has
    /// to outlive the worker or its writes point at nothing. That is why the
    /// action is called Release rather than Kill.
    #[test]
    fn a_temporary_worker_keeps_its_identity_through_adoption_and_release() {
        let store = TaskStore::in_memory().unwrap();
        let temporary = store
            .create_temporary_worker("Codex scratch", ProviderKind::Codex, "/workspace/petal", 5)
            .unwrap();
        assert!(temporary.ephemeral, "it is created temporary");
        assert_eq!(temporary.provider, ProviderKind::Codex);

        // It runs like any worker, which is what gives it history to preserve.
        let session = WorkerSessionId::new();
        store.bind_worker_session(temporary.id, session).unwrap();
        store.release_worker_session(session).unwrap();

        let adopted = store.adopt_worker(temporary.id, "Thistle").unwrap();
        assert_eq!(adopted.id, temporary.id, "adoption preserves the identity");
        assert_eq!(adopted.name, "Thistle");
        assert!(!adopted.ephemeral, "it is no longer temporary");
        assert!(
            adopted.has_session_history,
            "its history survives adoption; a re-creation would have lost it"
        );

        // Adopting twice is refused rather than silently accepted: the second
        // caller believes something false about the worker.
        assert!(matches!(
            store.adopt_worker(temporary.id, "Bramble"),
            Err(TaskStoreError::WorkerNotFound)
        ));

        // Release archives. The row must outlive the worker.
        let released = store
            .create_temporary_worker(
                "Codex scratch two",
                ProviderKind::Codex,
                "/workspace/petal",
                6,
            )
            .unwrap();
        store.archive_worker_profile(released.id).unwrap();
        let survives: bool = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM worker_profiles
                                WHERE id = ?1 AND archived_at IS NOT NULL)",
                [released.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            survives,
            "a released worker's row remains, so its writes still name it"
        );
        assert!(
            !store
                .list_worker_profiles()
                .unwrap()
                .iter()
                .any(|profile| profile.id == released.id),
            "but it is gone from the roster"
        );
    }

    /// Saving the same path is not a move, so it costs nothing.
    #[test]
    fn repeating_a_workers_own_repository_changes_nothing() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/projects/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        // Running, but the repository is unchanged, so nothing is refused.
        let same = store
            .update_worker_profile(worker.id, None, None, None, None, Some("/projects/petal"))
            .unwrap();

        assert_eq!(same.workspace, "/projects/petal");
        assert!(same.provider_conversation_id.is_some());
    }
    /// "When I open the desktop and mobile on the same worker it is jumping ALL
    /// over the place still."
    ///
    /// Both believed they were the foreground — one browser's idea of focus
    /// says nothing about another machine — so each claimed geometry, restored
    /// at the other's size, re-fitted to its own and claimed again. The device
    /// holding the worker now decides, and nothing else does.
    #[test]
    fn only_the_device_holding_a_worker_decides_its_terminal_size() {
        // The lease is compared against the wall clock in SQL, so the test's
        // idea of "now" has to be one the database agrees is the future.
        const FUTURE: i64 = 4_000_000_000;
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Swarm",
                ProviderKind::ClaudeCode,
                "/workspace/swarm",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let desktop = PresenceDeviceId::new();
        let phone = PresenceDeviceId::new();

        // Nobody holds it, so the first to ask may size it.
        assert!(store.claim_worker_geometry(session, desktop).unwrap());

        // The desktop takes the worker.
        store
            .renew_worker_engagement(session, Some(desktop), FUTURE, 300)
            .unwrap();

        // The phone is watching, and its idea of being in front is its own.
        assert!(
            !store.claim_worker_geometry(session, phone).unwrap(),
            "a viewer cannot resize a worker another device is holding"
        );
        // The holder still can.
        assert!(store.claim_worker_geometry(session, desktop).unwrap());

        // Moving the worker moves the authority with it, which is what "Work
        // here" does.
        store
            .renew_worker_engagement(session, Some(phone), FUTURE + 1, 300)
            .unwrap();
        assert!(store.claim_worker_geometry(session, phone).unwrap());
        assert!(!store.claim_worker_geometry(session, desktop).unwrap());
    }

    /// "I left this in desktop. Went to mobile and it just kept refreshing."
    ///
    /// Opening a socket used to take the geometry from whoever held it, so a
    /// second viewer stole the size simply by looking, the first took it back
    /// on its next resize, and the two oscillated. Connecting says only that
    /// someone is looking; intent arrives separately.
    #[test]
    fn merely_looking_at_a_worker_does_not_take_its_terminal_size() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Swarm",
                ProviderKind::ClaudeCode,
                "/workspace/swarm",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let desktop = PresenceDeviceId::new();
        let phone = PresenceDeviceId::new();

        // The desktop opens it first and sizes it.
        assert!(
            store
                .claim_unowned_worker_geometry(session, desktop)
                .unwrap()
        );

        // The phone opens the same worker. This is what a socket connect does
        // now, and it must not move the authority.
        assert!(
            !store.claim_unowned_worker_geometry(session, phone).unwrap(),
            "opening a worker someone else is sizing must not take it"
        );
        assert!(
            store
                .device_owns_worker_geometry(session, Some(desktop))
                .unwrap()
        );

        // A deliberate resize is still a claim, which is what lets the person
        // actually looking at it repair a size left by another device.
        assert!(store.claim_worker_geometry(session, phone).unwrap());
    }
}
