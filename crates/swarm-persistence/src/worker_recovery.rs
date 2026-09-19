//! The automatic recovery circuit, kept where a restart cannot forget it.
//!
//! ⚠️ WHY THIS TABLE EXISTS AT ALL. The circuit was correct inside one process
//! and meaningless across two. `worker_recovery_attempts` and `worker_errors`
//! were in-memory maps built empty at startup with nothing to rehydrate them,
//! so every API restart handed each worker a fresh "one safe attempt" and
//! erased the failure the operator had been shown. The comment beside the
//! circuit says it exists to stop an unstartable worker being restarted
//! forever; across restarts it did not, and AGENTS.md requires every retry
//! policy to be bounded.
//!
//! ⚠️ AND A NEW BUILD DELIBERATELY EARNS ONE FRESH ATTEMPT. Operator instruction
//! in session, 2026-09-18 — NOT decision 01a0b749-5def, which was filed for this
//! question and dismissed as already handled, so it records no answer. See
//! ADR 0103. A restart on the SAME build tells you nothing new
//! about whether a worker can start, so it must not buy another try. Shipping
//! DIFFERENT code is the one event that plausibly changes the answer, so it
//! buys exactly one. The alternative — an absolute bound — means shipping the
//! very fix that would revive a worker leaves it down anyway, which is the
//! silent permanent stall this file's neighbours already reject by name.

use rusqlite::{Connection, OptionalExtension as _, Transaction, params};

use crate::TaskStoreError;

/// One worker's standing with the recovery circuit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRecoveryCircuit {
    pub worker_id: String,
    /// When the one automatic attempt was spent.
    pub attempted_at: i64,
    /// The failure shown to the operator once the circuit opened, if it has.
    pub failure: Option<String>,
}

pub(super) fn migrate_worker_recovery_circuit(
    transaction: &Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_recovery_circuit (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             attempted_at INTEGER NOT NULL,
             build_revision TEXT NOT NULL,
             failure TEXT,
             opened_at INTEGER
         );",
    )?;
    // NO BACKFILL, deliberately. An empty table says every worker is in good
    // standing, which is exactly true of a fleet whose circuit state has only
    // ever lived in the memory of a process that has since exited.
    transaction.pragma_update(
        None,
        "user_version",
        crate::WORKER_RECOVERY_CIRCUIT_SCHEMA_VERSION,
    )?;
    Ok(())
}

/// Every worker still holding a recovery record for THIS build.
///
/// Records written by a different build are deleted as they are read, which is
/// what grants the one fresh attempt. Doing it here rather than on write means a
/// build that never runs a supervisor pass cannot leave a stale grant behind.
pub(crate) fn reconcile_worker_recovery_circuits(
    connection: &Connection,
    build_revision: &str,
) -> Result<Vec<WorkerRecoveryCircuit>, TaskStoreError> {
    connection.execute(
        "DELETE FROM worker_recovery_circuit WHERE build_revision <> ?1",
        params![build_revision],
    )?;
    let mut statement = connection
        .prepare("SELECT worker_id, attempted_at, failure FROM worker_recovery_circuit")?;
    let rows = statement
        .query_map([], |row| {
            Ok(WorkerRecoveryCircuit {
                worker_id: row.get(0)?,
                attempted_at: row.get(1)?,
                failure: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Spends the worker's one attempt, returning the moment a previous one was
/// spent on this same build if there was one.
///
/// The previous time is RETURNED RATHER THAN OVERWRITTEN BLINDLY: the caller
/// decides whether the worker is still coming up, and puts the original time
/// back if it is, because a stability window that restarts on every pass never
/// matures.
pub(crate) fn record_worker_recovery_attempt(
    connection: &Connection,
    worker_id: &str,
    attempted_at: i64,
    build_revision: &str,
) -> Result<Option<i64>, TaskStoreError> {
    let previous: Option<i64> = connection
        .query_row(
            "SELECT attempted_at FROM worker_recovery_circuit WHERE worker_id = ?1",
            params![worker_id],
            |row| row.get(0),
        )
        .optional()?;
    connection.execute(
        "INSERT INTO worker_recovery_circuit (worker_id, attempted_at, build_revision)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(worker_id) DO UPDATE SET
             attempted_at = excluded.attempted_at,
             build_revision = excluded.build_revision",
        params![worker_id, attempted_at, build_revision],
    )?;
    Ok(previous)
}

/// Puts an earlier attempt time back, for a worker judged to be still coming up.
pub(crate) fn restore_worker_recovery_attempt(
    connection: &Connection,
    worker_id: &str,
    attempted_at: i64,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "UPDATE worker_recovery_circuit SET attempted_at = ?2 WHERE worker_id = ?1",
        params![worker_id, attempted_at],
    )?;
    Ok(())
}

/// Opens the circuit and records what the operator should be told.
pub(crate) fn open_worker_recovery_circuit(
    connection: &Connection,
    worker_id: &str,
    opened_at: i64,
    failure: &str,
    build_revision: &str,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "INSERT INTO worker_recovery_circuit
             (worker_id, attempted_at, build_revision, failure, opened_at)
         VALUES (?1, ?3, ?4, ?2, ?3)
         ON CONFLICT(worker_id) DO UPDATE SET
             failure = excluded.failure,
             opened_at = excluded.opened_at,
             build_revision = excluded.build_revision",
        params![worker_id, failure, opened_at, build_revision],
    )?;
    Ok(())
}

/// Clears the recorded failure while LEAVING THE SPENT ATTEMPT IN PLACE.
///
/// ⚠️ THE ATTEMPT MUST SURVIVE THIS. The stability window is the only thing
/// stopping a worker that starts and dies a second later from earning attempt
/// after attempt; forgetting the attempt whenever a failure is cleared would
/// hand exactly that back, which is the unbounded retry the circuit exists to
/// prevent.
pub(crate) fn clear_worker_recovery_failure(
    connection: &Connection,
    worker_id: &str,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "UPDATE worker_recovery_circuit SET failure = NULL, opened_at = NULL
         WHERE worker_id = ?1",
        params![worker_id],
    )?;
    Ok(())
}

/// Forgets a worker's standing entirely — it started, or the operator cleared it.
pub(crate) fn clear_worker_recovery_circuit(
    connection: &Connection,
    worker_id: &str,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "DELETE FROM worker_recovery_circuit WHERE worker_id = ?1",
        params![worker_id],
    )?;
    Ok(())
}

impl crate::TaskStore {
    /// Every worker still held by the recovery circuit on THIS build.
    ///
    /// Reading reconciles: records from a different build are dropped, which is
    /// how a new build grants its one fresh attempt.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn reconcile_worker_recovery_circuits(
        &self,
        build_revision: &str,
    ) -> Result<Vec<WorkerRecoveryCircuit>, TaskStoreError> {
        let connection = self.connection()?;
        reconcile_worker_recovery_circuits(&connection, build_revision)
    }

    /// Spends the worker's one attempt; returns when a previous one was spent.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn record_worker_recovery_attempt(
        &self,
        worker_id: swarm_domain::WorkerId,
        attempted_at: i64,
        build_revision: &str,
    ) -> Result<Option<i64>, TaskStoreError> {
        let connection = self.connection()?;
        record_worker_recovery_attempt(
            &connection,
            &worker_id.to_string(),
            attempted_at,
            build_revision,
        )
    }

    /// Puts an earlier attempt time back for a worker that is still coming up.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn restore_worker_recovery_attempt(
        &self,
        worker_id: swarm_domain::WorkerId,
        attempted_at: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        restore_worker_recovery_attempt(&connection, &worker_id.to_string(), attempted_at)
    }

    /// Opens the circuit and records what the operator should be told.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn open_worker_recovery_circuit(
        &self,
        worker_id: swarm_domain::WorkerId,
        opened_at: i64,
        failure: &str,
        build_revision: &str,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        open_worker_recovery_circuit(
            &connection,
            &worker_id.to_string(),
            opened_at,
            failure,
            build_revision,
        )
    }

    /// Clears a recorded failure, leaving the spent attempt in place.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn clear_worker_recovery_failure(
        &self,
        worker_id: swarm_domain::WorkerId,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        clear_worker_recovery_failure(&connection, &worker_id.to_string())
    }

    /// Forgets a worker's standing with the circuit.
    ///
    /// # Errors
    /// Returns database failures.
    pub fn clear_worker_recovery_circuit(
        &self,
        worker_id: swarm_domain::WorkerId,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        clear_worker_recovery_circuit(&connection, &worker_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::TaskStore;
    use swarm_domain::ProviderKind;

    const BUILD_A: &str = "3b0d267e1f7b";
    const BUILD_B: &str = "8083d90d3aa3";

    fn store_with_worker() -> (TaskStore, swarm_domain::WorkerId) {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Poppy", ProviderKind::ClaudeCode, "/workspace", true, 1)
            .unwrap();
        (store, worker.id)
    }

    /// ⚠️ THE DEFECT THIS TABLE EXISTS FOR. The attempt lived in a `HashMap` built
    /// empty at startup, so restarting the API handed every worker its one safe
    /// attempt back and an unstartable worker could be restarted forever —
    /// exactly what the circuit's own comment says it prevents.
    #[test]
    fn a_spent_attempt_survives_a_restart_on_the_same_build() {
        let (store, worker) = store_with_worker();
        assert_eq!(
            store
                .record_worker_recovery_attempt(worker, 1_000, BUILD_A)
                .unwrap(),
            None,
            "nothing was spent before this"
        );

        // A restart is only ever a fresh read of this table.
        let after_restart = store.reconcile_worker_recovery_circuits(BUILD_A).unwrap();

        assert_eq!(after_restart.len(), 1, "the attempt is still spent");
        assert_eq!(after_restart[0].attempted_at, 1_000);
    }

    /// The operator's in-session answer (see ADR 0103; the decision card filed
    /// for this was dismissed as already handled and records nothing):
    /// shipping DIFFERENT code is the one
    /// event that might change whether a worker can start, so it buys exactly one
    /// attempt. Asserted beside the test above, because a change that satisfied
    /// one by breaking the other would look like a pass.
    #[test]
    fn a_new_build_grants_exactly_one_fresh_attempt() {
        let (store, worker) = store_with_worker();
        store
            .record_worker_recovery_attempt(worker, 1_000, BUILD_A)
            .unwrap();

        let after_deploy = store.reconcile_worker_recovery_circuits(BUILD_B).unwrap();
        assert!(
            after_deploy.is_empty(),
            "a changed build revision returns the attempt: {after_deploy:?}"
        );

        // And exactly one: spending it on the new build leaves nothing over.
        store
            .record_worker_recovery_attempt(worker, 2_000, BUILD_B)
            .unwrap();
        let again = store.reconcile_worker_recovery_circuits(BUILD_B).unwrap();
        assert_eq!(again.len(), 1, "the fresh attempt is spent, not renewed");
        assert_eq!(again[0].attempted_at, 2_000);
    }

    /// ⚠️ CLEARING A FAILURE MUST NOT RETURN THE ATTEMPT. It reads as the
    /// generous thing to do and it reinstates the unbounded retry: a worker that
    /// starts and dies a second later would earn attempt after attempt, because
    /// each start clears the failure.
    #[test]
    fn clearing_a_failure_leaves_the_spent_attempt_in_place() {
        let (store, worker) = store_with_worker();
        store
            .record_worker_recovery_attempt(worker, 1_000, BUILD_A)
            .unwrap();
        store
            .open_worker_recovery_circuit(worker, 1_500, "exited again", BUILD_A)
            .unwrap();

        let opened = store.reconcile_worker_recovery_circuits(BUILD_A).unwrap();
        assert_eq!(opened[0].failure.as_deref(), Some("exited again"));

        store.clear_worker_recovery_failure(worker).unwrap();

        let cleared = store.reconcile_worker_recovery_circuits(BUILD_A).unwrap();
        assert_eq!(cleared.len(), 1, "the attempt is still on record");
        assert_eq!(cleared[0].failure, None, "the failure is gone");
        assert_eq!(cleared[0].attempted_at, 1_000, "and the attempt is not");
    }

    /// The opened circuit is what the operator reads to know why a worker is
    /// down, and it used to be a string in memory that a restart erased.
    #[test]
    fn an_opened_circuit_is_still_readable_after_a_restart() {
        let (store, worker) = store_with_worker();
        store
            .open_worker_recovery_circuit(worker, 1_500, "exited again", BUILD_A)
            .unwrap();

        let after_restart = store.reconcile_worker_recovery_circuits(BUILD_A).unwrap();

        assert_eq!(after_restart.len(), 1);
        assert_eq!(after_restart[0].failure.as_deref(), Some("exited again"));
    }
}
