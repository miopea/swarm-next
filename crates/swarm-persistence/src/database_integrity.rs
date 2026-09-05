use std::{
    sync::TryLockError,
    time::{Duration, Instant},
};

use rusqlite::{Connection, ErrorCode};

use crate::{Ordering, TaskStore, TaskStoreError};

impl TaskStore {
    /// Content-free consequence available even when database access is refused.
    #[must_use]
    pub fn database_recovery_required(&self) -> bool {
        self.recovery_required.load(Ordering::Acquire)
    }

    /// One opportunistic integrity probe. False means busy, never verified healthy.
    ///
    /// # Errors
    /// Reports incomplete checks separately from confirmed damage. The `SQLite`
    /// progress deadline cannot interrupt a stalled kernel filesystem operation.
    pub fn probe_database_integrity(&self) -> Result<bool, TaskStoreError> {
        self.probe_with_budget(Duration::from_secs(1))
    }

    fn probe_with_budget(&self, budget: Duration) -> Result<bool, TaskStoreError> {
        let connection = match self.connection.try_lock() {
            Ok(connection) => connection,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => return Err(TaskStoreError::LockPoisoned),
        };
        if self.database_recovery_required() {
            return Err(TaskStoreError::DatabaseRecoveryRequired);
        }
        let started = Instant::now();
        connection.progress_handler(1000, Some(move || started.elapsed() >= budget));
        let result = self.check_integrity_result(&connection);
        connection.progress_handler(0, None::<fn() -> bool>);
        result.map(|()| true)
    }

    pub(super) fn check_integrity_result(
        &self,
        connection: &Connection,
    ) -> Result<(), TaskStoreError> {
        let result =
            connection.query_row::<String, _, _>("PRAGMA quick_check(1)", [], |row| row.get(0));
        match result {
            Ok(result) if result == "ok" => Ok(()),
            Ok(_) => {
                self.recovery_required.store(true, Ordering::Release);
                Err(TaskStoreError::DatabaseRecoveryRequired)
            }
            Err(error)
                if matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
                ) =>
            {
                self.recovery_required.store(true, Ordering::Release);
                Err(TaskStoreError::DatabaseRecoveryRequired)
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_damage_blocks_every_clone_until_a_healthy_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let other = store.clone();
        let task = store.create_task("Retain this task", "/fixture").unwrap();
        store.connection().unwrap().execute_batch("CREATE TABLE integrity_fixture(value INTEGER CHECK(value>0)); PRAGMA ignore_check_constraints=ON; INSERT INTO integrity_fixture VALUES(-1); PRAGMA ignore_check_constraints=OFF;").unwrap();
        assert!(matches!(
            store.verify_integrity(),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
        assert!(other.database_recovery_required());
        assert!(matches!(
            other.create_task("Must not write", "/fixture"),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
        assert!(matches!(
            other.get_task(task.id),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
        assert!(matches!(
            other.probe_database_integrity(),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
        // Explicit fixture repair is not a product recovery endpoint. Repairing
        // bytes does not release the old process's latch; a verified reopen does.
        Connection::open(&path)
            .unwrap()
            .execute("DELETE FROM integrity_fixture", [])
            .unwrap();
        assert!(matches!(
            store.verify_integrity(),
            Err(TaskStoreError::DatabaseRecoveryRequired)
        ));
        let reopened = TaskStore::open(&path).unwrap();
        assert!(!reopened.database_recovery_required());
        assert_eq!(
            reopened.get_task(task.id).unwrap().title,
            "Retain this task"
        );
        assert!(reopened.probe_database_integrity().unwrap());
    }

    #[test]
    fn busy_or_interrupted_probe_is_not_corruption_and_leaves_no_progress_hook() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        assert!(!store.probe_database_integrity().unwrap());
        drop(connection);
        assert!(store.probe_with_budget(Duration::ZERO).is_err());
        assert!(!store.database_recovery_required());
        assert!(store.probe_database_integrity().unwrap());
        store.create_task("Still writable", "/fixture").unwrap();
    }
}
