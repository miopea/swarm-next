use crate::{TaskStore, TaskStoreError};
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::WorkspaceSearchSettings;

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspace_search_settings (
        singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
        revision INTEGER NOT NULL CHECK(revision > 0),
        folders_json TEXT NOT NULL
    );",
    )?;
    tx.pragma_update(None, "user_version", super::WORKSPACE_SEARCH_SCHEMA_VERSION)
}

impl TaskStore {
    /// Reads local discovery settings, including the optimistic editor revision.
    /// # Errors
    /// Returns database or malformed saved-setting errors.
    pub fn workspace_search_settings(&self) -> Result<WorkspaceSearchSettings, TaskStoreError> {
        let connection = self.connection()?;
        let row: Option<(u64, String)> = connection
            .query_row(
                "SELECT revision, folders_json FROM workspace_search_settings WHERE singleton = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((revision, json)) = row else {
            return Ok(WorkspaceSearchSettings::default());
        };
        let folders = serde_json::from_str(&json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        Ok(WorkspaceSearchSettings { revision, folders })
    }

    /// Atomically replaces discovery folders if the editor revision is current.
    /// # Errors
    /// Returns invalid settings, overflow, or persistence errors. False is a stale editor.
    pub fn replace_workspace_search_settings(
        &self,
        expected: u64,
        folders: &[String],
    ) -> Result<bool, TaskStoreError> {
        if !WorkspaceSearchSettings::valid_folders(folders) || expected >= i64::MAX as u64 {
            return Err(TaskStoreError::IntegrityFailure(
                "invalid workspace search settings".into(),
            ));
        }
        let json = serde_json::to_string(folders)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let current: u64 = tx
            .query_row(
                "SELECT revision FROM workspace_search_settings WHERE singleton = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if current != expected {
            return Ok(false);
        }
        tx.execute("INSERT INTO workspace_search_settings(singleton, revision, folders_json) VALUES(1, ?1, ?2)
            ON CONFLICT(singleton) DO UPDATE SET revision = excluded.revision, folders_json = excluded.folders_json",
            params![expected + 1, json])?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_is_durable_and_stale_or_invalid_saves_preserve_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(store.workspace_search_settings().unwrap().revision, 0);
        assert!(
            store
                .replace_workspace_search_settings(0, &["/projects".into()])
                .unwrap()
        );
        assert!(!store.replace_workspace_search_settings(0, &[]).unwrap());
        assert!(
            store
                .replace_workspace_search_settings(1, &[String::new()])
                .is_err()
        );
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store.workspace_search_settings().unwrap().folders,
            vec!["/projects"]
        );
        assert!(store.replace_workspace_search_settings(1, &[]).unwrap());
        assert_eq!(store.workspace_search_settings().unwrap().revision, 2);
    }
}
