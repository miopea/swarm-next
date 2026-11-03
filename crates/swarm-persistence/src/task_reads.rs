//! Whether a worker may read production for its own assigned task without
//! asking the operator first (ADR 0146).
//!
//! Off unless the operator turns it on. On, each Claude worker's auto mode is
//! told that a read-only check for the task it holds is the operator's choice,
//! and its brief says such a read needs no command request. A write, a change
//! of any kind, spend and personal data still come to the operator.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::ControlRoomEventKind;

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

/// Adds the Hive-wide choice, and the column recording what each worker's
/// launch carried of it (ADR 0132's record).
pub(crate) fn migrate_task_reads(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_read_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             updated_at INTEGER NOT NULL
         );",
    )?;
    crate::add_column_once(
        transaction,
        "worker_launch_choices",
        "task_reads",
        "INTEGER CHECK (task_reads IN (0, 1))",
        version,
    )
}

fn enabled(connection: &Connection) -> Result<bool, TaskStoreError> {
    Ok(connection
        .query_row(
            "SELECT enabled FROM task_read_settings WHERE singleton = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false))
}

impl TaskStore {
    /// Whether a worker may read production for its own task unasked.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn task_reads(&self) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        enabled(&connection)
    }

    /// The operator choosing whether a worker may read production for its own
    /// task unasked. A worker's auto mode and brief take it when that worker
    /// next starts.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn set_task_reads(&self, enabled_now: bool, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if enabled(&transaction)? != enabled_now {
            transaction.execute(
                "INSERT INTO task_read_settings (singleton, enabled, updated_at)
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
    fn task_reads_only_once_the_operator_turns_it_on() {
        let store = TaskStore::in_memory().unwrap();

        assert!(!store.task_reads().unwrap());
        assert!(store.set_task_reads(true, 10).unwrap());
        assert!(store.task_reads().unwrap());
        assert!(!store.set_task_reads(false, 20).unwrap());
        assert!(!store.task_reads().unwrap());
    }
}
