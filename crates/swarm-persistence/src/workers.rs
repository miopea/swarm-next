use std::{collections::HashSet, str::FromStr};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, HiveId, PresenceDeviceId, ProviderConversationId, ProviderKind, WorkerId,
    WorkerProfile, WorkerRole, WorkerSessionId,
};
use uuid::Uuid;

use super::{MAX_WORKSPACE_BYTES, TaskStore, TaskStoreError, insert_control_room_event};

const MAX_WORKER_NAME_BYTES: usize = 80;
const MAX_WORKER_DESCRIPTION_BYTES: usize = 2_000;

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
            (true, 0),
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
        let workspace = workspace.trim();
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
            (autostart, position),
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
            (autostart, position),
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
    ) -> Result<WorkerProfile, TaskStoreError> {
        if name.is_none() && description.is_none() && provider.is_none() && autostart.is_none() {
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
        transaction.execute(
            "UPDATE worker_profiles SET updated_at = unixepoch() WHERE id = ?1",
            [worker_id.to_string()],
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
                                 AND state != 'completed')
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
                   EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id),
                   e.expires_at,
                   p.created_at, p.updated_at, p.description
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
    /// Returns `WorkerNotFound` when the identity is unknown.
    pub fn get_worker_profile(&self, id: WorkerId) -> Result<WorkerProfile, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT p.id, p.hive_id, p.name, p.role, p.provider, p.workspace, p.autostart,
                       p.position, s.session_id, p.provider_conversation_id,
                       EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id),
                       e.expires_at,
                       p.created_at, p.updated_at, p.description
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
                "SELECT task.id
                 FROM tasks task
                 WHERE task.assigned_worker_id = ?1 AND task.state != 'completed'
                   AND NOT EXISTS (
                       SELECT 1 FROM task_assignments assignment
                       WHERE assignment.task_id = task.id AND assignment.released_at IS NULL
                   )
                 ORDER BY task.position, task.id",
            )?;
            statement
                .query_map([worker_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for task_id in &owned_tasks {
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
            if !previously_briefed {
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
        if current.is_some_and(|(expiry, current_owner)| {
            expiry >= renewal_threshold && current_owner == owner_device_id
        }) {
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
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let released = transaction.execute(
            "UPDATE worker_sessions SET ended_at = unixepoch()
             WHERE session_id = ?1 AND ended_at IS NULL",
            [session_id.to_string()],
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
                       EXISTS(SELECT 1 FROM worker_sessions history WHERE history.worker_id = p.id),
                       e.expires_at,
                       p.created_at, p.updated_at, p.description
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
        startup: (bool, i64),
    ) -> Result<WorkerProfile, TaskStoreError> {
        let (autostart, position) = startup;
        let name = name.trim();
        let description = description.trim();
        let workspace = workspace.trim();
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
              provider_conversation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                provider_conversation_id.map(|value| value.to_string())
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_worker_profile(id)
    }
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

fn validate_worker_name(name: &str) -> Result<(), TaskStoreError> {
    if name.is_empty() || name.len() > MAX_WORKER_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(TaskStoreError::InvalidWorkerName);
    }
    Ok(())
}

fn validate_worker_description(description: &str) -> Result<(), TaskStoreError> {
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
    let provider = ProviderKind::from_str(&row.get::<_, String>(4)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
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
    })
}

#[cfg(test)]
mod tests {
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
            store.update_worker_profile(scout.id, Some("Root"), None, None, None),
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
            store.update_worker_profile(queen.id, Some("Empress"), None, None, None),
            Err(TaskStoreError::QueenProfileImmutable)
        ));
        assert!(matches!(
            store.update_worker_profile(daisy.id, Some("poppy"), None, None, None),
            Err(TaskStoreError::DuplicateWorkerName)
        ));
        assert!(matches!(
            store.update_worker_profile(poppy.id, None, None, None, None),
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
            .update_worker_profile(worker.id, None, None, Some(ProviderKind::Codex), None)
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
}
