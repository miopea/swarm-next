use crate::{HiveId, WorkerId, WorkerSessionId};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

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
                | (Self::Review, Self::Active | Self::Ready | Self::Completed)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDispatchState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}

impl fmt::Display for TaskDispatchState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}

impl FromStr for TaskDispatchState {
    type Err = ParseTaskDispatchStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseTaskDispatchStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskDispatchStateError;

impl fmt::Display for ParseTaskDispatchStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task dispatch state")
    }
}

impl std::error::Error for ParseTaskDispatchStateError {}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcomeDeliveryState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}

impl fmt::Display for TaskOutcomeDeliveryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}

impl FromStr for TaskOutcomeDeliveryState {
    type Err = ParseTaskOutcomeDeliveryStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseTaskOutcomeDeliveryStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskOutcomeDeliveryStateError;

impl fmt::Display for ParseTaskOutcomeDeliveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task outcome delivery state")
    }
}

impl std::error::Error for ParseTaskOutcomeDeliveryStateError {}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

impl fmt::Display for TaskPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        })
    }
}

impl FromStr for TaskPriority {
    type Err = ParseTaskPriorityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "urgent" => Ok(Self::Urgent),
            _ => Err(ParseTaskPriorityError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskPriorityError;

impl fmt::Display for ParseTaskPriorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task priority")
    }
}

impl std::error::Error for ParseTaskPriorityError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityKind {
    Created,
    DetailsUpdated,
    StateChanged,
    Assigned,
    Unassigned,
    Removed,
    Restored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityActorKind {
    Operator,
    Worker,
    Jira,
    Email,
    System,
}

impl fmt::Display for TaskActivityActorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Operator => "operator",
            Self::Worker => "worker",
            Self::Jira => "jira",
            Self::Email => "email",
            Self::System => "system",
        })
    }
}

impl FromStr for TaskActivityActorKind {
    type Err = ParseTaskActivityActorKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "operator" => Ok(Self::Operator),
            "worker" => Ok(Self::Worker),
            "jira" => Ok(Self::Jira),
            "email" => Ok(Self::Email),
            "system" => Ok(Self::System),
            _ => Err(ParseTaskActivityActorKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskActivityActorKindError;

impl fmt::Display for ParseTaskActivityActorKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task activity actor kind")
    }
}

impl std::error::Error for ParseTaskActivityActorKindError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskActivityActor {
    pub kind: TaskActivityActorKind,
    pub id: Option<String>,
}

impl TaskActivityActor {
    #[must_use]
    pub fn operator() -> Self {
        Self {
            kind: TaskActivityActorKind::Operator,
            id: None,
        }
    }
    #[must_use]
    pub fn worker(id: WorkerId) -> Self {
        Self {
            kind: TaskActivityActorKind::Worker,
            id: Some(id.to_string()),
        }
    }
    #[must_use]
    pub fn jira() -> Self {
        Self {
            kind: TaskActivityActorKind::Jira,
            id: None,
        }
    }
    #[must_use]
    pub fn email() -> Self {
        Self {
            kind: TaskActivityActorKind::Email,
            id: None,
        }
    }
    #[must_use]
    pub fn system() -> Self {
        Self {
            kind: TaskActivityActorKind::System,
            id: None,
        }
    }
}

impl fmt::Display for TaskActivityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Created => "created",
            Self::DetailsUpdated => "details_updated",
            Self::StateChanged => "state_changed",
            Self::Assigned => "assigned",
            Self::Unassigned => "unassigned",
            Self::Removed => "removed",
            Self::Restored => "restored",
        })
    }
}

impl FromStr for TaskActivityKind {
    type Err = ParseTaskActivityKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "created" => Ok(Self::Created),
            "details_updated" => Ok(Self::DetailsUpdated),
            "state_changed" => Ok(Self::StateChanged),
            "assigned" => Ok(Self::Assigned),
            "unassigned" => Ok(Self::Unassigned),
            "removed" => Ok(Self::Removed),
            "restored" => Ok(Self::Restored),
            _ => Err(ParseTaskActivityKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskActivityKindError;

impl fmt::Display for ParseTaskActivityKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task activity kind")
    }
}

impl std::error::Error for ParseTaskActivityKindError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskActivity {
    pub sequence: i64,
    pub task_id: TaskId,
    pub kind: TaskActivityKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>,
    pub note: String,
    pub occurred_at: i64,
    pub actor_kind: TaskActivityActorKind,
    pub actor_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskActivityPage {
    pub events: Vec<TaskActivity>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskDetailsUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<TaskPriority>,
    pub workspace: Option<String>,
    pub operator_instruction: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub hive_id: HiveId,
    pub title: String,
    pub description: String,
    /// One line from the operator about how this task should be approached,
    /// rather than about what it contains. "Interview me first" and "analyse
    /// this, do not act on it" govern the work without being part of it, and
    /// putting them in the description makes them read as part of it.
    pub operator_instruction: String,
    /// Whether anyone has recorded where this work is running.
    ///
    /// Computed rather than stored. A completed task without it is finished,
    /// not shown to be live — and calling that COMPLETED claims more than
    /// anyone has established, which is the same distinction between committed
    /// and deployed that this repo draws everywhere else.
    #[serde(default)]
    pub deployment_recorded: bool,
    pub priority: TaskPriority,
    pub workspace: String,
    pub state: TaskState,
    pub assigned_worker_id: Option<WorkerId>,
    pub assigned_session_id: Option<WorkerSessionId>,
    pub dispatch_state: Option<TaskDispatchState>,
    pub outcome_delivery_state: Option<TaskOutcomeDeliveryState>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
