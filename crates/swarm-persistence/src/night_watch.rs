//! Persisted operator schedule and IANA clock conversion. Policy stays in domain.
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use swarm_domain::{ControlRoomEventKind, NightWatchSchedule};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NightWatchConfiguration {
    enabled: bool,
    timezone: String,
    start_minute: u16,
    end_minute: u16,
}

impl NightWatchConfiguration {
    /// Invalid zones/windows are refused even when the schedule is disabled.
    #[must_use]
    pub fn new(enabled: bool, timezone: &str, start_minute: u16, end_minute: u16) -> Option<Self> {
        let zone = timezone.parse::<Tz>().ok()?;
        NightWatchSchedule::new(start_minute, end_minute).ok()?;
        Some(Self {
            enabled,
            timezone: zone.name().to_owned(),
            start_minute,
            end_minute,
        })
    }

    /// Converts a UTC instant through the configured IANA zone, not host locale.
    ///
    /// # Errors
    /// Rejects unsupported timestamps or invalid persisted configuration.
    pub fn occurrence(&self, now: i64) -> Result<Option<i64>, TaskStoreError> {
        if !self.enabled {
            return Ok(None);
        }
        let invalid = || {
            TaskStoreError::IntegrityFailure("invalid Night Watch clock or configuration".into())
        };
        let utc = DateTime::<Utc>::from_timestamp(now, 0).ok_or_else(invalid)?;
        let local = utc.with_timezone(&self.timezone.parse::<Tz>().map_err(|_| invalid())?);
        let minute = u16::try_from(local.hour() * 60 + local.minute()).map_err(|_| invalid())?;
        let window =
            NightWatchSchedule::new(self.start_minute, self.end_minute).map_err(|_| invalid())?;
        Ok(window.occurrence(i64::from(local.date_naive().num_days_from_ce()), minute))
    }
}

pub(super) fn migrate(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    if version >= crate::NIGHT_WATCH_SCHEMA_VERSION {
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_night_watch (
            operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
            timezone TEXT NOT NULL CHECK(length(timezone) BETWEEN 1 AND 128),
            start_minute INTEGER NOT NULL CHECK(start_minute BETWEEN 0 AND 1439),
            end_minute INTEGER NOT NULL CHECK(end_minute BETWEEN 0 AND 1439),
            dismissed_occurrence INTEGER,
            CHECK(start_minute != end_minute)
        );",
    )?;
    transaction.pragma_update(None, "user_version", crate::NIGHT_WATCH_SCHEMA_VERSION)
}

fn read(
    connection: &rusqlite::Connection,
) -> Result<Option<NightWatchConfiguration>, TaskStoreError> {
    let operator = crate::presence::local_operator_id(connection)?;
    let config = connection.query_row(
        "SELECT enabled, timezone, start_minute, end_minute FROM operator_night_watch WHERE operator_id = ?1",
        [operator.to_string()],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, String>(1)?, row.get::<_, u16>(2)?, row.get::<_, u16>(3)?)),
    ).optional()?;
    config
        .map(|(enabled, zone, start, end)| {
            NightWatchConfiguration::new(enabled, &zone, start, end).ok_or_else(|| {
                TaskStoreError::IntegrityFailure("invalid stored Night Watch configuration".into())
            })
        })
        .transpose()
}

impl TaskStore {
    /// No configuration means no schedule, never an invented default bedtime.
    ///
    /// # Errors
    /// Returns persistence or configuration integrity errors.
    pub fn night_watch_configuration(
        &self,
    ) -> Result<Option<NightWatchConfiguration>, TaskStoreError> {
        let connection = self.connection()?;
        read(&connection)
    }

    /// Saves validated settings without turning them into a provider permission.
    ///
    /// # Errors
    /// Returns persistence errors; settings and their event commit atomically.
    pub fn set_night_watch_configuration(
        &self,
        config: &NightWatchConfiguration,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if read(&transaction)?.as_ref() == Some(config) {
            return Ok(false);
        }
        let operator = crate::presence::local_operator_id(&transaction)?;
        transaction.execute(
            "INSERT INTO operator_night_watch (operator_id, enabled, timezone, start_minute, end_minute)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(operator_id) DO UPDATE SET enabled = excluded.enabled, timezone = excluded.timezone,
                 start_minute = excluded.start_minute, end_minute = excluded.end_minute",
            params![operator.to_string(), config.enabled, config.timezone, config.start_minute, config.end_minute],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::PresenceChanged)?;
        transaction.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value).unwrap().timestamp()
    }

    #[test]
    fn named_zone_handles_both_dst_transitions_without_server_locale() {
        let config =
            NightWatchConfiguration::new(true, "America/New_York", 22 * 60, 7 * 60).unwrap();
        for dates in [
            [
                "2026-03-08T06:59:00Z",
                "2026-03-08T07:00:00Z",
                "2026-03-08T10:59:00Z",
                "2026-03-08T11:00:00Z",
            ],
            [
                "2026-11-01T05:59:00Z",
                "2026-11-01T06:00:00Z",
                "2026-11-01T11:59:00Z",
                "2026-11-01T12:00:00Z",
            ],
        ] {
            let occurrence = config.occurrence(instant(dates[0])).unwrap();
            assert!(occurrence.is_some());
            assert_eq!(config.occurrence(instant(dates[1])).unwrap(), occurrence);
            assert_eq!(config.occurrence(instant(dates[2])).unwrap(), occurrence);
            assert_eq!(config.occurrence(instant(dates[3])).unwrap(), None);
        }
        assert!(config.occurrence(i64::MAX).is_err());
        assert!(NightWatchConfiguration::new(true, "not/a-zone", 0, 60).is_none());
    }

    #[test]
    fn configuration_survives_reopen_and_repeated_save_does_not_emit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("night-watch.sqlite3");
        let config =
            NightWatchConfiguration::new(true, "America/New_York", 22 * 60, 7 * 60).unwrap();
        {
            let store = TaskStore::open(&path).unwrap();
            assert_eq!(store.night_watch_configuration().unwrap(), None);
            assert!(store.set_night_watch_configuration(&config).unwrap());
            let cursor = store.list_control_room_events(0).unwrap().next_cursor;
            assert!(!store.set_night_watch_configuration(&config).unwrap());
            assert!(
                store
                    .list_control_room_events(cursor)
                    .unwrap()
                    .events
                    .is_empty()
            );
        }
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .night_watch_configuration()
                .unwrap(),
            Some(config)
        );
    }

    #[test]
    fn configuration_event_failure_rolls_back_the_setting() {
        let store = TaskStore::in_memory().unwrap();
        let config = NightWatchConfiguration::new(true, "UTC", 1_320, 420).unwrap();
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER reject_presence_event BEFORE INSERT ON control_room_events
             BEGIN SELECT RAISE(ABORT, 'injected event failure'); END;",
            )
            .unwrap();
        assert!(store.set_night_watch_configuration(&config).is_err());
        assert_eq!(store.night_watch_configuration().unwrap(), None);
    }

    #[test]
    fn upgrading_previous_schema_preserves_manual_presence_without_enabling_schedule() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("upgrade.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .set_manual_presence(Some(swarm_domain::PresenceMode::NightWatch), 100)
                .unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TABLE operator_night_watch;")
                .unwrap();
            connection
                .pragma_update(
                    None,
                    "user_version",
                    crate::TERMINAL_CONTROL_PROJECTION_SCHEMA_VERSION,
                )
                .unwrap();
        }
        let upgraded = TaskStore::open(&path).unwrap();
        assert_eq!(upgraded.night_watch_configuration().unwrap(), None);
        assert_eq!(
            upgraded.operator_presence(101).unwrap().manual_mode,
            Some(swarm_domain::PresenceMode::NightWatch)
        );
        assert_eq!(
            upgraded.schema_version().unwrap(),
            crate::CURRENT_SCHEMA_VERSION
        );
    }
}
