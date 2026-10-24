//! Whether this Hive's Claude workers connect to Remote Control (ADR 0122).
//!
//! Off unless the operator turns it on. Claude refuses `/restart` while Remote
//! Control is connected, so a worker connected to it cannot be moved onto a
//! newly installed Claude in place; it keeps the older release until it is
//! stopped and started again.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::ControlRoomEventKind;

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

pub(crate) fn migrate_worker_remote_control(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_remote_control_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             updated_at INTEGER NOT NULL
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

fn enabled(connection: &Connection) -> Result<bool, TaskStoreError> {
    Ok(connection
        .query_row(
            "SELECT enabled FROM worker_remote_control_settings WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

impl TaskStore {
    /// Whether Claude workers on this Hive connect to Remote Control.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn worker_remote_control(&self) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        enabled(&connection)
    }

    /// The operator choosing whether Claude workers connect to Remote Control.
    /// It reaches each worker the next time that worker starts.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn set_worker_remote_control(
        &self,
        enabled_now: bool,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if enabled(&transaction)? != enabled_now {
            transaction.execute(
                "INSERT INTO worker_remote_control_settings (singleton, enabled, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     enabled = excluded.enabled, updated_at = excluded.updated_at",
                params![enabled_now, now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        let stored = enabled(&transaction)?;
        transaction.commit()?;
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use crate::TaskStore;

    #[test]
    fn remote_control_is_off_until_the_operator_turns_it_on() {
        let store = TaskStore::in_memory().unwrap();

        assert!(!store.worker_remote_control().unwrap());
        assert!(store.set_worker_remote_control(true, 10).unwrap());
        assert!(store.worker_remote_control().unwrap());
        assert!(!store.set_worker_remote_control(false, 20).unwrap());
        assert!(!store.worker_remote_control().unwrap());
    }
}
