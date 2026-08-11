use std::{collections::HashSet, str::FromStr};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{ProviderKind, WorkerId, WorkerProfile, WorkerRole, WorkerSessionId};

use super::{MAX_WORKSPACE_BYTES, TaskStore, TaskStoreError};

const MAX_WORKER_NAME_BYTES: usize = 80;

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
            WorkerRole::Queen,
            ProviderKind::ClaudeCode,
            workspace,
            true,
            0,
        )
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
            WorkerRole::Worker,
            provider,
            workspace,
            autostart,
            position,
        )
    }

    /// Lists the roster in stable operator order with its current session binding.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or contains invalid data.
    pub fn list_worker_profiles(&self) -> Result<Vec<WorkerProfile>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT p.id, p.name, p.role, p.provider, p.workspace, p.autostart,
                   p.position, s.session_id, p.created_at, p.updated_at
            FROM worker_profiles p
            LEFT JOIN worker_sessions s
              ON s.worker_id = p.id AND s.ended_at IS NULL
            ORDER BY CASE p.role WHEN 'queen' THEN 0 ELSE 1 END,
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
                SELECT p.id, p.name, p.role, p.provider, p.workspace, p.autostart,
                       p.position, s.session_id, p.created_at, p.updated_at
                FROM worker_profiles p
                LEFT JOIN worker_sessions s
                  ON s.worker_id = p.id AND s.ended_at IS NULL
                WHERE p.id = ?1
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
                "SELECT 1 FROM worker_profiles WHERE id = ?1",
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
        transaction.execute(
            "UPDATE worker_profiles SET updated_at = unixepoch() WHERE id = ?1",
            [worker_id.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Releases a session binding after its process exits or is stopped.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn release_worker_session(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE worker_sessions SET ended_at = unixepoch()
             WHERE session_id = ?1 AND ended_at IS NULL",
            [session_id.to_string()],
        )? == 1)
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
                "UPDATE worker_sessions SET ended_at = unixepoch()
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(stale.len())
    }

    fn profile_by_role(&self, role: WorkerRole) -> Result<Option<WorkerProfile>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT p.id, p.name, p.role, p.provider, p.workspace, p.autostart,
                       p.position, s.session_id, p.created_at, p.updated_at
                FROM worker_profiles p
                LEFT JOIN worker_sessions s
                  ON s.worker_id = p.id AND s.ended_at IS NULL
                WHERE p.role = ?1
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
        role: WorkerRole,
        provider: ProviderKind,
        workspace: &str,
        autostart: bool,
        position: i64,
    ) -> Result<WorkerProfile, TaskStoreError> {
        let name = name.trim();
        let workspace = workspace.trim();
        validate_profile(name, workspace)?;
        let connection = self.connection()?;
        let duplicate = connection
            .query_row(
                "SELECT 1 FROM worker_profiles WHERE name = ?1 COLLATE NOCASE",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Err(TaskStoreError::DuplicateWorkerName);
        }
        if role == WorkerRole::Queen
            && connection
                .query_row(
                    "SELECT 1 FROM worker_profiles WHERE role = 'queen'",
                    [],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
        {
            return Err(TaskStoreError::QueenAlreadyExists);
        }
        let id = WorkerId::new();
        connection.execute(
            "INSERT INTO worker_profiles
             (id, name, role, provider, workspace, autostart, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id.to_string(),
                name,
                role.to_string(),
                provider.to_string(),
                workspace,
                autostart,
                position
            ],
        )?;
        drop(connection);
        self.get_worker_profile(id)
    }
}

fn validate_profile(name: &str, workspace: &str) -> Result<(), TaskStoreError> {
    if name.is_empty() || name.len() > MAX_WORKER_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(TaskStoreError::InvalidWorkerName);
    }
    if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(())
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkerProfile> {
    let id =
        WorkerId::from_str(&row.get::<_, String>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let role = WorkerRole::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let provider = ProviderKind::from_str(&row.get::<_, String>(3)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let session = row
        .get::<_, Option<String>>(7)?
        .map(|value| WorkerSessionId::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()?;
    Ok(WorkerProfile {
        id,
        name: row.get(1)?,
        role,
        provider,
        workspace: row.get(4)?,
        autostart: row.get::<_, i64>(5)? != 0,
        position: row.get(6)?,
        active_session_id: session,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
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
}
