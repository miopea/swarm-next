use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

mod apiary;
mod control_room;
mod decisions;
mod release;
mod tasks;
mod terminal_control;
mod version;
mod workers;

pub use apiary::*;
pub use control_room::*;
pub use decisions::*;
pub use release::*;
pub use tasks::*;
pub use terminal_control::*;
pub use version::{DevelopmentBuild, SwarmVersion};
pub use workers::*;

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
domain_id!(ApiaryInvitationId);
domain_id!(ApiaryJoinLinkId);
domain_id!(FederationNodeId);
domain_id!(FederationMembershipReceiptId);
domain_id!(FederationDepartureReceiptId);
domain_id!(FederationClaimId);
domain_id!(FederationClaimHandoffId);
domain_id!(ApiaryTaskId);
domain_id!(FederationTaskCommandId);
domain_id!(FederationStewardTaskCommandId);
domain_id!(FederationStewardAssistCommandId);
domain_id!(FederationStewardAssistRequestId);
domain_id!(FederationStewardTakeoverCommandId);
domain_id!(FederationStewardTakeoverLeaseId);
domain_id!(StewardshipId);
domain_id!(ProviderConversationId);
domain_id!(PresenceDeviceId);
domain_id!(TerminalViewId);
domain_id!(JiraProjectBindingId);
domain_id!(DeploymentGrantId);
domain_id!(DeploymentAuthorizationId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedWorkBackend {
    Jira,
    Native,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraConnectionState {
    NotConnected,
    Ready,
    NetworkUnavailable,
    CredentialsInvalid,
    PermissionDenied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraProjectScope {
    Hive,
    Apiary,
}

impl fmt::Display for JiraProjectScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Hive => "hive",
            Self::Apiary => "apiary",
        })
    }
}

impl FromStr for JiraProjectScope {
    type Err = ParseJiraProjectScopeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hive" => Ok(Self::Hive),
            "apiary" => Ok(Self::Apiary),
            _ => Err(ParseJiraProjectScopeError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseJiraProjectScopeError;

impl fmt::Display for ParseJiraProjectScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown Jira project scope")
    }
}

impl std::error::Error for ParseJiraProjectScopeError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JiraProjectBinding {
    pub id: JiraProjectBindingId,
    pub project_id: String,
    pub project_key: String,
    pub project_name: String,
    pub scope: JiraProjectScope,
    pub hive_id: HiveId,
    pub apiary_id: Option<ApiaryId>,
    pub access_verified: bool,
    pub workflow_mapped: bool,
    pub auto_sync_assigned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JiraStatusMapping {
    pub jira_status_id: String,
    pub jira_status_name: String,
    pub task_state: TaskState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JiraIssueLink {
    pub issue_id: String,
    pub issue_key: String,
    pub binding_id: JiraProjectBindingId,
    pub task_id: TaskId,
    pub jira_status_id: String,
    pub jira_status_name: String,
    pub jira_assignee_account_id: Option<String>,
    pub jira_assignee_name: Option<String>,
    pub remote_updated_at: String,
    pub last_synced_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraProjectReadiness {
    Ready,
    OfflineOwnedWorkOnly,
    ConnectionRequired,
    AccessRequired,
    WorkflowMappingRequired,
    ApiaryMembershipRequired,
}

impl JiraProjectBinding {
    /// Evaluates whether this Hive can synchronize or claim work without treating
    /// temporary network loss as an authorization failure.
    #[must_use]
    pub fn readiness(
        &self,
        connection: JiraConnectionState,
        hive_apiary_id: Option<ApiaryId>,
        already_owned: bool,
    ) -> JiraProjectReadiness {
        if self.scope == JiraProjectScope::Apiary
            && (self.apiary_id.is_none() || self.apiary_id != hive_apiary_id)
        {
            return JiraProjectReadiness::ApiaryMembershipRequired;
        }
        match connection {
            JiraConnectionState::NetworkUnavailable if already_owned => {
                return JiraProjectReadiness::OfflineOwnedWorkOnly;
            }
            JiraConnectionState::NotConnected | JiraConnectionState::NetworkUnavailable => {
                return JiraProjectReadiness::ConnectionRequired;
            }
            JiraConnectionState::CredentialsInvalid | JiraConnectionState::PermissionDenied => {
                return JiraProjectReadiness::AccessRequired;
            }
            JiraConnectionState::Ready => {}
        }
        if !self.access_verified {
            JiraProjectReadiness::AccessRequired
        } else if !self.workflow_mapped {
            JiraProjectReadiness::WorkflowMappingRequired
        } else {
            JiraProjectReadiness::Ready
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceMode {
    AtHive,
    Away,
    NightWatch,
}

/// Highest class of work Queen may continue without a new operator decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenAutonomyLevel {
    Advisory,
    Coordinate,
    LocalExecution,
}

impl fmt::Display for QueenAutonomyLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Advisory => "advisory",
            Self::Coordinate => "coordinate",
            Self::LocalExecution => "local_execution",
        })
    }
}

impl FromStr for QueenAutonomyLevel {
    type Err = ParseQueenAutonomyLevelError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "advisory" => Ok(Self::Advisory),
            "coordinate" => Ok(Self::Coordinate),
            "local_execution" => Ok(Self::LocalExecution),
            _ => Err(ParseQueenAutonomyLevelError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseQueenAutonomyLevelError;
impl fmt::Display for ParseQueenAutonomyLevelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown Queen autonomy level")
    }
}
impl std::error::Error for ParseQueenAutonomyLevelError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueenAutonomyPolicy {
    pub at_hive: QueenAutonomyLevel,
    pub away: QueenAutonomyLevel,
    pub night_watch: QueenAutonomyLevel,
}

/// One operator-created rule that Queen may consume during Night Watch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeploymentGrant {
    pub id: DeploymentGrantId,
    pub worker_id: WorkerId,
    pub worker_name: String,
    pub repository: String,
    pub environment: String,
    pub max_uses: u32,
    pub uses: u32,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeploymentAuthorization {
    pub id: DeploymentAuthorizationId,
    pub grant_id: DeploymentGrantId,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub environment: String,
    pub authorized_at: i64,
}

impl Default for QueenAutonomyPolicy {
    fn default() -> Self {
        Self {
            at_hive: QueenAutonomyLevel::Coordinate,
            away: QueenAutonomyLevel::Coordinate,
            night_watch: QueenAutonomyLevel::LocalExecution,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueenActionClass {
    Advise,
    Coordinate,
    ModifyWorkspace,
    ExternalSideEffect,
}

/// Durable lifecycle of one bounded, unattended Queen review.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenAutomationState {
    Idle,
    Queued,
    Delivering,
    Running,
    Completed,
    Uncertain,
}

impl fmt::Display for QueenAutomationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Queued => "queued",
            Self::Delivering => "delivering",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Uncertain => "uncertain",
        })
    }
}

impl FromStr for QueenAutomationState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "queued" => Ok(Self::Queued),
            "delivering" => Ok(Self::Delivering),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenAutomationTrigger {
    ActionableWork,
    Manual,
}

impl fmt::Display for QueenAutomationTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionableWork => "actionable_work",
            Self::Manual => "manual",
        })
    }
}

impl FromStr for QueenAutomationTrigger {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "actionable_work" => Ok(Self::ActionableWork),
            "manual" => Ok(Self::Manual),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenAutomationOutcome {
    Completed,
    NeedsOperator,
    NoAction,
}

impl fmt::Display for QueenAutomationOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Completed => "completed",
            Self::NeedsOperator => "needs_operator",
            Self::NoAction => "no_action",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueenAutomationStatus {
    pub enabled: bool,
    pub state: QueenAutomationState,
    pub run_id: Option<String>,
    pub trigger: Option<QueenAutomationTrigger>,
    pub actionable_count: usize,
    pub attempts: usize,
    pub requested_at: Option<i64>,
    pub delivered_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub outcome: Option<QueenAutomationOutcome>,
    pub waiting_reason: Option<String>,
}

impl QueenAutonomyPolicy {
    /// Applies the deterministic presence policy. External effects always require a
    /// separately recorded approval; model confidence never expands authority.
    #[must_use]
    pub const fn permits(
        self,
        presence: PresenceMode,
        action: QueenActionClass,
        explicit_external_approval: bool,
    ) -> bool {
        let level = match presence {
            PresenceMode::AtHive => self.at_hive,
            PresenceMode::Away => self.away,
            PresenceMode::NightWatch => self.night_watch,
        };
        match action {
            QueenActionClass::Advise => true,
            QueenActionClass::Coordinate => !matches!(level, QueenAutonomyLevel::Advisory),
            QueenActionClass::ModifyWorkspace => {
                matches!(level, QueenAutonomyLevel::LocalExecution)
            }
            QueenActionClass::ExternalSideEffect => {
                matches!(level, QueenAutonomyLevel::LocalExecution) && explicit_external_approval
            }
        }
    }
}

impl fmt::Display for PresenceMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AtHive => "at_hive",
            Self::Away => "away",
            Self::NightWatch => "night_watch",
        })
    }
}

impl FromStr for PresenceMode {
    type Err = ParsePresenceModeError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "at_hive" => Ok(Self::AtHive),
            "away" => Ok(Self::Away),
            "night_watch" => Ok(Self::NightWatch),
            _ => Err(ParsePresenceModeError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsePresenceModeError;
impl fmt::Display for ParsePresenceModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown operator presence mode")
    }
}
impl std::error::Error for ParsePresenceModeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceDeviceClass {
    Desktop,
    Mobile,
}
impl fmt::Display for PresenceDeviceClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Desktop => "desktop",
            Self::Mobile => "mobile",
        })
    }
}
impl FromStr for PresenceDeviceClass {
    type Err = ParsePresenceDeviceClassError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "desktop" => Ok(Self::Desktop),
            "mobile" => Ok(Self::Mobile),
            _ => Err(ParsePresenceDeviceClassError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsePresenceDeviceClassError;
impl fmt::Display for ParsePresenceDeviceClassError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown presence device class")
    }
}
impl std::error::Error for ParsePresenceDeviceClassError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceObservationState {
    Active,
    Idle,
    Locked,
    Hidden,
}
impl fmt::Display for PresenceObservationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Locked => "locked",
            Self::Hidden => "hidden",
        })
    }
}
impl FromStr for PresenceObservationState {
    type Err = ParsePresenceObservationStateError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "locked" => Ok(Self::Locked),
            "hidden" => Ok(Self::Hidden),
            _ => Err(ParsePresenceObservationStateError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsePresenceObservationStateError;
impl fmt::Display for ParsePresenceObservationStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown presence observation state")
    }
}
impl std::error::Error for ParsePresenceObservationStateError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceSource {
    Manual,
    ActiveDevice,
    ScreenLocked,
    InactiveDevice,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperatorPresence {
    pub mode: PresenceMode,
    pub manual_mode: Option<PresenceMode>,
    pub source: PresenceSource,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub id: DecisionRequestId,
    pub hive_id: HiveId,
    pub requesting_worker_id: WorkerId,
    pub task_id: Option<TaskId>,
    pub kind: DecisionRequestKind,
    pub urgency: DecisionUrgency,
    pub title: String,
    /// One or two sentences saying what the operator is deciding and what turns
    /// on it. Bounded hard, because the reason, risk and evidence around it are
    /// bounded at ten thousand characters each and routinely run to thousands —
    /// roughly five thousand characters to read before a decision can be made.
    #[serde(default)]
    pub summary: String,
    pub reason: String,
    pub risk: String,
    pub evidence: String,
    pub suggested_action: String,
    pub allowed_actions: Vec<String>,
    /// The one command this request asks to be allowed to run.
    ///
    /// READ BACK SO THE OPERATOR CAN SEE IT. It was stored and never returned,
    /// which left the grant button — "Allow the command shown in this request"
    /// — with no command shown. Approving a grant you cannot read is worse than
    /// having no grant mechanism at all, so this field is what makes the button
    /// safe to press rather than a detail of it.
    ///
    /// Defaulted for records written before the column existed.
    #[serde(default)]
    pub requested_command: Option<String>,
    pub deadline: Option<i64>,
    pub state: DecisionRequestState,
    /// Present when this record is an interview rather than a ruling. Empty
    /// means a ruling, and such records behave exactly as they did before
    /// interviews existed.
    #[serde(default)]
    pub questions: Vec<DecisionQuestion>,
    pub resolution_action: Option<String>,
    /// The operator's answers, keyed by question header. Empty for a ruling.
    #[serde(default)]
    pub resolution_answers: std::collections::BTreeMap<String, Vec<String>>,
    pub resolution_note: String,
    /// Which surface submitted the resolution, for diagnosis after the fact.
    /// Empty on records written before this was captured. Reported by the
    /// client, so it identifies where an answer came from; it is not evidence
    /// of who was allowed to give it.
    pub resolution_surface: String,
    pub resolved_by_operator_id: Option<OperatorId>,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub delivery_state: Option<DecisionDeliveryState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPolicy {
    ImportantOnly,
    AllDecisions,
    Off,
}

impl NotificationPolicy {
    #[must_use]
    pub fn allows(self, urgency: DecisionUrgency, presence: PresenceMode) -> bool {
        presence != PresenceMode::AtHive
            && match self {
                Self::ImportantOnly => matches!(urgency, DecisionUrgency::TimeSensitive),
                Self::AllDecisions => true,
                Self::Off => false,
            }
    }
}

impl fmt::Display for NotificationPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ImportantOnly => "important_only",
            Self::AllDecisions => "all_decisions",
            Self::Off => "off",
        })
    }
}

impl FromStr for NotificationPolicy {
    type Err = ParseNotificationPolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "important_only" => Ok(Self::ImportantOnly),
            "all_decisions" => Ok(Self::AllDecisions),
            "off" => Ok(Self::Off),
            _ => Err(ParseNotificationPolicyError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseNotificationPolicyError;

impl fmt::Display for ParseNotificationPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown notification policy")
    }
}

impl std::error::Error for ParseNotificationPolicyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn federation_retry_backoff_is_bounded() {
        let delays = (0..=8)
            .map(federation_retry_delay_seconds)
            .collect::<Vec<_>>();
        assert_eq!(delays, vec![5, 5, 15, 30, 60, 120, 300, 300, 300]);
    }

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
            description: "Coordinates this Hive.".into(),
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
            ephemeral: false,
            mark: None,
        };
        assert_eq!(profile.role, WorkerRole::Queen);
        assert!(profile.autostart);
    }

    #[test]
    fn task_transitions_are_explicit() {
        assert!(TaskState::Draft.can_transition_to(TaskState::Ready));
        assert!(TaskState::Review.can_transition_to(TaskState::Completed));
        // Sending work back to the queue is a normal outcome of review. It was
        // not permitted, so the only rejection path was Review -> Active, which
        // put the task where nobody was working it.
        assert!(TaskState::Review.can_transition_to(TaskState::Ready));
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
    fn apiary_join_readiness_names_every_unmet_boundary() {
        let operator_id = OperatorId::new();
        let hive = Hive::personal("Daisy", operator_id);
        let apiary = Apiary::new("Garden", OperatorId::new(), SharedWorkBackend::Jira);
        let invitation = ApiaryInvitation {
            id: ApiaryInvitationId::new(),
            apiary_id: apiary.id,
            invited_hive_id: hive.id,
            invited_by_operator_id: apiary.keeper_operator_id,
            state: ApiaryInvitationState::Pending,
            created_at: 10,
            expires_at: 100,
            resolved_at: None,
            required_policy_revision: 1,
            accepted_policy_revision: None,
            policy_accepted_at: None,
        };

        let blocked = ApiaryJoinReadiness::evaluate(
            &hive,
            &apiary,
            Some(&invitation),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Blocked,
                integration: ApiaryJoinCheckState::Blocked,
                project_access: ApiaryJoinCheckState::Blocked,
                protocol: ApiaryJoinCheckState::Blocked,
            },
            50,
        );
        assert_eq!(
            blocked.blockers(),
            [
                ApiaryJoinBlocker::IdentityNotVerified,
                ApiaryJoinBlocker::IntegrationNotReady,
                ApiaryJoinBlocker::ProjectAccessNotReady,
                ApiaryJoinBlocker::PolicyNotAccepted,
                ApiaryJoinBlocker::ProtocolMismatch,
            ]
        );

        let mut accepted_invitation = invitation;
        accepted_invitation.accepted_policy_revision = Some(1);
        accepted_invitation.policy_accepted_at = Some(40);
        let ready = ApiaryJoinReadiness::evaluate(
            &hive,
            &apiary,
            Some(&accepted_invitation),
            ApiaryJoinChecks {
                identity: ApiaryJoinCheckState::Ready,
                integration: ApiaryJoinCheckState::Ready,
                project_access: ApiaryJoinCheckState::Ready,
                protocol: ApiaryJoinCheckState::Ready,
            },
            50,
        );
        assert!(ready.can_join());
    }

    #[test]
    fn imported_invitation_readiness_requires_local_jira_policy_and_project_evidence() {
        let operator_id = OperatorId::new();
        let hive = Hive::personal("Daisy", operator_id);
        let invitation = FederationJoinInvitation {
            invitation_id: ApiaryInvitationId::new(),
            apiary_id: ApiaryId::new(),
            apiary_name: "Garden".into(),
            shared_work_backend: SharedWorkBackend::Jira,
            required_policy_revision: 3,
            promoted_project_catalog_digest: "digest".into(),
            promoted_projects: Vec::new(),
            keeper_node_id: FederationNodeId::new(),
            keeper_hive_id: HiveId::new(),
            keeper_hive_name: "Rose Hive".into(),
            keeper_operator_id: OperatorId::new(),
            keeper_operator_display_name: "Rosa".into(),
            keeper_endpoint: "https://keeper.example.test".into(),
            state: FederationJoinInvitationState::KeeperPinned,
            imported_at: 10,
            expires_at: 100,
        };
        let project = FederationProjectReadiness {
            project: FederationProjectManifestEntry {
                project_id: "10000".into(),
                project_key: "WWD".into(),
                project_name: "Website Development".into(),
            },
            binding_id: None,
            access_verified: false,
            workflow_mapped: false,
        };
        let blocked = FederationJoinReadiness::evaluate(
            &hive,
            &invitation,
            JiraConnectionState::NotConnected,
            vec![project.clone()],
            50,
        );
        assert_eq!(
            blocked.blockers,
            vec![
                ApiaryJoinBlocker::IntegrationNotReady,
                ApiaryJoinBlocker::ProjectAccessNotReady,
                ApiaryJoinBlocker::PolicyNotAccepted,
            ]
        );
        assert!(!blocked.can_submit());

        let ready = FederationJoinReadiness::evaluate(
            &hive,
            &FederationJoinInvitation {
                state: FederationJoinInvitationState::PolicyAccepted,
                ..invitation.clone()
            },
            JiraConnectionState::Ready,
            vec![FederationProjectReadiness {
                binding_id: Some(JiraProjectBindingId::new()),
                access_verified: true,
                workflow_mapped: true,
                ..project
            }],
            50,
        );
        assert!(ready.can_submit());
        let retry_ready = FederationJoinReadiness::evaluate(
            &hive,
            &FederationJoinInvitation {
                state: FederationJoinInvitationState::Submitted,
                ..invitation
            },
            JiraConnectionState::Ready,
            ready.projects,
            50,
        );
        assert!(retry_ready.can_submit());
    }

    #[test]
    fn apiary_join_requires_a_current_matching_invitation_and_personal_hive() {
        let operator_id = OperatorId::new();
        let mut hive = Hive::personal("Daisy", operator_id);
        let apiary = Apiary::new("Garden", OperatorId::new(), SharedWorkBackend::Jira);
        let invitation = ApiaryInvitation {
            id: ApiaryInvitationId::new(),
            apiary_id: apiary.id,
            invited_hive_id: hive.id,
            invited_by_operator_id: apiary.keeper_operator_id,
            state: ApiaryInvitationState::Pending,
            created_at: 10,
            expires_at: 20,
            resolved_at: None,
            required_policy_revision: 1,
            accepted_policy_revision: Some(1),
            policy_accepted_at: Some(12),
        };
        let checks = ApiaryJoinChecks {
            identity: ApiaryJoinCheckState::Ready,
            integration: ApiaryJoinCheckState::Ready,
            project_access: ApiaryJoinCheckState::Ready,
            protocol: ApiaryJoinCheckState::Ready,
        };

        assert_eq!(
            ApiaryJoinReadiness::evaluate(&hive, &apiary, Some(&invitation), checks, 20).blockers(),
            [ApiaryJoinBlocker::InvitationExpired]
        );
        hive.join(ApiaryId::new()).unwrap();
        assert_eq!(
            ApiaryJoinReadiness::evaluate(&hive, &apiary, None, checks, 15).blockers(),
            [
                ApiaryJoinBlocker::HiveAlreadyFederated,
                ApiaryJoinBlocker::InvitationRequired,
                ApiaryJoinBlocker::PolicyNotAccepted,
            ]
        );
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
    fn persisted_apiary_context_keeps_backend_and_local_role_explicit() {
        let keeper_id = OperatorId::new();
        let apiary = Apiary::persisted(
            ApiaryId::new(),
            "Garden",
            keeper_id,
            SharedWorkBackend::Jira,
            3,
        );
        let context = LocalApiaryContext::Federated {
            apiary: apiary.clone(),
            local_role: LocalApiaryRole::Keeper,
        };

        assert_eq!(apiary.shared_work_backend(), SharedWorkBackend::Jira);
        assert_eq!(apiary.policy_revision(), 3);
        assert!(matches!(
            context,
            LocalApiaryContext::Federated {
                local_role: LocalApiaryRole::Keeper,
                ..
            }
        ));
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

    #[test]
    fn queen_autonomy_is_presence_bounded_and_external_actions_fail_closed() {
        let policy = QueenAutonomyPolicy::default();
        assert!(policy.permits(PresenceMode::Away, QueenActionClass::Coordinate, false));
        assert!(!policy.permits(PresenceMode::Away, QueenActionClass::ModifyWorkspace, false));
        assert!(policy.permits(
            PresenceMode::NightWatch,
            QueenActionClass::ModifyWorkspace,
            false
        ));
        assert!(!policy.permits(
            PresenceMode::NightWatch,
            QueenActionClass::ExternalSideEffect,
            false
        ));
        assert!(policy.permits(
            PresenceMode::NightWatch,
            QueenActionClass::ExternalSideEffect,
            true
        ));

        let advisory = QueenAutonomyPolicy {
            at_hive: QueenAutonomyLevel::Advisory,
            away: QueenAutonomyLevel::Advisory,
            night_watch: QueenAutonomyLevel::Advisory,
        };
        assert!(advisory.permits(PresenceMode::AtHive, QueenActionClass::Advise, false));
        assert!(!advisory.permits(PresenceMode::AtHive, QueenActionClass::Coordinate, true));
    }

    #[test]
    fn jira_readiness_preserves_owned_offline_work_and_blocks_new_shared_claims() {
        let hive_id = HiveId::new();
        let apiary_id = ApiaryId::new();
        let binding = JiraProjectBinding {
            id: JiraProjectBindingId::new(),
            project_id: "10001".into(),
            project_key: "WEB".into(),
            project_name: "Website Services".into(),
            scope: JiraProjectScope::Apiary,
            hive_id,
            apiary_id: Some(apiary_id),
            access_verified: true,
            workflow_mapped: true,
            auto_sync_assigned: true,
        };
        assert_eq!(
            binding.readiness(JiraConnectionState::Ready, Some(apiary_id), false),
            JiraProjectReadiness::Ready
        );
        assert_eq!(
            binding.readiness(
                JiraConnectionState::NetworkUnavailable,
                Some(apiary_id),
                true
            ),
            JiraProjectReadiness::OfflineOwnedWorkOnly
        );
        assert_eq!(
            binding.readiness(
                JiraConnectionState::NetworkUnavailable,
                Some(apiary_id),
                false
            ),
            JiraProjectReadiness::ConnectionRequired
        );
        assert_eq!(
            binding.readiness(JiraConnectionState::Ready, None, false),
            JiraProjectReadiness::ApiaryMembershipRequired
        );
    }

    #[test]
    fn jira_readiness_distinguishes_access_from_workflow_configuration() {
        let hive_id = HiveId::new();
        let mut binding = JiraProjectBinding {
            id: JiraProjectBindingId::new(),
            project_id: "10002".into(),
            project_key: "OPS".into(),
            project_name: "Operations".into(),
            scope: JiraProjectScope::Hive,
            hive_id,
            apiary_id: None,
            access_verified: false,
            workflow_mapped: false,
            auto_sync_assigned: true,
        };
        assert_eq!(
            binding.readiness(JiraConnectionState::CredentialsInvalid, None, false),
            JiraProjectReadiness::AccessRequired
        );
        binding.access_verified = true;
        assert_eq!(
            binding.readiness(JiraConnectionState::Ready, None, false),
            JiraProjectReadiness::WorkflowMappingRequired
        );
    }
}

#[cfg(test)]
mod abandoned_state_tests {
    use crate::tasks::TaskState;

    #[test]
    fn every_unfinished_state_can_be_abandoned_directly() {
        // Directly, with no detour. Forcing a trip through Blocked to abandon
        // something would be clicking, which is what this state exists to delete.
        for from in [
            TaskState::Draft,
            TaskState::Ready,
            TaskState::Active,
            TaskState::Blocked,
            TaskState::Review,
        ] {
            assert!(
                from.can_transition_to(TaskState::Abandoned),
                "{from} should reach Abandoned directly"
            );
        }
    }

    #[test]
    fn abandoned_is_terminal_exactly_like_completed() {
        for target in [
            TaskState::Draft,
            TaskState::Ready,
            TaskState::Active,
            TaskState::Blocked,
            TaskState::Review,
            TaskState::Completed,
        ] {
            assert!(
                !TaskState::Abandoned.can_transition_to(target),
                "Abandoned must not reopen into {target}"
            );
        }
    }

    #[test]
    fn completed_and_abandoned_do_not_convert_into_each_other() {
        // Two different outcomes, not two spellings of one. Converting between
        // them would be a correction of the record, not a transition.
        assert!(!TaskState::Completed.can_transition_to(TaskState::Abandoned));
        assert!(!TaskState::Abandoned.can_transition_to(TaskState::Completed));
    }

    #[test]
    fn abandoned_survives_the_round_trip_through_text() {
        // It is stored as text and read back through FromStr, so a Display that
        // disagrees with the parser is a task that cannot be loaded.
        let parsed: TaskState = TaskState::Abandoned.to_string().parse().unwrap();
        assert_eq!(parsed, TaskState::Abandoned);
        assert_eq!(TaskState::Abandoned.to_string(), "abandoned");
    }
}

#[cfg(test)]
mod commit_settlement_tests {
    use crate::tasks::{
        CommitRepositoryState, CommitSettlement, CommitVerdict, TaskCommit, TaskCommitReport,
        TaskId, commit_settlement, documentation_path,
    };

    fn report(commits: Vec<TaskCommit>) -> TaskCommitReport {
        TaskCommitReport {
            task_id: TaskId::new(),
            workspace: "/workspace/petal".to_owned(),
            repository_state: CommitRepositoryState::Read,
            reported_at: 1_000,
            commits,
        }
    }

    fn commit(verdict: CommitVerdict, paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: "aaa1111".to_owned(),
            verdict,
            subject: "something".to_owned(),
            changed_paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn nobody_reporting_is_not_the_same_as_reporting_nothing() {
        // The distinction the previous task exists to preserve. If these ever
        // agree, work whose worker simply forgot closes itself.
        assert_eq!(commit_settlement(None), CommitSettlement::Unknown);
        assert_eq!(
            commit_settlement(Some(&report(vec![]))),
            CommitSettlement::NothingBuilt
        );
    }

    #[test]
    fn documentation_is_recognised_where_this_project_keeps_it() {
        for path in [
            "docs/41-verification.md",
            "README.md",
            "crates/swarm-api/docs/notes.md",
            "NOTICE",
            "CHANGELOG.md",
            "some/deep/path/notes.txt",
        ] {
            assert!(documentation_path(path), "{path} should be documentation");
        }
    }

    #[test]
    fn anything_unrecognised_is_code_because_that_error_is_the_cheap_one() {
        for path in [
            "crates/swarm-api/src/lib.rs",
            "web/src/App.tsx",
            "Cargo.toml",
            "packaging/linux/swarm-package",
            "docsomething/file.rs",
            ".github/workflows/ci.yml",
        ] {
            assert!(!documentation_path(path), "{path} must not read as docs");
        }
    }

    #[test]
    fn a_documentation_only_report_settles_and_a_mixed_one_does_not() {
        assert_eq!(
            commit_settlement(Some(&report(vec![
                commit(CommitVerdict::Present, &["docs/a.md"]),
                commit(CommitVerdict::Present, &["README.md", "docs/b.md"]),
            ]))),
            CommitSettlement::DocumentationOnly
        );
        assert_eq!(
            commit_settlement(Some(&report(vec![commit(
                CommitVerdict::Present,
                &["docs/a.md", "crates/swarm-api/src/lib.rs"],
            )]))),
            CommitSettlement::BuiltCode
        );
    }

    #[test]
    fn one_commit_nobody_could_check_leaves_the_whole_report_unsettled() {
        // A report is read as a set. One commit nobody looked at means the set
        // has not been established, whatever the others say.
        for unchecked in [
            CommitVerdict::Missing,
            CommitVerdict::Unreachable,
            CommitVerdict::Unchecked,
        ] {
            assert_eq!(
                commit_settlement(Some(&report(vec![
                    commit(CommitVerdict::Present, &["docs/a.md"]),
                    commit(unchecked, &["docs/b.md"]),
                ]))),
                CommitSettlement::Unknown,
                "{unchecked} must not settle"
            );
        }
    }

    /// THE MERGE COMMIT, which is the case vacuous truth gets wrong.
    ///
    /// `git show --name-only` summarises a merge as no paths at all, so "every
    /// path is documentation" is trivially true of a commit that may carry an
    /// entire release. Settling on that would close shipped work automatically.
    #[test]
    fn a_commit_reporting_no_paths_is_unknown_rather_than_documentation() {
        assert_eq!(
            commit_settlement(Some(&report(vec![commit(CommitVerdict::Present, &[])]))),
            CommitSettlement::Unknown
        );
    }
}
