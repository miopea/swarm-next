use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, OperatorId, OperatorPresence, PresenceDeviceClass, PresenceDeviceId,
    PresenceMode, PresenceObservationState, PresenceSource,
};

use super::{
    PRESENCE_LAST_ACTIVE_SCHEMA_VERSION, TaskStore, TaskStoreError, insert_control_room_event,
};

const MAX_PRESENCE_DEVICES: i64 = 16;
const ACTIVE_TTL_SECONDS: i64 = 150;
const INACTIVE_TTL_SECONDS: i64 = 300;
// A locked desktop keeps reporting: measured on the dogfood host, a locked
// browser heartbeats every sixty seconds. A day-long lifetime therefore did not
// keep a locked machine visible, it kept a machine that had been switched off
// looking locked for a day. Both read as away, so the answer was never wrong,
// but the reason given for it was, and a stale row also outranked a fresher one
// from a device still reporting. Five minutes is the existing inactive
// lifetime and five times the observed heartbeat.
const LOCKED_TTL_SECONDS: i64 = INACTIVE_TTL_SECONDS;
const HIDDEN_TTL_SECONDS: i64 = 90;

/// Records when a device was last actually interacted with.
///
/// A forward step rather than an edit to the migration that created the table:
/// every installed database has already passed that one.
pub(super) fn migrate_presence_last_active(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    // A database old enough not to have the table yet reaches this step on its
    // way up, and creates the table complete further along.
    let table_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'operator_presence_devices')",
        [],
        |row| row.get(0),
    )?;
    let exists: bool = table_exists
        && transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('operator_presence_devices') WHERE name = 'last_active_at')",
            [],
            |row| row.get(0),
        )?;
    if table_exists && !exists {
        transaction.execute_batch(
            "ALTER TABLE operator_presence_devices ADD COLUMN last_active_at INTEGER;",
        )?;
        // An existing device has been interacted with at some point; its last
        // observation is the closest honest answer available.
        transaction
            .execute_batch("UPDATE operator_presence_devices SET last_active_at = updated_at;")?;
    }
    transaction.pragma_update(None, "user_version", PRESENCE_LAST_ACTIVE_SCHEMA_VERSION)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresenceMutation {
    pub presence: OperatorPresence,
    pub changed: bool,
}
impl TaskStore {
    /// Returns the effective operator presence from manual policy and unexpired devices.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn operator_presence(&self, now: i64) -> Result<OperatorPresence, TaskStoreError> {
        let connection = self.connection()?;
        operator_presence_from_connection(&connection, now)
    }

    /// Sets or clears the explicit operator presence override.
    ///
    /// # Errors
    /// Returns a persistence error when the preference cannot be saved atomically.
    pub fn set_manual_presence(
        &self,
        mode: Option<PresenceMode>,
        now: i64,
    ) -> Result<PresenceMutation, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let before = operator_presence_from_connection(&transaction, now)?;
        let operator_id = local_operator_id(&transaction)?;
        if mode.is_none() {
            transaction.execute("UPDATE operator_night_watch SET dismissed_occurrence = NULL WHERE operator_id = ?1", [operator_id.to_string()])?;
        }
        transaction.execute(
            "INSERT INTO operator_presence_preferences (operator_id, manual_mode, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(operator_id) DO UPDATE SET manual_mode = excluded.manual_mode,
                 updated_at = excluded.updated_at",
            params![
                operator_id.to_string(),
                mode.map(|value| value.to_string()),
                now
            ],
        )?;
        let after = operator_presence_from_connection(&transaction, now)?;
        let changed = before != after;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::PresenceChanged)?;
        }
        if after.mode != PresenceMode::AtHive {
            super::notifications::enqueue_pending_notifications(&transaction, now)?;
        }
        transaction.commit()?;
        Ok(PresenceMutation {
            presence: after,
            changed,
        })
    }

    /// Records one bounded authenticated device observation and returns effective presence.
    ///
    /// # Errors
    /// Rejects capacity overflow or persistence failures.
    pub fn record_presence_observation(
        &self,
        device_id: PresenceDeviceId,
        device_class: PresenceDeviceClass,
        state: PresenceObservationState,
        now: i64,
    ) -> Result<PresenceMutation, TaskStoreError> {
        self.record_presence_observation_with_return(device_id, device_class, state, false, now)
    }

    /// Records an observation, distinguishing explicit desktop return from a heartbeat.
    ///
    /// # Errors
    /// Rejects capacity overflow or persistence failures atomically.
    pub fn record_presence_observation_with_return(
        &self,
        device_id: PresenceDeviceId,
        device_class: PresenceDeviceClass,
        state: PresenceObservationState,
        desktop_return: bool,
        now: i64,
    ) -> Result<PresenceMutation, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let before = operator_presence_from_connection(&transaction, now)?;
        transaction.execute(
            "DELETE FROM operator_presence_devices WHERE expires_at <= ?1",
            [now],
        )?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM operator_presence_devices WHERE id = ?1",
                [device_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            let count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM operator_presence_devices",
                [],
                |row| row.get(0),
            )?;
            if count >= MAX_PRESENCE_DEVICES {
                return Err(TaskStoreError::PresenceDeviceLimit);
            }
        }
        let operator_id = local_operator_id(&transaction)?;
        if desktop_return
            && device_class == PresenceDeviceClass::Desktop
            && state == PresenceObservationState::Active
        {
            super::night_watch::dismiss_current(&transaction, now)?;
            transaction.execute(
                "UPDATE operator_presence_preferences SET manual_mode = NULL, updated_at = ?2
                 WHERE operator_id = ?1 AND manual_mode = 'night_watch'",
                params![operator_id.to_string(), now],
            )?;
        }
        let expires_at = now.saturating_add(observation_ttl(state));
        transaction.execute(
            "INSERT INTO operator_presence_devices (
                 id, operator_id, device_class, state, expires_at, updated_at, last_active_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET operator_id = excluded.operator_id,
                 device_class = excluded.device_class, state = excluded.state,
                 expires_at = excluded.expires_at, updated_at = excluded.updated_at,
                 last_active_at = MAX(
                     COALESCE(operator_presence_devices.last_active_at, 0),
                     COALESCE(excluded.last_active_at, 0)
                 )",
            params![
                device_id.to_string(),
                operator_id.to_string(),
                device_class.to_string(),
                state.to_string(),
                expires_at,
                now,
                (state == PresenceObservationState::Active).then_some(now),
            ],
        )?;
        let after = operator_presence_from_connection(&transaction, now)?;
        let changed = before != after;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::PresenceChanged)?;
        }
        if after.mode != PresenceMode::AtHive {
            super::notifications::enqueue_pending_notifications(&transaction, now)?;
        }
        transaction.commit()?;
        Ok(PresenceMutation {
            presence: after,
            changed,
        })
    }
}

fn observation_ttl(state: PresenceObservationState) -> i64 {
    match state {
        PresenceObservationState::Active => ACTIVE_TTL_SECONDS,
        PresenceObservationState::Idle => INACTIVE_TTL_SECONDS,
        PresenceObservationState::Locked => LOCKED_TTL_SECONDS,
        PresenceObservationState::Hidden => HIDDEN_TTL_SECONDS,
    }
}

pub(super) fn operator_presence_from_connection(
    connection: &Connection,
    now: i64,
) -> Result<OperatorPresence, TaskStoreError> {
    let operator_id = local_operator_id(connection)?;
    let manual_mode = connection
        .query_row(
            "SELECT manual_mode FROM operator_presence_preferences WHERE operator_id = ?1",
            [operator_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .map(|value| PresenceMode::from_str(&value))
        .transpose()
        .map_err(|_| TaskStoreError::IntegrityFailure("invalid operator presence mode".into()))?;
    if let Some(mode) = manual_mode {
        return Ok(OperatorPresence {
            mode,
            manual_mode: Some(mode),
            source: PresenceSource::Manual,
        });
    }
    if super::night_watch::scheduled_active(connection, now)? {
        return Ok(OperatorPresence {
            mode: PresenceMode::NightWatch,
            manual_mode: None,
            source: PresenceSource::Scheduled,
        });
    }
    let state = connection
        .query_row(
            // A device interacted with inside the active window still counts as
            // active. Backgrounding an app reports hidden, which describes the
            // screen rather than the person holding it, and treating the two as
            // the same made presence flip every time an operator changed apps.
            // The window is the existing active lifetime rather than a new
            // number: an active observation is already treated as meaningful
            // for exactly that long.
            "SELECT CASE
                 WHEN state = 'hidden' AND last_active_at IS NOT NULL
                      AND last_active_at + ?3 > ?2 THEN 'active'
                 ELSE state
             END AS state
             FROM operator_presence_devices
             WHERE operator_id = ?1 AND expires_at > ?2
             ORDER BY CASE state
                 WHEN 'active' THEN 0 WHEN 'locked' THEN 1
                 WHEN 'idle' THEN 2 ELSE 3 END,
                 updated_at DESC LIMIT 1",
            params![operator_id.to_string(), now, ACTIVE_TTL_SECONDS],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| PresenceObservationState::from_str(&value))
        .transpose()
        .map_err(|_| TaskStoreError::IntegrityFailure("invalid device presence state".into()))?;
    Ok(match state {
        Some(PresenceObservationState::Active) => OperatorPresence {
            mode: PresenceMode::AtHive,
            manual_mode: None,
            source: PresenceSource::ActiveDevice,
        },
        Some(PresenceObservationState::Locked) => OperatorPresence {
            mode: PresenceMode::Away,
            manual_mode: None,
            source: PresenceSource::ScreenLocked,
        },
        Some(PresenceObservationState::Idle | PresenceObservationState::Hidden) => {
            OperatorPresence {
                mode: PresenceMode::Away,
                manual_mode: None,
                source: PresenceSource::InactiveDevice,
            }
        }
        None => OperatorPresence {
            mode: PresenceMode::Away,
            manual_mode: None,
            source: PresenceSource::TimedOut,
        },
    })
}

pub(super) fn local_operator_id(connection: &Connection) -> Result<OperatorId, TaskStoreError> {
    let value: String = connection.query_row(
        "SELECT h.operator_id FROM local_hive_identity local
         JOIN hives h ON h.id = local.hive_id WHERE local.singleton = 1",
        [],
        |row| row.get(0),
    )?;
    OperatorId::from_str(&value)
        .map_err(|_| TaskStoreError::IntegrityFailure("invalid local operator identity".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_lock_or_idle_overrides_the_same_devices_recent_activity() {
        for state in [
            PresenceObservationState::Locked,
            PresenceObservationState::Idle,
        ] {
            let store = TaskStore::in_memory().unwrap();
            let device = PresenceDeviceId::new();
            store
                .record_presence_observation(
                    device,
                    PresenceDeviceClass::Desktop,
                    PresenceObservationState::Active,
                    1_000,
                )
                .unwrap();
            let away = store
                .record_presence_observation(device, PresenceDeviceClass::Desktop, state, 1_001)
                .unwrap();
            assert_eq!(away.presence.mode, PresenceMode::Away);
            assert!(away.changed);
            let back = store
                .record_presence_observation(
                    device,
                    PresenceDeviceClass::Desktop,
                    PresenceObservationState::Active,
                    1_002,
                )
                .unwrap();
            assert_eq!(back.presence.mode, PresenceMode::AtHive);
            assert!(back.changed);
        }
    }

    #[test]
    fn a_machine_that_stopped_reporting_stops_being_called_locked() {
        // A locked desktop heartbeats, so silence means the machine is gone
        // rather than locked. Swarm should stop claiming to know which.
        let store = TaskStore::in_memory().unwrap();
        let desktop = PresenceDeviceId::new();

        let locked = store
            .record_presence_observation(
                desktop,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Locked,
                1_000,
            )
            .unwrap();
        assert_eq!(locked.presence.mode, PresenceMode::Away);
        assert_eq!(locked.presence.source, PresenceSource::ScreenLocked);

        // Still within the lifetime: the report stands.
        assert_eq!(
            store
                .operator_presence(1_000 + LOCKED_TTL_SECONDS - 1)
                .unwrap()
                .source,
            PresenceSource::ScreenLocked
        );

        // Past it, Swarm says it no longer knows rather than repeating a claim
        // about a machine it has not heard from.
        let stale = store
            .operator_presence(1_000 + LOCKED_TTL_SECONDS + 1)
            .unwrap();
        assert_eq!(stale.mode, PresenceMode::Away);
        assert_eq!(stale.source, PresenceSource::TimedOut);
    }

    #[test]
    fn switching_apps_on_a_phone_does_not_read_as_leaving() {
        // The operator's own account: "if I am on my phone and it is active, it
        // is perfectly normal to jump between apps, but I never left my phone."
        // A backgrounded app reports hidden, which is a statement about the
        // screen rather than about the person holding it. Measured on the
        // dogfood host, a phone in use flipped active/hidden four times in
        // ninety seconds, which flipped derived presence just as often.
        let store = TaskStore::in_memory().unwrap();
        let phone = PresenceDeviceId::new();

        let active = store
            .record_presence_observation(
                phone,
                PresenceDeviceClass::Mobile,
                PresenceObservationState::Active,
                1_000,
            )
            .unwrap();
        assert_eq!(active.presence.mode, PresenceMode::AtHive);

        // Switching apps a few seconds later.
        let hidden = store
            .record_presence_observation(
                phone,
                PresenceDeviceClass::Mobile,
                PresenceObservationState::Hidden,
                1_010,
            )
            .unwrap();
        assert_eq!(
            hidden.presence.mode,
            PresenceMode::AtHive,
            "a device active moments ago is still the operator's device"
        );
        assert!(
            !hidden.changed,
            "presence did not change, so nothing downstream should be told it did"
        );

        // Long enough with no interaction and it is a real absence again.
        let gone = store
            .record_presence_observation(
                phone,
                PresenceDeviceClass::Mobile,
                PresenceObservationState::Hidden,
                1_000 + ACTIVE_TTL_SECONDS + 1,
            )
            .unwrap();
        assert_eq!(gone.presence.mode, PresenceMode::Away);
    }

    #[test]
    fn device_observations_derive_presence_without_timer_owned_truth() {
        let store = TaskStore::in_memory().unwrap();
        let desktop = PresenceDeviceId::new();
        let mobile = PresenceDeviceId::new();
        assert_eq!(
            store.operator_presence(100).unwrap().mode,
            PresenceMode::Away
        );

        let active = store
            .record_presence_observation(
                desktop,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                100,
            )
            .unwrap();
        assert_eq!(active.presence.mode, PresenceMode::AtHive);
        assert_eq!(active.presence.source, PresenceSource::ActiveDevice);

        store
            .record_presence_observation(
                mobile,
                PresenceDeviceClass::Mobile,
                PresenceObservationState::Locked,
                120,
            )
            .unwrap();
        assert_eq!(
            store.operator_presence(121).unwrap().mode,
            PresenceMode::AtHive
        );
        let away = store.operator_presence(251).unwrap();
        assert_eq!(away.mode, PresenceMode::Away);
        assert_eq!(away.source, PresenceSource::ScreenLocked);
    }

    #[test]
    fn manual_night_watch_overrides_devices_and_can_return_to_auto() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .record_presence_observation(
                device,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                100,
            )
            .unwrap();
        let night = store
            .set_manual_presence(Some(PresenceMode::NightWatch), 101)
            .unwrap();
        assert_eq!(night.presence.mode, PresenceMode::NightWatch);
        assert_eq!(night.presence.source, PresenceSource::Manual);
        let automatic = store.set_manual_presence(None, 102).unwrap();
        assert_eq!(automatic.presence.mode, PresenceMode::AtHive);
        assert_eq!(automatic.presence.manual_mode, None);
    }

    #[test]
    fn repeated_heartbeat_does_not_emit_presence_churn() {
        let store = TaskStore::in_memory().unwrap();
        let device = PresenceDeviceId::new();
        store
            .record_presence_observation(
                device,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                100,
            )
            .unwrap();
        let cursor = store.list_control_room_events(0).unwrap().next_cursor;
        store
            .record_presence_observation(
                device,
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                120,
            )
            .unwrap();
        assert!(
            store
                .list_control_room_events(cursor)
                .unwrap()
                .events
                .is_empty()
        );
    }

    #[test]
    fn active_device_capacity_is_bounded_and_expired_rows_are_reclaimed() {
        let store = TaskStore::in_memory().unwrap();
        for _ in 0..MAX_PRESENCE_DEVICES {
            store
                .record_presence_observation(
                    PresenceDeviceId::new(),
                    PresenceDeviceClass::Mobile,
                    PresenceObservationState::Locked,
                    100,
                )
                .unwrap();
        }
        assert!(matches!(
            store.record_presence_observation(
                PresenceDeviceId::new(),
                PresenceDeviceClass::Mobile,
                PresenceObservationState::Locked,
                101,
            ),
            Err(TaskStoreError::PresenceDeviceLimit)
        ));
        store
            .record_presence_observation(
                PresenceDeviceId::new(),
                PresenceDeviceClass::Desktop,
                PresenceObservationState::Active,
                86_501,
            )
            .unwrap();
    }
}
