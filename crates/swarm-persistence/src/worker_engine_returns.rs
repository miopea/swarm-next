//! Exact source identities for bounded, existing worker return promises.
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, WorkerId, WorkerReturnAttention, WorkerRevivalAttemptId, WorkerSessionId,
};

use crate::{TaskStore, TaskStoreError};

pub(super) const MAX_RETURN_INTENTS: usize = 256;

pub(super) fn migrate_attempts(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_revival_attempts (
        worker_id TEXT PRIMARY KEY REFERENCES worker_revival_intents(worker_id) ON DELETE CASCADE,
        attempt_id TEXT NOT NULL UNIQUE CHECK(length(attempt_id)=36),
        state TEXT NOT NULL CHECK(state IN ('started','failed','unconfirmed')),
        attempted_at INTEGER NOT NULL
    );",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::WORKER_REVIVAL_ATTEMPTS_SCHEMA_VERSION,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct WorkerEngineReturnSession {
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub recorded_at: i64,
}

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    // No inferred backfill: legacy promises do not prove a source session.
    // Source IDs deliberately outlive worker-session history retention; the
    // parent promise (not age) owns removal of this bounded evidence.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_engine_return_sessions (
        worker_id TEXT PRIMARY KEY REFERENCES worker_revival_intents(worker_id) ON DELETE CASCADE,
        session_id TEXT NOT NULL UNIQUE CHECK(length(session_id)=36),
        recorded_at INTEGER NOT NULL
    );",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::WORKER_ENGINE_RETURN_SESSIONS_SCHEMA_VERSION,
    )
}

pub(super) fn record_intent(
    tx: &Transaction<'_>,
    worker_id: WorkerId,
    now: i64,
) -> Result<(), TaskStoreError> {
    tx.execute(
        "INSERT INTO worker_revival_intents(worker_id,recorded_at) VALUES (?1,?2)
        ON CONFLICT(worker_id) DO UPDATE SET recorded_at=excluded.recorded_at",
        params![worker_id.to_string(), now],
    )?;
    // Existing explicit maintenance callers also retain their actual source.
    // An asleep worker's legacy promise remains unconfirmed, not manufactured.
    tx.execute("INSERT INTO worker_engine_return_sessions(worker_id,session_id,recorded_at)
        SELECT worker_id,session_id,?2 FROM worker_sessions WHERE worker_id=?1 AND ended_at IS NULL
        ON CONFLICT(worker_id) DO UPDATE SET session_id=excluded.session_id,recorded_at=excluded.recorded_at
        WHERE worker_engine_return_sessions.session_id<>excluded.session_id",
        params![worker_id.to_string(), now])?;
    Ok(())
}

pub(super) fn check_capacity(tx: &Transaction<'_>) -> Result<(), TaskStoreError> {
    let pending: usize =
        tx.query_row("SELECT COUNT(*) FROM worker_revival_intents", [], |row| {
            row.get(0)
        })?;
    if pending > MAX_RETURN_INTENTS {
        return Err(TaskStoreError::WorkerReturnPreparationRefused(
            "worker return queue is full; maintenance must wait",
        ));
    }
    Ok(())
}

impl TaskStore {
    /// Claims one queued return before contacting a provider. The exact token
    /// fences late replies, and an interrupted claim is never silently retried.
    /// # Errors
    /// Returns persistence failures without launching anything.
    pub fn claim_worker_revival_attempt(
        &self,
        worker: WorkerId,
        now: i64,
    ) -> Result<Option<WorkerRevivalAttemptId>, TaskStoreError> {
        let id = WorkerRevivalAttemptId::new();
        let changed = self.connection()?.execute(
            "INSERT INTO worker_revival_attempts(worker_id,attempt_id,state,attempted_at)
            SELECT worker_id,?2,'started',?3 FROM worker_revival_intents WHERE worker_id=?1
            ON CONFLICT(worker_id) DO NOTHING",
            params![worker.to_string(), id.to_string(), now],
        )?;
        Ok((changed == 1).then_some(id))
    }

    /// A confirmed result settles only its own claim, never a newer promise.
    /// # Errors
    /// Returns persistence failures; a missing/stale claim is a harmless no-op.
    pub fn complete_worker_revival_attempt(
        &self,
        worker: WorkerId,
        attempt: WorkerRevivalAttemptId,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute("DELETE FROM worker_revival_intents WHERE worker_id=?1
            AND EXISTS(SELECT 1 FROM worker_revival_attempts a WHERE a.worker_id=?1 AND a.attempt_id=?2)",
            params![worker.to_string(),attempt.to_string()])?;
        if changed != 0 {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Keep the obligation and its source after failure, outside the retry queue.
    /// # Errors
    /// Returns persistence failures without replacing a newer attempt's result.
    pub fn fail_worker_revival_attempt(
        &self,
        worker: WorkerId,
        attempt: WorkerRevivalAttemptId,
    ) -> Result<bool, TaskStoreError> {
        self.record_worker_return_attention(worker, attempt, WorkerReturnAttention::Failed)
    }

    /// An ambiguous launch result must not be presented as a confirmed failure.
    /// # Errors
    /// Returns persistence failures without changing another attempt's outcome.
    pub fn record_worker_return_attention(
        &self,
        worker: WorkerId,
        attempt: WorkerRevivalAttemptId,
        attention: WorkerReturnAttention,
    ) -> Result<bool, TaskStoreError> {
        let status = match attention {
            WorkerReturnAttention::Failed => "failed",
            WorkerReturnAttention::Unconfirmed => "unconfirmed",
        };
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE worker_revival_attempts SET state=?3
            WHERE worker_id=?1 AND attempt_id=?2 AND state<>?3",
            params![worker.to_string(), attempt.to_string(), status],
        )?;
        if changed != 0 {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        }
        tx.commit()?;
        Ok(changed != 0)
    }

    /// Caller owns the worker lifecycle and has proved no start is still active.
    /// A missing reply is uncertainty, not evidence of a failed provider launch.
    /// # Errors
    /// Returns persistence failures; no launch or replay is performed.
    pub fn recover_interrupted_worker_returns(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE worker_revival_attempts SET state='unconfirmed' WHERE state='started'",
            [],
        )?;
        if changed != 0 {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Content-free durable attention, bounded by the parent return queue.
    /// # Errors
    /// Returns persistence or invalid-identity failures rather than hiding rows.
    pub fn worker_return_attention(
        &self,
    ) -> Result<HashMap<WorkerId, WorkerReturnAttention>, TaskStoreError> {
        let connection = self.connection()?;
        let mut query = connection.prepare(
            "SELECT worker_id,state FROM worker_revival_attempts
            WHERE state IN ('failed','unconfirmed') ORDER BY attempted_at,worker_id LIMIT 256",
        )?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(worker, state)| {
                Ok((
                    WorkerId::from_str(&worker).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    if state == "failed" {
                        WorkerReturnAttention::Failed
                    } else {
                        WorkerReturnAttention::Unconfirmed
                    },
                ))
            })
            .collect()
    }

    /// An explicit successful open/retry acknowledges the current live worker.
    /// Queued maintenance promises are not cancelled by merely opening a worker.
    /// # Errors
    /// Returns persistence failures without launching or stopping a session.
    pub fn clear_worker_return_attention(&self, worker: WorkerId) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let changed = tx.execute("DELETE FROM worker_revival_intents WHERE worker_id=?1 AND EXISTS(
            SELECT 1 FROM worker_revival_attempts a WHERE a.worker_id=?1 AND a.state IN ('failed','unconfirmed'))",[worker.to_string()])?;
        if changed != 0 {
            crate::insert_control_room_event(&tx, ControlRoomEventKind::WorkersChanged)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Records the complete engine-observed running set in one transaction.
    /// Unknown/unbound, ended or duplicate sessions reject the entire request.
    /// This is durable return evidence, never permission to stop the engine.
    /// # Errors
    /// Refuses invalid membership, exhausted capacity or unavailable persistence.
    pub fn record_worker_engine_return_sessions(
        &self,
        sessions: &[WorkerSessionId],
        now: i64,
    ) -> Result<Vec<WorkerEngineReturnSession>, TaskStoreError> {
        if sessions.len() > MAX_RETURN_INTENTS
            || sessions.iter().copied().collect::<HashSet<_>>().len() != sessions.len()
        {
            return Err(TaskStoreError::WorkerReturnPreparationRefused(
                "return set is oversized or contains duplicate sessions",
            ));
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let mut workers = Vec::with_capacity(sessions.len());
        for session in sessions {
            let id: Option<String> = tx
                .query_row(
                    "SELECT s.worker_id FROM worker_sessions s
                JOIN worker_profiles p ON p.id=s.worker_id
                WHERE s.session_id=?1 AND s.ended_at IS NULL AND p.archived_at IS NULL",
                    [session.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            let id = id.ok_or(TaskStoreError::WorkerReturnPreparationRefused(
                "a running engine session has no matching active worker binding",
            ))?;
            workers.push(WorkerId::from_str(&id).map_err(|_| rusqlite::Error::InvalidQuery)?);
        }
        for worker in &workers {
            record_intent(&tx, *worker, now)?;
        }
        check_capacity(&tx)?;
        let mut result = Vec::with_capacity(sessions.len());
        for (worker, session) in workers.into_iter().zip(sessions) {
            let recorded_at = tx.query_row(
                "SELECT recorded_at FROM worker_engine_return_sessions
                WHERE worker_id=?1 AND session_id=?2",
                params![worker.to_string(), session.to_string()],
                |row| row.get(0),
            )?;
            result.push(WorkerEngineReturnSession {
                worker_id: worker,
                session_id: *session,
                recorded_at,
            });
        }
        tx.commit()?;
        Ok(result)
    }

    /// Exact retained sources only; migrated legacy promises remain unknown.
    /// # Errors
    /// Returns an error for corrupt identities or unavailable persistence.
    pub fn worker_engine_return_sessions(
        &self,
    ) -> Result<Vec<WorkerEngineReturnSession>, TaskStoreError> {
        let connection = self.connection()?;
        let mut query = connection.prepare(
            "SELECT worker_id,session_id,recorded_at
            FROM worker_engine_return_sessions ORDER BY recorded_at,worker_id LIMIT 256",
        )?;
        let rows = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(worker, session, recorded_at)| {
                Ok(WorkerEngineReturnSession {
                    worker_id: WorkerId::from_str(&worker)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: WorkerSessionId::from_str(&session)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    recorded_at,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn worker(store: &TaskStore, name: &str) -> (WorkerId, WorkerSessionId) {
        let worker = store
            .create_worker(name, ProviderKind::ClaudeCode, "/fictional", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        (worker.id, session)
    }

    #[test]
    fn failed_return_survives_reopen_without_becoming_an_automatic_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("returns.sqlite");
        let (id, source) = {
            let store = TaskStore::open(&path).unwrap();
            let (id, source) = worker(&store, "Failed return");
            store.record_worker_revival_intents(&[id], 1).unwrap();
            store.release_worker_session(source).unwrap();
            let attempt = store.claim_worker_revival_attempt(id, 2).unwrap().unwrap();
            assert!(store.claim_worker_revival_attempt(id, 3).unwrap().is_none());
            assert!(!store.worker_revival_pending(id).unwrap());
            assert!(store.fail_worker_revival_attempt(id, attempt).unwrap());
            assert!(!store.fail_worker_revival_attempt(id, attempt).unwrap());
            (id, source)
        };
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store.worker_return_attention().unwrap().get(&id),
            Some(&WorkerReturnAttention::Failed)
        );
        assert!(store.worker_revival_intents().unwrap().is_empty());
        assert_eq!(
            store.worker_engine_return_sessions().unwrap()[0].session_id,
            source
        );
        assert_eq!(store.recover_interrupted_worker_returns().unwrap(), 0);
        store.clear_worker_return_attention(id).unwrap();
        assert!(store.worker_return_attention().unwrap().is_empty());
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
    }

    #[test]
    fn interrupted_return_requires_ownership_recovery_and_fences_stale_results() {
        let store = TaskStore::in_memory().unwrap();
        let (id, _) = worker(&store, "Interrupted return");
        store.record_worker_revival_intents(&[id], 1).unwrap();
        let old = store.claim_worker_revival_attempt(id, 2).unwrap().unwrap();
        assert!(store.worker_return_attention().unwrap().is_empty());
        assert!(store.worker_revival_intents().unwrap().is_empty());
        assert_eq!(store.recover_interrupted_worker_returns().unwrap(), 1);
        assert_eq!(store.recover_interrupted_worker_returns().unwrap(), 0);
        assert_eq!(
            store.worker_return_attention().unwrap().get(&id),
            Some(&WorkerReturnAttention::Unconfirmed)
        );
        store.record_worker_revival_intents(&[id], 3).unwrap();
        let current = store.claim_worker_revival_attempt(id, 4).unwrap().unwrap();
        assert_ne!(old, current);
        assert!(!store.complete_worker_revival_attempt(id, old).unwrap());
        assert!(!store.fail_worker_revival_attempt(id, old).unwrap());
        assert!(store.complete_worker_revival_attempt(id, current).unwrap());
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
    }

    #[test]
    fn confirmed_binding_atomically_settles_an_unacknowledged_return() {
        let store = TaskStore::in_memory().unwrap();
        let (id, source) = worker(&store, "Confirmed return");
        store.record_worker_revival_intents(&[id], 1).unwrap();
        store.release_worker_session(source).unwrap();
        let attempt = store.claim_worker_revival_attempt(id, 2).unwrap().unwrap();
        let current = WorkerSessionId::new();
        store.bind_worker_session(id, current).unwrap();
        assert_eq!(
            store.get_worker_profile(id).unwrap().active_session_id,
            Some(current)
        );
        assert!(!store.complete_worker_revival_attempt(id, attempt).unwrap());
        assert_eq!(store.recover_interrupted_worker_returns().unwrap(), 0);
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
        assert!(store.worker_return_attention().unwrap().is_empty());
    }

    #[test]
    fn exact_returns_survive_database_reopen_and_stopped_source_until_settlement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.sqlite");
        let (worker, session, recorded) = {
            let store = TaskStore::open(&path).unwrap();
            let (worker, session) = worker(&store, "Return");
            let records = store
                .record_worker_engine_return_sessions(&[session], 100)
                .unwrap();
            assert_eq!(
                records,
                vec![WorkerEngineReturnSession {
                    worker_id: worker,
                    session_id: session,
                    recorded_at: 100
                }]
            );
            assert_eq!(
                store
                    .record_worker_engine_return_sessions(&[session], 200)
                    .unwrap(),
                records
            );
            store.release_worker_session(session).unwrap();
            (worker, session, records)
        };
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(store.worker_engine_return_sessions().unwrap(), recorded);
        assert_eq!(store.worker_revival_intents().unwrap(), vec![worker]);
        assert!(
            store
                .record_worker_engine_return_sessions(&[session], 300)
                .is_err()
        );
        assert_eq!(store.worker_engine_return_sessions().unwrap(), recorded);
        store.clear_worker_revival_intent(worker).unwrap();
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
    }

    #[test]
    fn incomplete_duplicate_and_ended_return_sets_leave_no_partial_promises() {
        let store = TaskStore::in_memory().unwrap();
        let (_, first) = worker(&store, "First");
        let (_, ended) = worker(&store, "Ended");
        store.release_worker_session(ended).unwrap();
        for sessions in [
            vec![first, WorkerSessionId::new()],
            vec![first, first],
            vec![first, ended],
        ] {
            assert!(matches!(
                store.record_worker_engine_return_sessions(&sessions, 1),
                Err(TaskStoreError::WorkerReturnPreparationRefused(_))
            ));
            assert!(store.worker_revival_intents().unwrap().is_empty());
            assert!(store.worker_engine_return_sessions().unwrap().is_empty());
        }
    }

    #[test]
    fn return_source_is_not_replaced_by_a_stale_engine_snapshot() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, old) = worker(&store, "Moved");
        store
            .record_worker_engine_return_sessions(&[old], 1)
            .unwrap();
        store.release_worker_session(old).unwrap();
        let current = WorkerSessionId::new();
        store.bind_worker_session(worker, current).unwrap();
        let expected = store
            .record_worker_engine_return_sessions(&[current], 2)
            .unwrap();
        assert_eq!(expected[0].session_id, current);
        assert!(
            store
                .record_worker_engine_return_sessions(&[old], 3)
                .is_err()
        );
        assert_eq!(store.worker_engine_return_sessions().unwrap(), expected);
    }

    #[test]
    fn explicit_maintenance_records_sources_and_cancellation_cascades() {
        let store = TaskStore::in_memory().unwrap();
        let (first, first_session) = worker(&store, "First");
        let (second, second_session) = worker(&store, "Second");
        store
            .record_worker_revival_intents(&[first, second], 1)
            .unwrap();
        assert_eq!(store.worker_engine_return_sessions().unwrap().len(), 2);
        store.cancel_session_revival(first_session).unwrap();
        assert_eq!(
            store.worker_engine_return_sessions().unwrap()[0].session_id,
            second_session
        );
        store.release_worker_session(second_session).unwrap();
        store.archive_worker_profile(second).unwrap();
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
    }

    #[test]
    fn failed_source_insert_rolls_back_the_parent_promise() {
        let store = TaskStore::in_memory().unwrap();
        let (_, session) = worker(&store, "Failure");
        store.connection().unwrap().execute_batch("CREATE TRIGGER fictional_return_failure
            BEFORE INSERT ON worker_engine_return_sessions BEGIN SELECT RAISE(ABORT,'fictional disk failure'); END;").unwrap();
        assert!(
            store
                .record_worker_engine_return_sessions(&[session], 1)
                .is_err()
        );
        assert!(store.worker_revival_intents().unwrap().is_empty());
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fictional_return_failure")
            .unwrap();
        assert_eq!(
            store
                .record_worker_engine_return_sessions(&[session], 2)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn return_capacity_refuses_atomically_without_evicting_existing_obligations() {
        let store = TaskStore::in_memory().unwrap();
        let sessions = (0..257)
            .map(|index| worker(&store, &format!("Worker {index}")).1)
            .collect::<Vec<_>>();
        assert!(
            store
                .record_worker_engine_return_sessions(&sessions, 1)
                .is_err()
        );
        assert!(store.worker_revival_intents().unwrap().is_empty());
        let existing = store
            .record_worker_engine_return_sessions(&sessions[..255], 2)
            .unwrap();
        assert!(
            store
                .record_worker_engine_return_sessions(&sessions[255..], 3)
                .is_err()
        );
        assert_eq!(store.worker_revival_intents().unwrap().len(), 255);
        assert_eq!(
            store.worker_engine_return_sessions().unwrap().len(),
            existing.len()
        );
    }

    #[test]
    fn schema_156_upgrade_preserves_promises_without_inventing_source_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.sqlite");
        let worker_id = {
            let store = TaskStore::open(&path).unwrap();
            let (worker, _) = worker(&store, "Legacy");
            store.record_worker_revival_intents(&[worker], 1).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE worker_engine_return_sessions; PRAGMA user_version=156;")
                .unwrap();
            worker
        };
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(store.worker_revival_intents().unwrap(), vec![worker_id]);
        assert!(store.worker_engine_return_sessions().unwrap().is_empty());
        assert!(
            store
                .get_worker_profile(worker_id)
                .unwrap()
                .active_session_id
                .is_some()
        );
    }

    #[test]
    fn schema_157_upgrade_keeps_existing_promises_pending_not_failed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("upgrade.sqlite");
        let (id, source) = {
            let store = TaskStore::open(&path).unwrap();
            let (id, source) = worker(&store, "Existing return");
            store.record_worker_revival_intents(&[id], 1).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE worker_revival_attempts; PRAGMA user_version=157;")
                .unwrap();
            (id, source)
        };
        let store = TaskStore::open(&path).unwrap();
        assert!(store.worker_revival_pending(id).unwrap());
        assert_eq!(
            store.worker_engine_return_sessions().unwrap()[0].session_id,
            source
        );
        assert!(store.worker_return_attention().unwrap().is_empty());
    }

    #[test]
    fn return_attention_and_settlement_emit_events_only_for_real_changes() {
        let store = TaskStore::in_memory().unwrap();
        let (id, _) = worker(&store, "Eventful return");
        store.record_worker_revival_intents(&[id], 1).unwrap();
        let attempt = store.claim_worker_revival_attempt(id, 2).unwrap().unwrap();
        let before = store.list_control_room_events(0).unwrap().events.len();
        store.fail_worker_revival_attempt(id, attempt).unwrap();
        store.fail_worker_revival_attempt(id, attempt).unwrap();
        assert_eq!(
            store.list_control_room_events(0).unwrap().events.len(),
            before + 1
        );
        store.clear_worker_return_attention(id).unwrap();
        store.clear_worker_return_attention(id).unwrap();
        assert_eq!(
            store.list_control_room_events(0).unwrap().events.len(),
            before + 2
        );
        store.record_worker_revival_intents(&[id], 3).unwrap();
        let attempt = store.claim_worker_revival_attempt(id, 4).unwrap().unwrap();
        store.complete_worker_revival_attempt(id, attempt).unwrap();
        store.complete_worker_revival_attempt(id, attempt).unwrap();
        assert_eq!(
            store.list_control_room_events(0).unwrap().events.len(),
            before + 3
        );
    }
}
