use crate::HiveId;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRoomEventKind {
    TasksChanged,
    WorkersChanged,
    SessionsChanged,
    RuntimeChanged,
    DecisionsChanged,
    PresenceChanged,
    NotificationsChanged,
}

impl fmt::Display for ControlRoomEventKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TasksChanged => "tasks_changed",
            Self::WorkersChanged => "workers_changed",
            Self::SessionsChanged => "sessions_changed",
            Self::RuntimeChanged => "runtime_changed",
            Self::DecisionsChanged => "decisions_changed",
            Self::PresenceChanged => "presence_changed",
            Self::NotificationsChanged => "notifications_changed",
        })
    }
}

impl FromStr for ControlRoomEventKind {
    type Err = ParseControlRoomEventKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tasks_changed" => Ok(Self::TasksChanged),
            "workers_changed" => Ok(Self::WorkersChanged),
            "sessions_changed" => Ok(Self::SessionsChanged),
            "runtime_changed" => Ok(Self::RuntimeChanged),
            "decisions_changed" => Ok(Self::DecisionsChanged),
            "presence_changed" => Ok(Self::PresenceChanged),
            "notifications_changed" => Ok(Self::NotificationsChanged),
            _ => Err(ParseControlRoomEventKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseControlRoomEventKindError;

impl fmt::Display for ParseControlRoomEventKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown control-room event kind")
    }
}

impl std::error::Error for ParseControlRoomEventKindError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ControlRoomEvent {
    pub sequence: i64,
    pub hive_id: HiveId,
    pub kind: ControlRoomEventKind,
    pub occurred_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ControlRoomEventPage {
    pub events: Vec<ControlRoomEvent>,
    pub next_cursor: i64,
    pub reset_required: bool,
}
