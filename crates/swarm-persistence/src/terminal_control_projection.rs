//! Bounded, generation-ordered projection of engine ownership for the roster and
//! orchestration guard. This table never authorizes a terminal effect.

use rusqlite::{OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, TerminalControlIdentity, WorkerSessionId};

use crate::{
    TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION, TERMINAL_CONTROL_PROJECTION_SCHEMA_VERSION,
    TaskStore, TaskStoreError, insert_control_room_event,
};

fn create_table(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let worker_profiles_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    if worker_profiles_exists {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS worker_terminal_control (
            worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
            session_id TEXT NOT NULL,
            generation TEXT NOT NULL CHECK(length(generation) = 20),
            owner_device_id TEXT,
            owner_view_id TEXT,
            expires_at INTEGER NOT NULL,
            CHECK((owner_device_id IS NULL) = (owner_view_id IS NULL))
        );",
        )?;
    }
    Ok(())
}

pub(super) fn migrate(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version >= TERMINAL_CONTROL_PROJECTION_SCHEMA_VERSION {
        return Ok(());
    }
    create_table(transaction)?;
    transaction.pragma_update(
        None,
        "user_version",
        TERMINAL_CONTROL_PROJECTION_SCHEMA_VERSION,
    )
}

/// Repair the historical schema-124 identity collision after all combined
/// migrations have run. `CREATE IF NOT EXISTS` makes this safe both for an
/// affected upstream database and for a maturity database that already has the
/// projection.
pub(super) fn repair_version_collision(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version >= TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION {
        return Ok(());
    }
    create_table(transaction)?;
    transaction.pragma_update(
        None,
        "user_version",
        TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalControlProjection {
    pub session_id: WorkerSessionId,
    pub generation: u64,
    pub owner: Option<TerminalControlIdentity>,
    pub lease_remaining_ms: u64,
}

impl TaskStore {
    /// Project an engine observation without allowing delayed replies to restore
    /// an older owner. At most one row exists per worker, even across restarts.
    /// Generation zero has not activated the new contract and changes nothing.
    ///
    /// # Errors
    /// Returns an error if persistence fails or the session is no longer active.
    pub fn project_terminal_control(
        &self,
        value: TerminalControlProjection,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        if value.generation == 0 {
            return Ok(false);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let session = value.session_id.to_string();
        let worker: String = transaction
            .query_row(
                "SELECT worker_id FROM worker_sessions WHERE session_id = ?1 AND ended_at IS NULL",
                [&session],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        let current = transaction
            .query_row(
                "SELECT session_id, generation, owner_device_id, owner_view_id, expires_at
             FROM worker_terminal_control WHERE worker_id = ?1",
                [&worker],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?;
        // Fixed-width decimal order preserves the whole u64 range in SQLite.
        let generation = format!("{:020}", value.generation);
        let device = value.owner.map(|owner| owner.device.to_string());
        let view = value.owner.map(|owner| owner.view.to_string());
        let seconds = i64::try_from(value.lease_remaining_ms.min(300_000) / 1000).unwrap_or(300);
        let expires = if value.owner.is_some() {
            now.saturating_add(seconds)
        } else {
            0
        };
        if let Some((old_session, old_generation, old_device, old_view, old_expiry)) = current
            && old_session == session
        {
            if old_generation > generation {
                return Ok(false);
            }
            if old_generation == generation {
                // Expiry at a generation is final. Only a new engine claim
                // can advance it and make ownership live again.
                if old_device.is_none()
                    || (device.is_some() && (device != old_device || view != old_view))
                {
                    return Ok(false);
                }
                if device.is_some() && old_expiry >= expires.saturating_sub(30) {
                    // Presentation refresh coalescing, never PTY authority.
                    return Ok(false);
                }
            }
        }
        transaction.execute(
            "INSERT INTO worker_terminal_control (worker_id, session_id, generation, owner_device_id, owner_view_id, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(worker_id) DO UPDATE SET session_id = excluded.session_id,
                 generation = excluded.generation, owner_device_id = excluded.owner_device_id,
                 owner_view_id = excluded.owner_view_id, expires_at = excluded.expires_at",
            params![worker, session, generation, device, view, expires],
        )?;
        if device.is_some() {
            transaction.execute(
                "INSERT INTO worker_engagements (worker_id, session_id, engaged_at, renewed_at, expires_at, owner_device_id)
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5)
                 ON CONFLICT(worker_id) DO UPDATE SET session_id = excluded.session_id,
                     renewed_at = excluded.renewed_at, expires_at = excluded.expires_at,
                     owner_device_id = excluded.owner_device_id",
                params![worker, session, now, expires, device],
            )?;
        } else {
            transaction.execute(
                "DELETE FROM worker_engagements WHERE session_id = ?1",
                [&session],
            )?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        transaction.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, TerminalViewId};

    fn fixture() -> (TaskStore, TerminalControlProjection) {
        let store = TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace/queen").unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        (
            store,
            TerminalControlProjection {
                session_id,
                generation: 1,
                owner: Some(TerminalControlIdentity {
                    device: PresenceDeviceId::new(),
                    view: TerminalViewId::new(),
                }),
                lease_remaining_ms: 90_000,
            },
        )
    }

    #[test]
    fn delayed_old_owner_and_expired_owner_cannot_return() {
        let (store, first) = fixture();
        assert!(store.project_terminal_control(first, 100).unwrap());
        let mut second = first;
        second.generation = 2;
        second.owner.as_mut().unwrap().view = TerminalViewId::new();
        assert!(store.project_terminal_control(second, 110).unwrap());
        assert!(!store.project_terminal_control(first, 200).unwrap());
        let mut expired = second;
        expired.owner = None;
        expired.lease_remaining_ms = 0;
        assert!(store.project_terminal_control(expired, 201).unwrap());
        assert!(!store.project_terminal_control(second, 202).unwrap());
        assert!(!store.operator_holds_any_terminal(202).unwrap());
        second.generation = 3;
        assert!(store.project_terminal_control(second, 203).unwrap());
        assert!(store.operator_holds_any_terminal(203).unwrap());
    }

    #[test]
    fn large_generations_and_repeated_updates_are_bounded() {
        let (store, mut value) = fixture();
        value.generation = u64::MAX;
        assert!(store.project_terminal_control(value, 100).unwrap());
        assert!(!store.project_terminal_control(value, 101).unwrap());
        assert!(store.project_terminal_control(value, 140).unwrap());
        value.generation = u64::MAX - 1;
        assert!(!store.project_terminal_control(value, 500).unwrap());
        let rows: u32 = store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM worker_terminal_control", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn legacy_engagement_cannot_replace_or_release_engine_projection() {
        let (store, value) = fixture();
        assert!(store.project_terminal_control(value, 100).unwrap());
        let owner = value.owner.unwrap();
        assert!(
            !store
                .renew_worker_engagement(value.session_id, Some(PresenceDeviceId::new()), 101, 300)
                .unwrap()
        );
        assert!(
            !store
                .release_worker_engagement(value.session_id, owner.device)
                .unwrap()
        );
        assert!(store.operator_holds_any_terminal(102).unwrap());
    }

    #[test]
    fn old_sessions_cannot_overwrite_replacements_and_zero_does_not_activate() {
        let (store, mut value) = fixture();
        let zero = TerminalControlProjection {
            generation: 0,
            ..value
        };
        assert!(!store.project_terminal_control(zero, 100).unwrap());
        assert!(store.project_terminal_control(value, 100).unwrap());
        let worker = store.ensure_queen("/workspace/queen").unwrap();
        let new_session = WorkerSessionId::new();
        store.release_worker_session(value.session_id).unwrap();
        store.bind_worker_session(worker.id, new_session).unwrap();
        assert!(matches!(
            store.project_terminal_control(value, 101),
            Err(TaskStoreError::WorkerSessionNotActive)
        ));
        value.session_id = new_session;
        assert!(store.project_terminal_control(value, 102).unwrap());
    }
}
