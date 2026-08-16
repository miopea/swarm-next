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
domain_id!(ApiaryInvitationId);
domain_id!(ApiaryJoinLinkId);
domain_id!(FederationNodeId);
domain_id!(FederationMembershipReceiptId);
domain_id!(FederationDepartureReceiptId);
domain_id!(FederationClaimId);
domain_id!(FederationClaimHandoffId);
domain_id!(ApiaryTaskId);
domain_id!(FederationTaskCommandId);
domain_id!(StewardshipId);
domain_id!(ProviderConversationId);
domain_id!(PresenceDeviceId);
domain_id!(JiraProjectBindingId);

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

pub const FEDERATION_CONNECTION_CARD_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_INVITATION_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_MEMBERSHIP_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_DEPARTURE_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_CATALOG_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_TASK_FEED_SCHEMA_VERSION: u16 = 1;
pub const FEDERATION_PROTOCOL_VERSION: u16 = 1;

/// Public, signed identity material that one Hive can deliberately share with
/// a Keeper before any invitation or federation authority exists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveConnectionCardPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub node_id: FederationNodeId,
    pub hive_id: HiveId,
    pub hive_name: String,
    pub operator_id: OperatorId,
    pub operator_display_name: String,
    pub public_key: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveConnectionCard {
    pub payload: HiveConnectionCardPayload,
    pub signature: String,
}

/// A Keeper-created, short-lived bootstrap capability. The ordinary view never
/// contains its bearer secret; it only exposes public Apiary and bound-Hive
/// identity needed for explicit Keeper approval.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryJoinLinkState {
    Open,
    AwaitingApproval,
    Approved,
    InvitationIssued,
    Revoked,
    Expired,
}

impl fmt::Display for ApiaryJoinLinkState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Open => "open",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::InvitationIssued => "invitation_issued",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        })
    }
}

impl FromStr for ApiaryJoinLinkState {
    type Err = ParseApiaryJoinLinkStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "open" => Ok(Self::Open),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "invitation_issued" => Ok(Self::InvitationIssued),
            "revoked" => Ok(Self::Revoked),
            "expired" => Ok(Self::Expired),
            _ => Err(ParseApiaryJoinLinkStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseApiaryJoinLinkStateError;

impl fmt::Display for ParseApiaryJoinLinkStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid Apiary join link state")
    }
}

impl std::error::Error for ParseApiaryJoinLinkStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJoinLink {
    pub id: ApiaryJoinLinkId,
    pub apiary_id: ApiaryId,
    pub apiary_name: String,
    pub keeper_endpoint: String,
    pub state: ApiaryJoinLinkState,
    pub candidate: Option<ApiaryHiveCandidate>,
    pub issued_at: i64,
    pub expires_at: i64,
}

/// Sensitive creation result returned once to the Keeper browser. The secret
/// belongs in the URL fragment and is never included in ordinary link lists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJoinLinkBundle {
    pub link: ApiaryJoinLink,
    pub one_time_secret: String,
}

/// The member-visible result of one outbound Keeper poll. Invitation material
/// appears only after explicit Keeper approval and remains retry-stable while
/// the short-lived join capability is valid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJoinLinkPoll {
    pub link: ApiaryJoinLink,
    pub invitation: Option<ApiaryInvitationBundle>,
}

/// Public local record of one outbound Keeper connection. The bearer secret is
/// deliberately absent; it remains private in the member Hive database for
/// server-side polling across browser reloads and device changes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryKeeperLink {
    pub link_id: ApiaryJoinLinkId,
    pub keeper_endpoint: String,
    pub apiary_name: Option<String>,
    pub state: ApiaryJoinLinkState,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
}

/// A Keeper-pinned remote Hive identity. Pinning proves which public key is
/// expected for a Hive but grants no membership, invitation, or authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryHiveCandidate {
    pub apiary_id: ApiaryId,
    pub node_id: FederationNodeId,
    pub hive_id: HiveId,
    pub hive_name: String,
    pub operator_id: OperatorId,
    pub operator_display_name: String,
    pub public_key: String,
    pub card_issued_at: i64,
    pub card_expires_at: i64,
    pub pinned_by_operator_id: OperatorId,
    pub pinned_at: i64,
    pub last_verified_at: i64,
}

/// The signed, immutable invitation facts a Keeper gives to one specifically
/// pinned Hive. The bearer secret is deliberately outside this payload; only
/// its digest is retained by the Keeper.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryInvitationEnvelopePayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub invitation_id: ApiaryInvitationId,
    pub apiary_id: ApiaryId,
    pub apiary_name: String,
    pub shared_work_backend: SharedWorkBackend,
    pub required_policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_operator_id: OperatorId,
    pub invited_node_id: FederationNodeId,
    pub invited_hive_id: HiveId,
    pub invited_operator_id: OperatorId,
    pub keeper_endpoint: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub nonce: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryInvitationEnvelope {
    pub payload: ApiaryInvitationEnvelopePayload,
    pub signature: String,
}

/// Public Jira project identity carried in a signed-digest invitation manifest.
/// Access evidence, workflow mappings, credentials, and issue content remain
/// local to each Hive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationProjectManifestEntry {
    pub project_id: String,
    pub project_key: String,
    pub project_name: String,
}

/// Keeper-signed public project catalog for one authenticated member node.
/// Per-Hive Jira access, workflow mappings, credentials, and issue content are
/// deliberately absent; the recipient must acknowledge readiness locally.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationCatalogSnapshotPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub apiary_id: ApiaryId,
    pub policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub projects: Vec<FederationProjectManifestEntry>,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_operator_id: OperatorId,
    pub member_node_id: FederationNodeId,
    pub issued_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationCatalogSnapshot {
    pub payload: FederationCatalogSnapshotPayload,
    pub signature: String,
}

/// The canonical source of an Apiary-visible work item. Jira issue content is
/// never carried by the Swarm task feed; every Hive reads that work from Jira
/// with its own operator identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryTaskSource {
    Swarm,
}

/// Keeper-canonical shared work created inside Swarm. The home Hive is absent
/// until the Keeper or a governed claim assigns it. Worker assignment remains
/// private to the home Hive and therefore never appears here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryTask {
    pub id: ApiaryTaskId,
    pub apiary_id: ApiaryId,
    pub source: ApiaryTaskSource,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub state: TaskState,
    pub home_node_id: Option<FederationNodeId>,
    pub home_hive_id: Option<HiveId>,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One ordered Keeper event. Carrying the complete bounded task snapshot makes
/// member retries idempotent and permits deterministic projection repair.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryTaskEvent {
    pub sequence: i64,
    pub task: ApiaryTask,
}

/// A bounded task page addressed to one authenticated member node. `next_cursor`
/// is the largest event sequence included (or the requested cursor for an empty
/// page); `has_more` requires another immediate poll.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskPage {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub apiary_id: ApiaryId,
    pub member_node_id: FederationNodeId,
    pub events: Vec<ApiaryTaskEvent>,
    pub next_cursor: i64,
    pub has_more: bool,
    pub generated_at: i64,
}

/// Durable member-local evidence for the Keeper task projection. This is safe
/// for operator UI because it contains no task content or transport secrets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskSyncStatus {
    pub cursor: i64,
    pub task_count: usize,
    pub last_applied_at: Option<i64>,
}

/// A member-originated change to one Keeper-canonical Swarm task. The command
/// is revision checked and identified independently from transport retries.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationTaskCommandKind {
    Claim,
    Transition,
}

impl fmt::Display for FederationTaskCommandKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Claim => "claim",
            Self::Transition => "transition",
        })
    }
}

impl FromStr for FederationTaskCommandKind {
    type Err = ParseFederationTaskCommandKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claim" => Ok(Self::Claim),
            "transition" => Ok(Self::Transition),
            _ => Err(ParseFederationTaskCommandKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFederationTaskCommandKindError;

impl fmt::Display for ParseFederationTaskCommandKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid federation task command kind")
    }
}

impl std::error::Error for ParseFederationTaskCommandKindError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskCommand {
    pub id: FederationTaskCommandId,
    pub apiary_id: ApiaryId,
    pub task_id: ApiaryTaskId,
    pub expected_revision: u64,
    pub kind: FederationTaskCommandKind,
    pub target_state: Option<TaskState>,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationTaskCommandOutcome {
    Applied,
    Conflict,
    Rejected,
}

impl fmt::Display for FederationTaskCommandOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Applied => "applied",
            Self::Conflict => "conflict",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationTaskCommandOutcome {
    type Err = ParseFederationTaskCommandOutcomeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "applied" => Ok(Self::Applied),
            "conflict" => Ok(Self::Conflict),
            "rejected" => Ok(Self::Rejected),
            _ => Err(ParseFederationTaskCommandOutcomeError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFederationTaskCommandOutcomeError;

impl fmt::Display for ParseFederationTaskCommandOutcomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid federation task command outcome")
    }
}

impl std::error::Error for ParseFederationTaskCommandOutcomeError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskCommandReceipt {
    pub command_id: FederationTaskCommandId,
    pub outcome: FederationTaskCommandOutcome,
    pub task_revision: Option<u64>,
    pub processed_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationTaskOutboxState {
    Queued,
    Applied,
    Conflict,
    Rejected,
}

impl fmt::Display for FederationTaskOutboxState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Applied => "applied",
            Self::Conflict => "conflict",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationTaskOutboxState {
    type Err = ParseFederationTaskOutboxStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "applied" => Ok(Self::Applied),
            "conflict" => Ok(Self::Conflict),
            "rejected" => Ok(Self::Rejected),
            _ => Err(ParseFederationTaskOutboxStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFederationTaskOutboxStateError;

impl fmt::Display for ParseFederationTaskOutboxStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid federation task outbox state")
    }
}

impl std::error::Error for ParseFederationTaskOutboxStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskOutboxEntry {
    pub command: FederationTaskCommand,
    pub state: FederationTaskOutboxState,
    pub attempt_count: u32,
    pub last_attempt_at: Option<i64>,
    pub receipt: Option<FederationTaskCommandReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskOutboxStatus {
    pub queued_count: usize,
    pub conflict_count: usize,
    pub rejected_count: usize,
    pub last_attempt_at: Option<i64>,
}

/// Durable Member-side evidence that one exact Keeper catalog was verified.
/// This does not claim local Jira readiness or policy acceptance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationCatalogAcknowledgement {
    pub apiary_id: ApiaryId,
    pub policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub project_count: usize,
    pub snapshot_issued_at: i64,
    pub snapshot_expires_at: i64,
    pub acknowledged_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationCatalogBlocker {
    CatalogMissing,
    CatalogStale,
    IntegrationNotReady,
    PolicyRevisionChanged,
    ProjectAccessNotReady,
}

/// Member-local convergence state for the latest verified Keeper catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationCatalogReadiness {
    pub acknowledgement: Option<FederationCatalogAcknowledgement>,
    pub jira_connection: JiraConnectionState,
    pub projects: Vec<FederationProjectReadiness>,
    pub blockers: Vec<FederationCatalogBlocker>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationClaimState {
    Reserved,
    Confirmed,
    Released,
    Expired,
}

impl fmt::Display for FederationClaimState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Reserved => "reserved",
            Self::Confirmed => "confirmed",
            Self::Released => "released",
            Self::Expired => "expired",
        })
    }
}

impl FromStr for FederationClaimState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "confirmed" => Ok(Self::Confirmed),
            "released" => Ok(Self::Released),
            "expired" => Ok(Self::Expired),
            _ => Err(()),
        }
    }
}

/// Keeper-authoritative home-Hive claim for one Jira issue in one promoted
/// project. Issue content and Jira credentials remain local to each Hive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationSharedClaim {
    pub id: FederationClaimId,
    pub apiary_id: ApiaryId,
    pub project_id: String,
    pub issue_id: String,
    pub issue_key: String,
    pub home_node_id: FederationNodeId,
    pub home_hive_id: HiveId,
    pub home_operator_id: OperatorId,
    pub state: FederationClaimState,
    pub reserved_at: i64,
    pub reservation_expires_at: i64,
    pub confirmed_at: Option<i64>,
    pub released_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationClaimHandoffState {
    Offered,
    Accepted,
    Completed,
    Declined,
    Cancelled,
}

impl fmt::Display for FederationClaimHandoffState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Offered => "offered",
            Self::Accepted => "accepted",
            Self::Completed => "completed",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
        })
    }
}

impl FromStr for FederationClaimHandoffState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "offered" => Ok(Self::Offered),
            "accepted" => Ok(Self::Accepted),
            "completed" => Ok(Self::Completed),
            "declined" => Ok(Self::Declined),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// Keeper-authoritative transfer of one confirmed shared Jira claim. The
/// source remains the claim owner until the target confirms its local Jira
/// assignment succeeded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationClaimHandoff {
    pub id: FederationClaimHandoffId,
    pub apiary_id: ApiaryId,
    pub claim_id: FederationClaimId,
    pub project_id: String,
    pub issue_id: String,
    pub issue_key: String,
    pub source_node_id: FederationNodeId,
    pub source_hive_id: HiveId,
    pub source_operator_id: OperatorId,
    pub target_node_id: FederationNodeId,
    pub target_hive_id: HiveId,
    pub target_operator_id: OperatorId,
    pub state: FederationClaimHandoffState,
    pub reason: Option<String>,
    pub offered_at: i64,
    pub accepted_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub closed_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationSyncCondition {
    Idle,
    Current,
    Offline,
    AuthenticationRequired,
    Incompatible,
}

impl fmt::Display for FederationSyncCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "idle",
            Self::Current => "current",
            Self::Offline => "offline",
            Self::AuthenticationRequired => "authentication_required",
            Self::Incompatible => "incompatible",
        })
    }
}

impl FromStr for FederationSyncCondition {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "current" => Ok(Self::Current),
            "offline" => Ok(Self::Offline),
            "authentication_required" => Ok(Self::AuthenticationRequired),
            "incompatible" => Ok(Self::Incompatible),
            _ => Err(()),
        }
    }
}

/// Content-free Member-side evidence for the bounded Keeper reconciliation
/// loop. It never contains endpoints, credentials, Jira data, or response
/// bodies.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationSyncHealth {
    pub condition: FederationSyncCondition,
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub consecutive_failures: u32,
    pub next_attempt_at: Option<i64>,
}

impl Default for FederationSyncHealth {
    fn default() -> Self {
        Self {
            condition: FederationSyncCondition::Idle,
            last_attempt_at: None,
            last_success_at: None,
            consecutive_failures: 0,
            next_attempt_at: None,
        }
    }
}

/// Returns a deterministic bounded retry delay for temporary federation
/// outages. The first retry is quick, then backs off to a five-minute ceiling.
#[must_use]
pub const fn federation_retry_delay_seconds(consecutive_failures: u32) -> i64 {
    match consecutive_failures {
        0 | 1 => 5,
        2 => 15,
        3 => 30,
        4 => 60,
        5 => 120,
        _ => 300,
    }
}

impl FederationCatalogReadiness {
    #[must_use]
    pub fn evaluate(
        acknowledgement: Option<FederationCatalogAcknowledgement>,
        local_policy_revision: u64,
        jira_connection: JiraConnectionState,
        projects: Vec<FederationProjectReadiness>,
        now: i64,
    ) -> Self {
        let mut blockers = Vec::new();
        match &acknowledgement {
            None => blockers.push(FederationCatalogBlocker::CatalogMissing),
            Some(catalog) => {
                if catalog.snapshot_expires_at <= now {
                    blockers.push(FederationCatalogBlocker::CatalogStale);
                }
                if catalog.policy_revision != local_policy_revision {
                    blockers.push(FederationCatalogBlocker::PolicyRevisionChanged);
                }
            }
        }
        if jira_connection != JiraConnectionState::Ready {
            blockers.push(FederationCatalogBlocker::IntegrationNotReady);
        }
        if projects.iter().any(|project| !project.is_ready()) {
            blockers.push(FederationCatalogBlocker::ProjectAccessNotReady);
        }
        Self {
            acknowledgement,
            jira_connection,
            projects,
            blockers,
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// A one-time handoff bundle. The secret is shown only in this response and is
/// never recoverable from Keeper storage. The Keeper card lets the invited
/// operator deliberately pin the signing identity before accepting policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryInvitationBundle {
    pub keeper_connection_card: HiveConnectionCard,
    pub invitation: ApiaryInvitationEnvelope,
    pub promoted_projects: Vec<FederationProjectManifestEntry>,
    pub one_time_secret: String,
}

/// A signed assertion from the invited Hive that its private preflight passed
/// for the exact invitation, policy revision, and promoted-project catalog.
/// The bearer secret is transported separately and is never part of the
/// signed payload or an ordinary response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationJoinSubmissionPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub invitation_id: ApiaryInvitationId,
    pub apiary_id: ApiaryId,
    pub required_policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub invited_node_id: FederationNodeId,
    pub invited_hive_id: HiveId,
    pub invited_operator_id: OperatorId,
    pub submitted_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationJoinSubmission {
    pub payload: FederationJoinSubmissionPayload,
    pub signature: String,
    pub one_time_secret: String,
}

/// Keeper-signed proof that one invitation was consumed and one remote Hive
/// became a member. The separately returned node credential is bounded and
/// adapter-private; ordinary membership reads expose only this receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationMembershipReceiptPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub receipt_id: FederationMembershipReceiptId,
    pub invitation_id: ApiaryInvitationId,
    pub apiary_id: ApiaryId,
    pub apiary_name: String,
    pub shared_work_backend: SharedWorkBackend,
    pub policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_operator_id: OperatorId,
    pub member_node_id: FederationNodeId,
    pub member_hive_id: HiveId,
    pub member_operator_id: OperatorId,
    pub joined_at: i64,
    pub credential_expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationMembershipReceipt {
    pub payload: FederationMembershipReceiptPayload,
    pub signature: String,
}

/// Sensitive handshake response. The credential is returned only to the
/// authenticated invited node and must never enter browser diagnostics or
/// general activity streams.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationJoinAcceptance {
    pub receipt: FederationMembershipReceipt,
    pub node_credential: String,
}

/// Content-free evidence that one Member Hive can leave without abandoning
/// Apiary-owned work or an unresolved outbound side effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDepartureReadiness {
    pub apiary_id: ApiaryId,
    pub member_node_id: FederationNodeId,
    pub member_hive_id: HiveId,
    pub active_jira_claim_count: usize,
    pub open_swarm_task_count: usize,
    pub active_stewardship_count: usize,
    pub pending_task_command_count: usize,
    pub pending_jira_claim_count: usize,
}

/// Browser-safe local progress for an explicit Member departure. `Departing`
/// means shared mutations are frozen while the same Keeper request is retried;
/// it does not mean membership has been partially removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationDepartureState {
    Active,
    Departing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDepartureOverview {
    pub state: FederationDepartureState,
    pub readiness: FederationDepartureReadiness,
}

impl FederationDepartureReadiness {
    #[must_use]
    pub const fn can_leave(self) -> bool {
        self.active_jira_claim_count == 0
            && self.open_swarm_task_count == 0
            && self.active_stewardship_count == 0
            && self.pending_task_command_count == 0
            && self.pending_jira_claim_count == 0
    }

    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        Self {
            apiary_id: self.apiary_id,
            member_node_id: self.member_node_id,
            member_hive_id: self.member_hive_id,
            active_jira_claim_count: self
                .active_jira_claim_count
                .saturating_add(other.active_jira_claim_count),
            open_swarm_task_count: self
                .open_swarm_task_count
                .saturating_add(other.open_swarm_task_count),
            active_stewardship_count: self
                .active_stewardship_count
                .saturating_add(other.active_stewardship_count),
            pending_task_command_count: self
                .pending_task_command_count
                .saturating_add(other.pending_task_command_count),
            pending_jira_claim_count: self
                .pending_jira_claim_count
                .saturating_add(other.pending_jira_claim_count),
        }
    }
}

/// Keeper-signed, retry-stable evidence that one exact membership was ended.
/// The Member stores this audit receipt before removing its local credential
/// and shared projections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDepartureReceiptPayload {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub receipt_id: FederationDepartureReceiptId,
    pub membership_receipt_id: FederationMembershipReceiptId,
    pub apiary_id: ApiaryId,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_operator_id: OperatorId,
    pub member_node_id: FederationNodeId,
    pub member_hive_id: HiveId,
    pub member_operator_id: OperatorId,
    pub departed_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationDepartureReceipt {
    pub payload: FederationDepartureReceiptPayload,
    pub signature: String,
}

/// Host-private transport material for a joined Member Hive. This value may
/// cross application and adapter boundaries in process, but must never be
/// serialized into browser, diagnostics, activity, or agent responses.
#[derive(Clone, Eq, PartialEq)]
pub struct FederationMemberConnection {
    pub keeper_endpoint: String,
    pub node_credential: String,
    pub credential_expires_at: i64,
}

/// Durable invited-Hive view of a signed invitation. Sensitive bearer material
/// and the complete signed envelope remain private to persistence and are never
/// returned through ordinary application or browser reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationJoinInvitationState {
    KeeperPinned,
    PolicyAccepted,
    Submitted,
    Consumed,
    Revoked,
    Expired,
}

impl fmt::Display for FederationJoinInvitationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::KeeperPinned => "keeper_pinned",
            Self::PolicyAccepted => "policy_accepted",
            Self::Submitted => "submitted",
            Self::Consumed => "consumed",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        })
    }
}

impl FromStr for FederationJoinInvitationState {
    type Err = ParseFederationJoinInvitationStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "keeper_pinned" => Ok(Self::KeeperPinned),
            "policy_accepted" => Ok(Self::PolicyAccepted),
            "submitted" => Ok(Self::Submitted),
            "consumed" => Ok(Self::Consumed),
            "revoked" => Ok(Self::Revoked),
            "expired" => Ok(Self::Expired),
            _ => Err(ParseFederationJoinInvitationStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFederationJoinInvitationStateError;

impl fmt::Display for ParseFederationJoinInvitationStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid federation join invitation state")
    }
}

impl std::error::Error for ParseFederationJoinInvitationStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationJoinInvitation {
    pub invitation_id: ApiaryInvitationId,
    pub apiary_id: ApiaryId,
    pub apiary_name: String,
    pub shared_work_backend: SharedWorkBackend,
    pub required_policy_revision: u64,
    pub promoted_project_catalog_digest: String,
    pub promoted_projects: Vec<FederationProjectManifestEntry>,
    pub keeper_node_id: FederationNodeId,
    pub keeper_hive_id: HiveId,
    pub keeper_hive_name: String,
    pub keeper_operator_id: OperatorId,
    pub keeper_operator_display_name: String,
    pub keeper_endpoint: String,
    pub state: FederationJoinInvitationState,
    pub imported_at: i64,
    pub expires_at: i64,
}

/// Local evidence for one signed Jira project in an imported invitation.
/// The Keeper supplies only project identity; credentials and mappings remain
/// private to this Hive.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationProjectReadiness {
    pub project: FederationProjectManifestEntry,
    pub binding_id: Option<JiraProjectBindingId>,
    pub access_verified: bool,
    pub workflow_mapped: bool,
}

impl FederationProjectReadiness {
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.binding_id.is_some() && self.access_verified && self.workflow_mapped
    }
}

/// Server-derived preflight for an imported invitation. This is intentionally
/// separate from membership: clearing every blocker means the Hive may submit
/// the one-time handshake, not that it has joined.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationJoinReadiness {
    pub jira_connection: JiraConnectionState,
    pub projects: Vec<FederationProjectReadiness>,
    pub blockers: Vec<ApiaryJoinBlocker>,
}

impl FederationJoinReadiness {
    #[must_use]
    pub fn evaluate(
        hive: &Hive,
        invitation: &FederationJoinInvitation,
        jira_connection: JiraConnectionState,
        projects: Vec<FederationProjectReadiness>,
        now: i64,
    ) -> Self {
        let mut blockers = Vec::new();
        if hive.apiary_id.is_some() {
            blockers.push(ApiaryJoinBlocker::HiveAlreadyFederated);
        }
        if invitation.expires_at <= now {
            blockers.push(ApiaryJoinBlocker::InvitationExpired);
        }
        if jira_connection != JiraConnectionState::Ready {
            blockers.push(ApiaryJoinBlocker::IntegrationNotReady);
        }
        if projects.iter().any(|project| !project.is_ready()) {
            blockers.push(ApiaryJoinBlocker::ProjectAccessNotReady);
        }
        if !matches!(
            invitation.state,
            FederationJoinInvitationState::PolicyAccepted
                | FederationJoinInvitationState::Submitted
        ) {
            blockers.push(ApiaryJoinBlocker::PolicyNotAccepted);
        }
        Self {
            jira_connection,
            projects,
            blockers,
        }
    }

    #[must_use]
    pub fn can_submit(&self) -> bool {
        self.blockers.is_empty()
    }
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
    policy_revision: u64,
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
            policy_revision: 1,
        }
    }

    #[must_use]
    pub const fn shared_work_backend(&self) -> SharedWorkBackend {
        self.shared_work_backend
    }

    #[must_use]
    pub const fn policy_revision(&self) -> u64 {
        self.policy_revision
    }

    /// Reconstitutes an Apiary whose immutable identity and backend were loaded
    /// from durable storage.
    #[must_use]
    pub fn persisted(
        id: ApiaryId,
        name: impl Into<String>,
        keeper_operator_id: OperatorId,
        shared_work_backend: SharedWorkBackend,
        policy_revision: u64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            keeper_operator_id,
            shared_work_backend,
            policy_revision,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalApiaryRole {
    Keeper,
    Member,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum LocalApiaryContext {
    Personal,
    Federated {
        apiary: Apiary,
        local_role: LocalApiaryRole,
    },
}

/// Public identity shown in an Apiary roster. This intentionally carries no
/// federation credential, signed receipt, repository, task, or presence data.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryMemberSummary {
    pub hive_id: HiveId,
    pub hive_name: String,
    pub operator_id: OperatorId,
    pub operator_display_name: String,
    pub role: LocalApiaryRole,
    pub is_local: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryInvitationState {
    Pending,
    Accepted,
    Revoked,
    Expired,
}

impl fmt::Display for ApiaryInvitationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        })
    }
}

impl FromStr for ApiaryInvitationState {
    type Err = ParseApiaryInvitationStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "revoked" => Ok(Self::Revoked),
            "expired" => Ok(Self::Expired),
            _ => Err(ParseApiaryInvitationStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseApiaryInvitationStateError;

impl fmt::Display for ParseApiaryInvitationStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown Apiary invitation state")
    }
}

impl std::error::Error for ParseApiaryInvitationStateError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryInvitation {
    pub id: ApiaryInvitationId,
    pub apiary_id: ApiaryId,
    pub invited_hive_id: HiveId,
    pub invited_by_operator_id: OperatorId,
    pub state: ApiaryInvitationState,
    pub created_at: i64,
    pub expires_at: i64,
    pub resolved_at: Option<i64>,
    pub required_policy_revision: u64,
    pub accepted_policy_revision: Option<u64>,
    pub policy_accepted_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJiraProject {
    pub apiary_id: ApiaryId,
    pub project_id: String,
    pub project_key: String,
    pub project_name: String,
    pub promoted_by_operator_id: OperatorId,
    pub promoted_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryJoinBlocker {
    HiveAlreadyFederated,
    InvitationRequired,
    InvitationExpired,
    IdentityNotVerified,
    IntegrationNotReady,
    ProjectAccessNotReady,
    PolicyNotAccepted,
    ProtocolMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiaryJoinCheckState {
    Ready,
    Blocked,
}

impl ApiaryJoinCheckState {
    const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiaryJoinChecks {
    pub identity: ApiaryJoinCheckState,
    pub integration: ApiaryJoinCheckState,
    pub project_access: ApiaryJoinCheckState,
    pub protocol: ApiaryJoinCheckState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJoinReadiness {
    apiary_id: ApiaryId,
    hive_id: HiveId,
    blockers: Vec<ApiaryJoinBlocker>,
}

impl ApiaryJoinReadiness {
    #[must_use]
    pub fn evaluate(
        hive: &Hive,
        apiary: &Apiary,
        invitation: Option<&ApiaryInvitation>,
        checks: ApiaryJoinChecks,
        now: i64,
    ) -> Self {
        let mut blockers = Vec::new();
        if hive.apiary_id.is_some() {
            blockers.push(ApiaryJoinBlocker::HiveAlreadyFederated);
        }
        match invitation.filter(|candidate| {
            candidate.apiary_id == apiary.id
                && candidate.invited_hive_id == hive.id
                && candidate.state == ApiaryInvitationState::Pending
        }) {
            None => blockers.push(ApiaryJoinBlocker::InvitationRequired),
            Some(invitation) if invitation.expires_at <= now => {
                blockers.push(ApiaryJoinBlocker::InvitationExpired);
            }
            Some(_) => {}
        }
        let policy_ready = invitation.is_some_and(|candidate| {
            candidate.required_policy_revision == apiary.policy_revision()
                && candidate.accepted_policy_revision == Some(apiary.policy_revision())
                && candidate.policy_accepted_at.is_some()
        });
        for (ready, blocker) in [
            (
                checks.identity.is_ready(),
                ApiaryJoinBlocker::IdentityNotVerified,
            ),
            (
                checks.integration.is_ready(),
                ApiaryJoinBlocker::IntegrationNotReady,
            ),
            (
                checks.project_access.is_ready(),
                ApiaryJoinBlocker::ProjectAccessNotReady,
            ),
            (policy_ready, ApiaryJoinBlocker::PolicyNotAccepted),
            (
                checks.protocol.is_ready(),
                ApiaryJoinBlocker::ProtocolMismatch,
            ),
        ] {
            if !ready {
                blockers.push(blocker);
            }
        }
        Self {
            apiary_id: apiary.id,
            hive_id: hive.id,
            blockers,
        }
    }

    #[must_use]
    pub fn can_join(&self) -> bool {
        self.blockers.is_empty()
    }

    #[must_use]
    pub const fn apiary_id(&self) -> ApiaryId {
        self.apiary_id
    }

    #[must_use]
    pub const fn hive_id(&self) -> HiveId {
        self.hive_id
    }

    #[must_use]
    pub fn blockers(&self) -> &[ApiaryJoinBlocker] {
        &self.blockers
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

impl fmt::Display for StewardCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Observe => "observe",
            Self::Assign => "assign",
            Self::Assist => "assist",
            Self::Takeover => "takeover",
            Self::ManageProjects => "manage_projects",
            Self::ManageMembers => "manage_members",
        })
    }
}

impl FromStr for StewardCapability {
    type Err = ParseStewardCapabilityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "observe" => Ok(Self::Observe),
            "assign" => Ok(Self::Assign),
            "assist" => Ok(Self::Assist),
            "takeover" => Ok(Self::Takeover),
            "manage_projects" => Ok(Self::ManageProjects),
            "manage_members" => Ok(Self::ManageMembers),
            _ => Err(ParseStewardCapabilityError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseStewardCapabilityError;

impl fmt::Display for ParseStewardCapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown Steward capability")
    }
}

impl std::error::Error for ParseStewardCapabilityError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stewardship {
    pub id: StewardshipId,
    pub apiary_id: ApiaryId,
    pub steward_operator_id: OperatorId,
    pub managed_hive_ids: Vec<HiveId>,
    pub capabilities: Vec<StewardCapability>,
}

pub const FEDERATION_STEWARDSHIP_SCHEMA_VERSION: u16 = 1;

/// One bounded Keeper response describing only the authenticated Member
/// operator's current Steward delegation. Worker, repository, terminal, Jira,
/// credential, and task content are deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardshipSnapshot {
    pub schema_version: u16,
    pub protocol_version: u16,
    pub apiary_id: ApiaryId,
    pub member_node_id: FederationNodeId,
    pub member_operator_id: OperatorId,
    pub stewardship: Option<Stewardship>,
    pub generated_at: i64,
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
    /// Operator-reviewed routing context describing what this worker owns.
    pub description: String,
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
    Resting,
    Buzzing,
    WithOperator,
    AwaitingOperator,
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
    pub assigned_worker_id: Option<WorkerId>,
    pub assigned_session_id: Option<WorkerSessionId>,
    pub dispatch_state: Option<TaskDispatchState>,
    pub outcome_delivery_state: Option<TaskOutcomeDeliveryState>,
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
            blocked.blockers,
            vec![
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
            ApiaryJoinReadiness::evaluate(&hive, &apiary, Some(&invitation), checks, 20).blockers,
            vec![ApiaryJoinBlocker::InvitationExpired]
        );
        hive.join(ApiaryId::new()).unwrap();
        assert_eq!(
            ApiaryJoinReadiness::evaluate(&hive, &apiary, None, checks, 15).blockers,
            vec![
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
