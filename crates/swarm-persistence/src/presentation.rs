use std::{fmt, str::FromStr};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use swarm_domain::ControlRoomEventKind;

use super::{TaskStore, TaskStoreError, insert_control_room_event, presence::local_operator_id};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationDeviceClass {
    Desktop,
    Mobile,
}

impl fmt::Display for PresentationDeviceClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
        })
    }
}

impl FromStr for PresentationDeviceClass {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "desktop" => Ok(Self::Desktop),
            "mobile" => Ok(Self::Mobile),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationColorTheme {
    Light,
    Dark,
}

impl fmt::Display for PresentationColorTheme {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Light => "light",
            Self::Dark => "dark",
        })
    }
}

impl FromStr for PresentationColorTheme {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PresentationPreferences {
    pub device_class: PresentationDeviceClass,
    pub color_theme: PresentationColorTheme,
    pub terminal_keys_visible: bool,
    pub configured: bool,
}

impl PresentationPreferences {
    fn defaults(device_class: PresentationDeviceClass) -> Self {
        Self {
            device_class,
            color_theme: PresentationColorTheme::Light,
            terminal_keys_visible: true,
            configured: false,
        }
    }
}

impl TaskStore {
    /// Returns one desktop or mobile presentation profile.
    /// # Errors
    /// Returns an error when preferences cannot be read from durable storage.
    pub fn presentation_preferences(
        &self,
        device_class: PresentationDeviceClass,
    ) -> Result<PresentationPreferences, TaskStoreError> {
        let connection = self.connection()?;
        presentation_preferences_from_connection(&connection, device_class)
    }

    /// Replaces one presentation profile atomically.
    /// # Errors
    /// Returns an error for invalid preferences or an unavailable transaction.
    pub fn set_presentation_preferences(
        &self,
        preferences: PresentationPreferences,
        now: i64,
    ) -> Result<PresentationPreferences, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let operator_id = local_operator_id(&transaction)?;
        let before =
            presentation_preferences_from_connection(&transaction, preferences.device_class)?;
        transaction.execute(
            "INSERT INTO presentation_preferences (
                 operator_id, device_class, color_theme, terminal_keys_visible, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(operator_id, device_class) DO UPDATE SET
                 color_theme = excluded.color_theme,
                 terminal_keys_visible = excluded.terminal_keys_visible,
                 updated_at = excluded.updated_at",
            params![
                operator_id.to_string(),
                preferences.device_class.to_string(),
                preferences.color_theme.to_string(),
                preferences.terminal_keys_visible,
                now,
            ],
        )?;
        let stored = PresentationPreferences {
            configured: true,
            ..preferences
        };
        if before != stored {
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        transaction.commit()?;
        Ok(stored)
    }
}

fn presentation_preferences_from_connection(
    connection: &Connection,
    device_class: PresentationDeviceClass,
) -> Result<PresentationPreferences, TaskStoreError> {
    let operator_id = local_operator_id(connection)?;
    let stored = connection
        .query_row(
            "SELECT color_theme, terminal_keys_visible
             FROM presentation_preferences
             WHERE operator_id = ?1 AND device_class = ?2",
            params![operator_id.to_string(), device_class.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?;
    let Some((color_theme, terminal_keys_visible)) = stored else {
        return Ok(PresentationPreferences::defaults(device_class));
    };
    Ok(PresentationPreferences {
        device_class,
        color_theme: PresentationColorTheme::from_str(&color_theme).map_err(|()| {
            TaskStoreError::IntegrityFailure("invalid presentation color theme".into())
        })?,
        terminal_keys_visible,
        configured: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_and_mobile_preferences_are_independent_and_durable() {
        let store = TaskStore::in_memory().unwrap();
        let desktop = store
            .presentation_preferences(PresentationDeviceClass::Desktop)
            .unwrap();
        assert_eq!(desktop.color_theme, PresentationColorTheme::Light);
        assert!(!desktop.configured);

        let mobile = PresentationPreferences {
            device_class: PresentationDeviceClass::Mobile,
            color_theme: PresentationColorTheme::Dark,
            terminal_keys_visible: false,
            configured: false,
        };
        let stored = store.set_presentation_preferences(mobile, 42).unwrap();
        assert!(stored.configured);
        assert_eq!(
            store
                .presentation_preferences(PresentationDeviceClass::Mobile)
                .unwrap(),
            stored
        );
        assert_eq!(
            store
                .presentation_preferences(PresentationDeviceClass::Desktop)
                .unwrap(),
            desktop
        );
    }
}
