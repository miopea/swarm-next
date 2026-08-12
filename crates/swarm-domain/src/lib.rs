use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

domain_id!(OperatorId);
domain_id!(HiveId);
domain_id!(ApiaryId);
domain_id!(StewardshipId);
domain_id!(ProviderConversationId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedWorkBackend {
    Jira,
    Native,
}

impl fmt::Display for SharedWorkBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Jira => "jira",
            Self::Native => "native",
        })
    }
}

impl FromStr for SharedWorkBackend {
    type Err = ParseSharedWorkBackendError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "jira" => Ok(Self::Jira),
            "native" => Ok(Self::Native),
            _ => Err(ParseSharedWorkBackendError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseSharedWorkBackendError;

impl fmt::Display for ParseSharedWorkBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown shared-work backend")
    }
}

impl std::error::Error for ParseSharedWorkBackendError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Operator {
    pub id: OperatorId,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hive {
    pub id: HiveId,
    pub name: String,
    pub operator_id: OperatorId,
    pub apiary_id: Option<ApiaryId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveIdentity {
    pub operator: Operator,
    pub hive: Hive,
}

impl Hive {
    #[must_use]
    pub fn personal(name: impl Into<String>, operator_id: OperatorId) -> Self {
        Self {
            id: HiveId::new(),
            name: name.into(),
            operator_id,
            apiary_id: None,
        }
    }

    /// Joins one Apiary without allowing implicit federation transfer.
    ///
    /// # Errors
    /// Returns the current Apiary when this Hive must leave it first.
    pub fn join(&mut self, apiary_id: ApiaryId) -> Result<(), HiveMembershipError> {
        if let Some(current) = self.apiary_id {
            return Err(HiveMembershipError::AlreadyJoined(current));
        }
        self.apiary_id = Some(apiary_id);
        Ok(())
    }

    /// Leaves the current Apiary after application services validate shared work.
    pub fn leave(&mut self) -> Option<ApiaryId> {
        self.apiary_id.take()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HiveMembershipError {
    AlreadyJoined(ApiaryId),
}

impl fmt::Display for HiveMembershipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyJoined(apiary_id) => {
                write!(
                    formatter,
                    "Hive must leave Apiary {apiary_id} before joining another"
                )
            }
        }
    }
}

impl std::error::Error for HiveMembershipError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Apiary {
    pub id: ApiaryId,
    pub name: String,
    pub keeper_operator_id: OperatorId,
    shared_work_backend: SharedWorkBackend,
}

impl Apiary {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        keeper_operator_id: OperatorId,
        shared_work_backend: SharedWorkBackend,
    ) -> Self {
        Self {
            id: ApiaryId::new(),
            name: name.into(),
            keeper_operator_id,
            shared_work_backend,
        }
    }

    #[must_use]
    pub const fn shared_work_backend(&self) -> SharedWorkBackend {
        self.shared_work_backend
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StewardCapability {
    Observe,
    Assign,
    Assist,
    Takeover,
    ManageProjects,
    ManageMembers,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stewardship {
    pub id: StewardshipId,
    pub apiary_id: ApiaryId,
    pub steward_operator_id: OperatorId,
    pub managed_hive_ids: Vec<HiveId>,
    pub capabilities: Vec<StewardCapability>,
}

impl Stewardship {
    #[must_use]
    pub fn allows(&self, hive_id: HiveId, capability: StewardCapability) -> bool {
        self.managed_hive_ids.contains(&hive_id) && self.capabilities.contains(&capability)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "role", content = "stewardship_id")]
pub enum HiveAuthority {
    Owner,
    Keeper,
    Steward(StewardshipId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveAuthorization {
    pub actor_operator_id: OperatorId,
    pub target_hive_id: HiveId,
    pub capability: StewardCapability,
    pub authority: HiveAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HiveAuthorizationDenied;

impl fmt::Display for HiveAuthorizationDenied {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operator is not authorized for the Hive capability")
    }
}

impl std::error::Error for HiveAuthorizationDenied {}

/// Resolves one explicit Hive capability without inferring authority from presence or agent output.
///
/// # Errors
/// Denies access when the actor is not the Hive owner, the matching Apiary Keeper, or an
/// in-scope Steward with the requested capability. Missing or mismatched federation context
/// fails closed.
pub fn authorize_hive_capability(
    actor_operator_id: OperatorId,
    target_hive: &Hive,
    apiary: Option<&Apiary>,
    stewardships: &[Stewardship],
    capability: StewardCapability,
) -> Result<HiveAuthorization, HiveAuthorizationDenied> {
    let authority = if actor_operator_id == target_hive.operator_id {
        HiveAuthority::Owner
    } else {
        let target_apiary_id = target_hive.apiary_id.ok_or(HiveAuthorizationDenied)?;
        let apiary = apiary
            .filter(|candidate| candidate.id == target_apiary_id)
            .ok_or(HiveAuthorizationDenied)?;
        if actor_operator_id == apiary.keeper_operator_id {
            HiveAuthority::Keeper
        } else {
            let stewardship = stewardships.iter().find(|stewardship| {
                stewardship.apiary_id == target_apiary_id
                    && stewardship.steward_operator_id == actor_operator_id
                    && stewardship.allows(target_hive.id, capability)
            });
            HiveAuthority::Steward(stewardship.ok_or(HiveAuthorizationDenied)?.id)
        }
    };

    Ok(HiveAuthorization {
        actor_operator_id,
        target_hive_id: target_hive.id,
        capability,
        authority,
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryCollapseReadiness {
    pub active_hive_count: usize,
    pub pending_invitation_count: usize,
    pub active_stewardship_count: usize,
    pub open_cross_hive_work_count: usize,
    pub departed_node_count: usize,
}

impl ApiaryCollapseReadiness {
    #[must_use]
    pub const fn can_collapse(self) -> bool {
        self.active_hive_count == 1
            && self.pending_invitation_count == 0
            && self.active_stewardship_count == 0
            && self.open_cross_hive_work_count == 0
            && self.departed_node_count == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRoomEventKind {
    TasksChanged,
    WorkersChanged,
    SessionsChanged,
    RuntimeChanged,
    DecisionsChanged,
}

impl fmt::Display for ControlRoomEventKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TasksChanged => "tasks_changed",
            Self::WorkersChanged => "workers_changed",
            Self::SessionsChanged => "sessions_changed",
            Self::RuntimeChanged => "runtime_changed",
            Self::DecisionsChanged => "decisions_changed",
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
    pub hive_id: HiveId,
    pub name: String,
    pub role: WorkerRole,
    pub provider: ProviderKind,
    pub workspace: String,
    pub autostart: bool,
    pub position: i64,
    pub active_session_id: Option<WorkerSessionId>,
    /// Provider-owned conversation identity used for exact process recovery.
    #[serde(skip)]
    pub provider_conversation_id: Option<ProviderConversationId>,
    /// Whether this profile has previously launched a provider process.
    #[serde(skip)]
    pub has_session_history: bool,
    /// Expiry of the active operator engagement lease, when one exists.
    #[serde(skip)]
    pub engagement_expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerAttentionState {
    Sleeping,
    Buzzing,
    WithOperator,
    Blocked,
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
}

impl fmt::Display for TaskActivityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Created => "created",
            Self::DetailsUpdated => "details_updated",
            Self::StateChanged => "state_changed",
            Self::Assigned => "assigned",
            Self::Unassigned => "unassigned",
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
    pub occurred_at: i64,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub hive_id: HiveId,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub workspace: String,
    pub state: TaskState,
    pub assigned_session_id: Option<WorkerSessionId>,
    pub dispatch_state: Option<TaskDispatchState>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecisionRequestId(Uuid);

impl DecisionRequestId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for DecisionRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DecisionRequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DecisionRequestId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRequestKind {
    Input,
    Approval,
    Credentials,
    Conflict,
    Help,
}

impl fmt::Display for DecisionRequestKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "input",
            Self::Approval => "approval",
            Self::Credentials => "credentials",
            Self::Conflict => "conflict",
            Self::Help => "help",
        })
    }
}

impl FromStr for DecisionRequestKind {
    type Err = ParseDecisionRequestKindError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "input" => Ok(Self::Input),
            "approval" => Ok(Self::Approval),
            "credentials" => Ok(Self::Credentials),
            "conflict" => Ok(Self::Conflict),
            "help" => Ok(Self::Help),
            _ => Err(ParseDecisionRequestKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionRequestKindError;
impl fmt::Display for ParseDecisionRequestKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision request kind")
    }
}
impl std::error::Error for ParseDecisionRequestKindError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionUrgency {
    #[default]
    Normal,
    TimeSensitive,
}
impl fmt::Display for DecisionUrgency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Normal => "normal",
            Self::TimeSensitive => "time_sensitive",
        })
    }
}
impl FromStr for DecisionUrgency {
    type Err = ParseDecisionUrgencyError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "normal" => Ok(Self::Normal),
            "time_sensitive" => Ok(Self::TimeSensitive),
            _ => Err(ParseDecisionUrgencyError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionUrgencyError;
impl fmt::Display for ParseDecisionUrgencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision urgency")
    }
}
impl std::error::Error for ParseDecisionUrgencyError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionDeliveryState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}
impl fmt::Display for DecisionDeliveryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}
impl FromStr for DecisionDeliveryState {
    type Err = ParseDecisionDeliveryStateError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseDecisionDeliveryStateError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionDeliveryStateError;
impl fmt::Display for ParseDecisionDeliveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision delivery state")
    }
}
impl std::error::Error for ParseDecisionDeliveryStateError {}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRequestState {
    Pending,
    Resolved,
}
impl fmt::Display for DecisionRequestState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
        })
    }
}
impl FromStr for DecisionRequestState {
    type Err = ParseDecisionRequestStateError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "resolved" => Ok(Self::Resolved),
            _ => Err(ParseDecisionRequestStateError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionRequestStateError;
impl fmt::Display for ParseDecisionRequestStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision request state")
    }
}
impl std::error::Error for ParseDecisionRequestStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub id: DecisionRequestId,
    pub hive_id: HiveId,
    pub requesting_worker_id: WorkerId,
    pub task_id: Option<TaskId>,
    pub kind: DecisionRequestKind,
    pub urgency: DecisionUrgency,
    pub title: String,
    pub reason: String,
    pub risk: String,
    pub evidence: String,
    pub suggested_action: String,
    pub allowed_actions: Vec<String>,
    pub deadline: Option<i64>,
    pub state: DecisionRequestState,
    pub resolution_action: Option<String>,
    pub resolution_note: String,
    pub resolved_by_operator_id: Option<OperatorId>,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub delivery_state: Option<DecisionDeliveryState>,
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
            hive_id: HiveId::new(),
            name: "Queen".into(),
            role: WorkerRole::Queen,
            provider: ProviderKind::ClaudeCode,
            workspace: "/workspace/queen".into(),
            autostart: true,
            position: 0,
            active_session_id: None,
            provider_conversation_id: None,
            has_session_history: false,
            engagement_expires_at: None,
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

    #[test]
    fn hive_membership_is_exclusive_and_requires_leaving_first() {
        let operator_id = OperatorId::new();
        let first_apiary = ApiaryId::new();
        let second_apiary = ApiaryId::new();
        let mut hive = Hive::personal("Violet Hive", operator_id);

        hive.join(first_apiary).unwrap();
        assert_eq!(hive.apiary_id, Some(first_apiary));
        assert_eq!(
            hive.join(second_apiary),
            Err(HiveMembershipError::AlreadyJoined(first_apiary))
        );

        assert_eq!(hive.leave(), Some(first_apiary));
        hive.join(second_apiary).unwrap();
        assert_eq!(hive.apiary_id, Some(second_apiary));
    }

    #[test]
    fn apiary_backend_is_selected_at_creation() {
        let keeper_id = OperatorId::new();
        let jira = Apiary::new("Garden", keeper_id, SharedWorkBackend::Jira);
        let native = Apiary::new("Workshop", keeper_id, SharedWorkBackend::Native);

        assert_eq!(jira.shared_work_backend(), SharedWorkBackend::Jira);
        assert_eq!(native.shared_work_backend(), SharedWorkBackend::Native);
        assert_ne!(jira.id, native.id);
    }

    #[test]
    fn stewardship_authority_is_explicitly_scoped() {
        let managed_hive = HiveId::new();
        let other_hive = HiveId::new();
        let stewardship = Stewardship {
            id: StewardshipId::new(),
            apiary_id: ApiaryId::new(),
            steward_operator_id: OperatorId::new(),
            managed_hive_ids: vec![managed_hive],
            capabilities: vec![StewardCapability::Observe, StewardCapability::Takeover],
        };

        assert!(stewardship.allows(managed_hive, StewardCapability::Observe));
        assert!(stewardship.allows(managed_hive, StewardCapability::Takeover));
        assert!(!stewardship.allows(managed_hive, StewardCapability::Assign));
        assert!(!stewardship.allows(other_hive, StewardCapability::Takeover));
    }

    #[test]
    fn hive_authorization_is_owner_keeper_or_explicitly_scoped_steward() {
        let owner_id = OperatorId::new();
        let keeper_id = OperatorId::new();
        let steward_id = OperatorId::new();
        let stranger_id = OperatorId::new();
        let mut hive = Hive::personal("Developer Hive", owner_id);

        let owner =
            authorize_hive_capability(owner_id, &hive, None, &[], StewardCapability::Takeover)
                .unwrap();
        assert_eq!(owner.authority, HiveAuthority::Owner);

        let apiary = Apiary::new("Garden", keeper_id, SharedWorkBackend::Jira);
        hive.join(apiary.id).unwrap();
        let keeper = authorize_hive_capability(
            keeper_id,
            &hive,
            Some(&apiary),
            &[],
            StewardCapability::ManageProjects,
        )
        .unwrap();
        assert_eq!(keeper.authority, HiveAuthority::Keeper);

        let stewardship = Stewardship {
            id: StewardshipId::new(),
            apiary_id: apiary.id,
            steward_operator_id: steward_id,
            managed_hive_ids: vec![hive.id],
            capabilities: vec![StewardCapability::Observe],
        };
        let steward = authorize_hive_capability(
            steward_id,
            &hive,
            Some(&apiary),
            std::slice::from_ref(&stewardship),
            StewardCapability::Observe,
        )
        .unwrap();
        assert_eq!(steward.authority, HiveAuthority::Steward(stewardship.id));

        assert!(
            authorize_hive_capability(
                steward_id,
                &hive,
                Some(&apiary),
                std::slice::from_ref(&stewardship),
                StewardCapability::Takeover,
            )
            .is_err()
        );
        assert!(
            authorize_hive_capability(
                stranger_id,
                &hive,
                Some(&apiary),
                std::slice::from_ref(&stewardship),
                StewardCapability::Observe,
            )
            .is_err()
        );
        let wrong_apiary = Apiary::new("Other", keeper_id, SharedWorkBackend::Jira);
        assert!(
            authorize_hive_capability(
                keeper_id,
                &hive,
                Some(&wrong_apiary),
                &[],
                StewardCapability::Observe,
            )
            .is_err()
        );
    }

    #[test]
    fn sole_hive_collapse_fails_closed_on_any_federation_state() {
        let ready = ApiaryCollapseReadiness {
            active_hive_count: 1,
            ..ApiaryCollapseReadiness::default()
        };
        assert!(ready.can_collapse());

        for blocked in [
            ApiaryCollapseReadiness {
                active_hive_count: 2,
                ..ready
            },
            ApiaryCollapseReadiness {
                pending_invitation_count: 1,
                ..ready
            },
            ApiaryCollapseReadiness {
                active_stewardship_count: 1,
                ..ready
            },
            ApiaryCollapseReadiness {
                open_cross_hive_work_count: 1,
                ..ready
            },
            ApiaryCollapseReadiness {
                departed_node_count: 1,
                ..ready
            },
        ] {
            assert!(!blocked.can_collapse());
        }
    }
}
