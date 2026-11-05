//! Whether a waiting worker engine update applies by itself (ADR 0156).
//!
//! On unless the operator turns it off. On, Swarm applies a waiting engine
//! update the moment every worker is resting, stops only those resting
//! sessions, brings their workers back, and says so on the worker engine card.
//! Off, the update waits for the operator's own click, as before.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, WorkerId};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

/// More workers than any Hive runs, so one attempt cannot grow the table.
const MAX_STOPPED_WORKERS: usize = 256;
const MAX_WORKER_NAME_BYTES: usize = 120;

pub(crate) fn migrate(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_engine_auto_update (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
             updated_at INTEGER NOT NULL,
             -- The engine build waiting to be applied, and since when. Kept
             -- here rather than in memory so an API restart does not reset how
             -- long the operator has been waiting.
             waiting_engine TEXT,
             waiting_since INTEGER,
             CHECK ((waiting_engine IS NULL) = (waiting_since IS NULL))
         );
         -- Who an engine update stopped, so the notice can name them and say
         -- whether each came back. Pruned with the attempts.
         CREATE TABLE IF NOT EXISTS worker_engine_update_workers (
             attempt_id TEXT NOT NULL,
             worker_id TEXT NOT NULL,
             worker_name TEXT NOT NULL,
             PRIMARY KEY (attempt_id, worker_id)
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// The setting and how long an update has waited.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerEngineAutoUpdate {
    pub enabled: bool,
    /// The engine build waiting to be applied and when Swarm first saw it
    /// waiting. `None` while nothing waits.
    pub waiting: Option<(String, i64)>,
}

fn read(connection: &Connection) -> Result<WorkerEngineAutoUpdate, TaskStoreError> {
    let row = connection
        .query_row(
            "SELECT enabled, waiting_engine, waiting_since
               FROM worker_engine_auto_update WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map_or(
        // Never set: on, which is what the operator ruled.
        WorkerEngineAutoUpdate {
            enabled: true,
            waiting: None,
        },
        |(enabled, engine, since)| WorkerEngineAutoUpdate {
            enabled,
            waiting: engine.zip(since),
        },
    ))
}

impl TaskStore {
    /// Whether a waiting engine update applies by itself, and how long one
    /// has waited.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn worker_engine_auto_update(&self) -> Result<WorkerEngineAutoUpdate, TaskStoreError> {
        let connection = self.connection()?;
        read(&connection)
    }

    /// The operator turning automatic engine updates on or off. Takes effect
    /// on the supervisor's next pass; nothing restarts.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn set_worker_engine_auto_update(
        &self,
        enabled_now: bool,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if read(&transaction)?.enabled != enabled_now {
            transaction.execute(
                "INSERT INTO worker_engine_auto_update (singleton, enabled, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     enabled = excluded.enabled, updated_at = excluded.updated_at",
                params![enabled_now, now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        let stored = read(&transaction)?.enabled;
        transaction.commit()?;
        Ok(stored)
    }

    /// Records which engine build is waiting, or that none is, and returns
    /// since when the current one has waited. Writes only when that changes,
    /// so the supervisor can call it on every pass.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn note_worker_engine_waiting(
        &self,
        engine: Option<&str>,
        now: i64,
    ) -> Result<Option<i64>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = read(&transaction)?;
        let since = match (engine, current.waiting) {
            (Some(engine), Some((waiting, since))) if waiting == engine => Some(since),
            (None, None) => None,
            (engine, _) => {
                transaction.execute(
                    "INSERT INTO worker_engine_auto_update
                         (singleton, enabled, updated_at, waiting_engine, waiting_since)
                     VALUES (1, ?1, ?2, ?3, ?4)
                     ON CONFLICT(singleton) DO UPDATE SET
                         waiting_engine = excluded.waiting_engine,
                         waiting_since = excluded.waiting_since",
                    params![current.enabled, now, engine, engine.map(|_| now)],
                )?;
                engine.map(|_| now)
            }
        };
        transaction.commit()?;
        Ok(since)
    }

    /// Records the workers an engine update stopped, against its attempt.
    ///
    /// # Errors
    /// Returns persistence failures, or a refusal past the bound.
    pub fn record_worker_engine_update_workers(
        &self,
        attempt_id: &str,
        workers: &[(WorkerId, String)],
    ) -> Result<(), TaskStoreError> {
        if workers.len() > MAX_STOPPED_WORKERS {
            return Err(TaskStoreError::WorkerReturnPreparationRefused(
                "an engine update stopped more workers than the record holds",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for (worker_id, name) in workers {
            transaction.execute(
                "INSERT OR IGNORE INTO worker_engine_update_workers
                     (attempt_id, worker_id, worker_name) VALUES (?1, ?2, ?3)",
                params![attempt_id, worker_id.to_string(), bounded(name)],
            )?;
        }
        // Kept only while its attempt is.
        transaction.execute(
            "DELETE FROM worker_engine_update_workers WHERE attempt_id NOT IN
                 (SELECT id FROM worker_engine_update_attempts)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// The workers an engine update stopped, by name.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn worker_engine_update_workers(
        &self,
        attempt_id: &str,
    ) -> Result<Vec<(WorkerId, String)>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT worker_id, worker_name FROM worker_engine_update_workers
              WHERE attempt_id = ?1 ORDER BY worker_name, worker_id LIMIT 256",
        )?;
        let rows = statement
            .query_map([attempt_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, name)| id.parse().ok().map(|id| (id, name)))
            .collect())
    }
}

fn bounded(name: &str) -> &str {
    if name.len() <= MAX_WORKER_NAME_BYTES {
        return name;
    }
    let mut cut = MAX_WORKER_NAME_BYTES;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    &name[..cut]
}

#[cfg(test)]
mod tests {
    use crate::{TaskStore, WorkerEngineUpdateInitiator};
    use swarm_domain::WorkerId;

    #[test]
    fn automatic_engine_updates_are_on_until_the_operator_turns_them_off() {
        let store = TaskStore::in_memory().unwrap();
        assert!(store.worker_engine_auto_update().unwrap().enabled);
        assert!(!store.set_worker_engine_auto_update(false, 10).unwrap());
        assert!(!store.worker_engine_auto_update().unwrap().enabled);
        // Noting a wait does not turn it back on.
        store
            .note_worker_engine_waiting(Some("engine-b"), 20)
            .unwrap();
        assert!(!store.worker_engine_auto_update().unwrap().enabled);
        assert!(store.set_worker_engine_auto_update(true, 30).unwrap());
        assert!(store.worker_engine_auto_update().unwrap().enabled);
    }

    #[test]
    fn a_wait_keeps_its_start_until_the_engine_changes_or_arrives() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(store.note_worker_engine_waiting(None, 5).unwrap(), None);
        assert_eq!(
            store.note_worker_engine_waiting(Some("b"), 10).unwrap(),
            Some(10)
        );
        assert_eq!(
            store.note_worker_engine_waiting(Some("b"), 99).unwrap(),
            Some(10)
        );
        assert_eq!(
            store.worker_engine_auto_update().unwrap().waiting,
            Some(("b".to_owned(), 10))
        );
        assert_eq!(
            store.note_worker_engine_waiting(Some("c"), 120).unwrap(),
            Some(120),
            "a newer engine starts its own wait"
        );
        assert_eq!(store.note_worker_engine_waiting(None, 130).unwrap(), None);
        assert_eq!(store.worker_engine_auto_update().unwrap().waiting, None);
    }

    #[test]
    fn the_workers_an_update_stopped_are_kept_with_their_attempt_only() {
        let store = TaskStore::in_memory().unwrap();
        let attempt = store
            .begin_worker_engine_update(
                "1.0.0",
                "1.0.1",
                None,
                Some(2),
                WorkerEngineUpdateInitiator::Automatic,
                100,
            )
            .unwrap();
        let (first, second) = (WorkerId::new(), WorkerId::new());
        store
            .record_worker_engine_update_workers(
                &attempt,
                &[(second, "Pantry".into()), (first, "Ledger".into())],
            )
            .unwrap();
        assert_eq!(
            store.worker_engine_update_workers(&attempt).unwrap(),
            vec![(first, "Ledger".to_owned()), (second, "Pantry".to_owned())]
        );
        let too_many = (0..257)
            .map(|_| (WorkerId::new(), "x".to_owned()))
            .collect::<Vec<_>>();
        assert!(
            store
                .record_worker_engine_update_workers(&attempt, &too_many)
                .is_err()
        );

        // Twenty newer attempts push this one out, and its workers with it.
        for start in 0..20 {
            store
                .begin_worker_engine_update(
                    "1.0.1",
                    "1.0.2",
                    None,
                    Some(0),
                    WorkerEngineUpdateInitiator::Automatic,
                    200 + start,
                )
                .unwrap();
        }
        let newest = store.last_worker_engine_update().unwrap().unwrap().id;
        store
            .record_worker_engine_update_workers(&newest, &[])
            .unwrap();
        assert!(
            store
                .worker_engine_update_workers(&attempt)
                .unwrap()
                .is_empty()
        );
    }
}
