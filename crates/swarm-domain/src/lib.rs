use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(Uuid);

impl WorkerId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorkerId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerSessionId(Uuid);

impl WorkerSessionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorkerSessionId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerRole {
    Queen,
    Worker,
}

impl fmt::Display for WorkerRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queen => "queen",
            Self::Worker => "worker",
        })
    }
}

impl FromStr for WorkerRole {
    type Err = ParseWorkerRoleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queen" => Ok(Self::Queen),
            "worker" => Ok(Self::Worker),
            _ => Err(ParseWorkerRoleError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseWorkerRoleError;

impl fmt::Display for ParseWorkerRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown worker role")
    }
}

impl std::error::Error for ParseWorkerRoleError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
        })
    }
}

impl FromStr for ProviderKind {
    type Err = ParseProviderKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude_code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            _ => Err(ParseProviderKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseProviderKindError;

impl fmt::Display for ParseProviderKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown provider kind")
    }
}

impl std::error::Error for ParseProviderKindError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkerProfile {
    pub id: WorkerId,
    pub name: String,
    pub role: WorkerRole,
    pub provider: ProviderKind,
    pub workspace: String,
    pub autostart: bool,
    pub position: i64,
    pub active_session_id: Option<WorkerSessionId>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerSessionState {
    Starting,
    Running,
    Exited,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkerSession {
    pub id: WorkerSessionId,
    pub worker_id: WorkerId,
    pub provider: ProviderKind,
    pub state: WorkerSessionState,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(Uuid);

impl TaskId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TaskId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Draft,
    Ready,
    Active,
    Blocked,
    Review,
    Completed,
}

impl TaskState {
    #[must_use]
    pub const fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Draft, Self::Ready)
                | (Self::Ready, Self::Active | Self::Blocked)
                | (Self::Active, Self::Blocked | Self::Review)
                | (Self::Blocked, Self::Ready | Self::Active)
                | (Self::Review, Self::Active | Self::Completed)
        )
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Completed => "completed",
        };
        formatter.write_str(value)
    }
}

impl FromStr for TaskState {
    type Err = ParseTaskStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "ready" => Ok(Self::Ready),
            "active" => Ok(Self::Active),
            "blocked" => Ok(Self::Blocked),
            "review" => Ok(Self::Review),
            "completed" => Ok(Self::Completed),
            _ => Err(ParseTaskStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskStateError;

impl fmt::Display for ParseTaskStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task state")
    }
}

impl std::error::Error for ParseTaskStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub workspace: String,
    pub state: TaskState,
    pub assigned_session_id: Option<WorkerSessionId>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_identity_is_not_worker_identity() {
        let worker_id = WorkerId::new();
        let first = WorkerSession {
            id: WorkerSessionId::new(),
            worker_id,
            provider: ProviderKind::ClaudeCode,
            state: WorkerSessionState::Running,
        };
        let second = WorkerSession {
            id: WorkerSessionId::new(),
            worker_id,
            provider: ProviderKind::ClaudeCode,
            state: WorkerSessionState::Starting,
        };
        assert_ne!(first.id, second.id);
        assert_eq!(first.worker_id, second.worker_id);
    }

    #[test]
    fn worker_profile_identity_outlives_its_session() {
        let profile = WorkerProfile {
            id: WorkerId::new(),
            name: "Queen".into(),
            role: WorkerRole::Queen,
            provider: ProviderKind::ClaudeCode,
            workspace: "/workspace/queen".into(),
            autostart: true,
            position: 0,
            active_session_id: None,
            created_at: 1,
            updated_at: 1,
        };
        assert_eq!(profile.role, WorkerRole::Queen);
        assert!(profile.autostart);
    }

    #[test]
    fn task_transitions_are_explicit() {
        assert!(TaskState::Draft.can_transition_to(TaskState::Ready));
        assert!(TaskState::Review.can_transition_to(TaskState::Completed));
        assert!(TaskState::Blocked.can_transition_to(TaskState::Active));
        assert!(!TaskState::Ready.can_transition_to(TaskState::Completed));
        assert!(!TaskState::Completed.can_transition_to(TaskState::Active));
    }
}
