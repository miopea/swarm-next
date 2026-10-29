//! Whether Queen's review is enough to release code on this Hive (ADR 0131).
//!
//! Off unless the operator turns it on. On, Queen tells a worker to commit,
//! push, mark ready, merge and deploy reviewed work rather than asking the
//! operator to approve it, and each Claude worker's auto mode is told that
//! Queen's instruction is the operator's chosen authority for releasing.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::ControlRoomEventKind;

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

pub(crate) fn migrate_queen_releases(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_release_settings (
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
            "SELECT enabled FROM queen_release_settings WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

impl TaskStore {
    /// Whether Queen's review is enough to release code on this Hive.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn queen_releases(&self) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        enabled(&connection)
    }

    /// The operator choosing whether Queen's review is enough to release code.
    /// A worker's auto mode and brief take it when that worker next starts;
    /// Queen's review prompt takes it at her next review.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn set_queen_releases(&self, enabled_now: bool, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if enabled(&transaction)? != enabled_now {
            transaction.execute(
                "INSERT INTO queen_release_settings (singleton, enabled, updated_at)
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
    fn queen_releases_only_once_the_operator_turns_it_on() {
        let store = TaskStore::in_memory().unwrap();

        assert!(!store.queen_releases().unwrap());
        assert!(store.set_queen_releases(true, 10).unwrap());
        assert!(store.queen_releases().unwrap());
        assert!(!store.set_queen_releases(false, 20).unwrap());
        assert!(!store.queen_releases().unwrap());
    }
}
