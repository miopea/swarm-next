//! Terminal history retention's storage (ADR 0118): the Hive's own choice, and
//! what its terminal host last said it is using. The host is told through a
//! file, never through this database, so what it says is the only evidence
//! that the setting is in effect.

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{ControlRoomEventKind, HistoryRetentionDays, HostRetention, RequiredSetting};

use crate::{
    TaskStore, TaskStoreError, apiary_policy::received_policy_settings_from,
    insert_control_room_event,
};

pub(crate) fn migrate_terminal_history(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS terminal_history_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             days INTEGER CHECK (days IS NULL OR days IN (1, 3, 7, 14, 30)),
             updated_at INTEGER,
             host_state TEXT NOT NULL DEFAULT 'unknown'
                 CHECK (host_state IN ('unknown', 'silent', 'using')),
             host_max_age_seconds INTEGER CHECK (host_max_age_seconds IS NULL OR host_max_age_seconds > 0),
             host_checked_at INTEGER
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::TERMINAL_HISTORY_SCHEMA_MARKER)
}

/// This Hive's operator's own choice, before any Apiary requirement.
fn own_days(connection: &Connection) -> Result<HistoryRetentionDays, TaskStoreError> {
    let stored: Option<Option<u16>> = connection
        .query_row(
            "SELECT days FROM terminal_history_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    stored
        .flatten()
        .map_or(Ok(HistoryRetentionDays::DEFAULT), |days| {
            HistoryRetentionDays::new(days).map_err(TaskStoreError::IntegrityFailure)
        })
}

/// The days this Hive's Apiary requires, if it requires them (ADR 0113).
fn required_days(connection: &Connection) -> Result<Option<HistoryRetentionDays>, TaskStoreError> {
    Ok(received_policy_settings_from(connection)?
        .into_iter()
        .find(|setting| setting.key == RequiredSetting::TerminalHistoryDays.key())
        .and_then(|setting| setting.value.parse().ok()))
}

fn host_retention(connection: &Connection) -> Result<HostRetention, TaskStoreError> {
    let stored: Option<(String, Option<i64>)> = connection
        .query_row(
            "SELECT host_state, host_max_age_seconds FROM terminal_history_settings
             WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match stored {
        Some((state, Some(seconds))) if state == "using" => HostRetention::Using {
            max_age_seconds: u64::try_from(seconds)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
        },
        Some((state, _)) if state == "silent" => HostRetention::Silent,
        _ => HostRetention::Unknown,
    })
}

fn setting_from(connection: &Connection) -> Result<TerminalHistorySetting, TaskStoreError> {
    let set_by_apiary = required_days(connection)?;
    let days = set_by_apiary.unwrap_or(own_days(connection)?);
    let host = host_retention(connection)?;
    Ok(TerminalHistorySetting {
        days,
        set_by_apiary,
        host,
        in_effect: host.keeps(days),
    })
}

/// Terminal history retention as this Hive runs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalHistorySetting {
    /// The days in effect as a setting.
    pub days: HistoryRetentionDays,
    /// The days this Hive's Apiary requires, if any; locked here.
    pub set_by_apiary: Option<HistoryRetentionDays>,
    /// What the terminal host last said it is using.
    pub host: HostRetention,
    /// Whether the host is keeping exactly `days`.
    pub in_effect: bool,
}

impl TaskStore {
    /// Terminal history retention on this Hive.
    ///
    /// # Errors
    /// Returns persistence or stored-setting integrity failures.
    pub fn terminal_history(&self) -> Result<TerminalHistorySetting, TaskStoreError> {
        let connection = self.connection()?;
        setting_from(&connection)
    }

    /// This Hive's operator choosing how long to keep terminal history.
    ///
    /// # Errors
    /// Refuses days other than the ones this Hive's Apiary requires (ADR 0113),
    /// and returns persistence failures.
    pub fn set_terminal_history_days(
        &self,
        days: HistoryRetentionDays,
        now: i64,
    ) -> Result<TerminalHistorySetting, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let required = required_days(&transaction)?;
        if required.is_some_and(|required| required != days) {
            return Err(TaskStoreError::SetByApiary(
                "your Apiary sets how long this Hive keeps terminal history; only its Keeper can \
                 change it"
                    .to_owned(),
            ));
        }
        if required.is_none() && own_days(&transaction)? != days {
            transaction.execute(
                "INSERT INTO terminal_history_settings (singleton, days, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     days = excluded.days, updated_at = excluded.updated_at",
                params![days.days(), now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        let setting = setting_from(&transaction)?;
        transaction.commit()?;
        Ok(setting)
    }

    /// Records what the terminal host said it is using: `None` when it has
    /// said nothing, which is a host older than this setting.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_terminal_history_host(
        &self,
        max_age_seconds: Option<u64>,
        now: i64,
    ) -> Result<TerminalHistorySetting, TaskStoreError> {
        let connection = self.connection()?;
        let seconds = max_age_seconds
            .map(i64::try_from)
            .transpose()
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        connection.execute(
            "INSERT INTO terminal_history_settings
                 (singleton, host_state, host_max_age_seconds, host_checked_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
                 host_state = excluded.host_state,
                 host_max_age_seconds = excluded.host_max_age_seconds,
                 host_checked_at = excluded.host_checked_at",
            params![
                if seconds.is_some() { "using" } else { "silent" },
                seconds,
                now
            ],
        )?;
        setting_from(&connection)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::ApiaryPolicySetting;

    use super::*;

    fn days(days: u16) -> HistoryRetentionDays {
        HistoryRetentionDays::new(days).unwrap()
    }

    /// Stands in for a verified policy body; the signed path is the API test's.
    fn require(store: &TaskStore, settings: &[ApiaryPolicySetting]) {
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO local_policy_manifest
                    (singleton, policy_revision, settings_digest, settings_json, received_at)
                 VALUES (1, 1, 'test', ?1, 1)
                 ON CONFLICT(singleton) DO UPDATE SET settings_json = excluded.settings_json",
                [serde_json::to_string(settings).unwrap()],
            )
            .unwrap();
    }

    /// ⚠️ IN EFFECT MEANS THE HOST SAYS SO. Seven days by default, and nothing
    /// counts as in effect until the host has said what it keeps.
    #[test]
    fn retention_is_in_effect_only_when_the_host_says_it_keeps_it() {
        let store = TaskStore::in_memory().unwrap();
        let start = store.terminal_history().unwrap();
        assert_eq!(start.days, HistoryRetentionDays::DEFAULT);
        assert_eq!(start.host, HostRetention::Unknown);
        assert!(!start.in_effect);

        let chosen = store.set_terminal_history_days(days(3), 10).unwrap();
        assert_eq!(chosen.days, days(3));
        assert!(!chosen.in_effect, "chosen is not applied");

        let silent = store.record_terminal_history_host(None, 20).unwrap();
        assert_eq!(silent.host, HostRetention::Silent);
        assert!(!silent.in_effect, "an older host has said nothing");

        let applying = store
            .record_terminal_history_host(Some(7 * 86_400), 30)
            .unwrap();
        assert!(!applying.in_effect, "still on the old age");

        let applied = store
            .record_terminal_history_host(Some(3 * 86_400), 40)
            .unwrap();
        assert!(applied.in_effect);
        assert_eq!(
            store.terminal_history().unwrap().days,
            days(3),
            "the choice survives the host write"
        );
    }

    /// ⚠️ ADR 0113: required days are the ones in effect, the member's own
    /// choice waits underneath, and a different local choice is refused.
    #[test]
    fn required_days_win_and_the_members_own_choice_waits_underneath() {
        let store = TaskStore::in_memory().unwrap();
        store.set_terminal_history_days(days(14), 10).unwrap();
        require(
            &store,
            &[ApiaryPolicySetting {
                key: "terminal.history_days".to_owned(),
                value: "1".to_owned(),
            }],
        );
        let locked = store.terminal_history().unwrap();
        assert_eq!(locked.days, days(1));
        assert_eq!(locked.set_by_apiary, Some(days(1)));
        assert!(matches!(
            store.set_terminal_history_days(days(30), 20),
            Err(TaskStoreError::SetByApiary(_))
        ));
        require(&store, &[]);
        assert_eq!(store.terminal_history().unwrap().days, days(14));
    }
}
