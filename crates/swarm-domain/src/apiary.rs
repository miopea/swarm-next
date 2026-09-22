//! Optional Hive federation: membership, scoped stewardship, the shared project
//! catalog, atomic claims, and cross-Hive routing.
//!
//! Grouped as one module because the target architecture names one `apiary`
//! module for all of it, and because these types are only meaningful together:
//! a claim without a membership, or a stewardship without the Hive it scopes,
//! does not describe anything.

use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

use crate::{
    ApiaryId, ApiaryInvitationId, ApiaryJoinLinkId, ApiaryTaskId, FederationClaimHandoffId,
    FederationClaimId, FederationDepartureReceiptId, FederationMembershipReceiptId,
    FederationNodeId, FederationStewardAssistCommandId, FederationStewardAssistRequestId,
    FederationStewardTakeoverCommandId, FederationStewardTakeoverLeaseId,
    FederationStewardTaskCommandId, FederationTaskCommandId, HiveId, JiraConnectionState,
    JiraProjectBindingId, OperatorId, SharedWorkBackend, StewardshipId, TaskId, TaskPriority,
    TaskState, WorkerId,
};

mod enrollment;
pub use enrollment::*;

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
/// A membership-only join assertion. Jira readiness is separate from joining.
/// Membership receipts retain their existing independent schema.
pub const FEDERATION_PROJECT_SCOPED_JOIN_SCHEMA_VERSION: u16 = 2;
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
    /// This exact invitation was consumed; delivery alone is not membership.
    #[serde(default)]
    pub membership_confirmed: bool,
    pub issued_at: i64,
    pub expires_at: i64,
}

/// Sensitive creation result returned once to the Keeper browser. The secret
/// belongs in the URL fragment and is never included in ordinary link lists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryJoinLinkBundle {
    pub link: ApiaryJoinLink,
    pub one_time_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrollment_offer: Option<ApiaryEnrollmentOffer>,
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
/// What a Hive is standing on when it says shared work is finished.
///
/// ⚠️ THIS TRAVELS WITH THE CLOSURE OR THE CLOSURE IS AN ASSERTION. The member
/// closes and Keeper mirrors; a dependent Hive elsewhere then resumes on that
/// conclusion. If the evidence stays on the closing Hive, every other Hive is
/// taking its word — and the task that chose this design named the residual
/// risk plainly: one Hive's closure unblocks dependents with no second check,
/// so a wrong no-deployment claim propagates. Mirroring the evidence makes that
/// RECOVERABLE, not impossible, and that is the whole value.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApiaryClosureEvidence {
    /// Commit SHAs the closing Hive reported, as it reported them.
    #[serde(default)]
    pub commits: Vec<String>,
    /// Deployment references recorded against the work.
    #[serde(default)]
    pub deployments: Vec<String>,
    /// The approved reason nothing shipped, when that is the claim.
    ///
    /// ⚠️ EMPTY COMMITS AND NO REASON IS ITSELF LEGIBLE. A reader can tell
    /// "closed with nothing behind it" from "closed with a reason somebody
    /// approved", which is exactly the distinction a dependent Hive needs and
    /// cannot make from a state alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_deployment_reason: Option<String>,
}

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
    /// What the closing Hive stood on, mirrored from it.
    ///
    /// `None` until the work closes. Defaulted so a Hive running an older build
    /// still reads the rest of the task rather than failing the whole feed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure_evidence: Option<ApiaryClosureEvidence>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The private execution bridge for one Keeper-canonical task in its home
/// Hive. This record never leaves the home Hive: Keeper can see ownership by
/// Hive, but not the selected worker, repository, terminal, or provider.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalApiaryTaskExecution {
    pub apiary_task_id: ApiaryTaskId,
    pub local_task_id: TaskId,
    pub worker_id: WorkerId,
    pub state: TaskState,
    pub created_at: i64,
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
    /// A member asking Keeper to put cross-Hive work on the shared board.
    ///
    /// ⚠️ THE ONE THE MOTIVATING FAILURE NEEDED. Before this, a member had no
    /// way to reach the shared board at all — relocation is Keeper-only — so
    /// work filed against another Hive's repository landed as a local draft
    /// nobody who could act on it would ever see.
    File,
}

impl fmt::Display for FederationTaskCommandKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Claim => "claim",
            Self::Transition => "transition",
            Self::File => "file",
        })
    }
}

impl FromStr for FederationTaskCommandKind {
    type Err = ParseFederationTaskCommandKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claim" => Ok(Self::Claim),
            "transition" => Ok(Self::Transition),
            "file" => Ok(Self::File),
            // ⚠️ AN UNKNOWN KIND IS REFUSED, NOT IGNORED. This is what lets a
            // new command kind be added without a protocol break: an older
            // Keeper rejects `file` outright rather than applying some other
            // part of the command and dropping the bit it did not understand.
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

/// What a member is asking Keeper to put on the shared board.
///
/// ⚠️ IT NAMES A REPOSITORY, NOT A HIVE. Members do not decide routing —
/// Keeper holds the fleet's capability reports and resolves who owns the repo.
/// A member choosing the destination would mean two Hives could disagree about
/// who owns something, with no authority between them; it would also go stale
/// the moment a repo moved.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskFiling {
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    /// The git remote, matched against what each Hive reports it can work in.
    pub repository: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationTaskCommand {
    pub id: FederationTaskCommandId,
    pub apiary_id: ApiaryId,
    pub task_id: ApiaryTaskId,
    pub expected_revision: u64,
    pub kind: FederationTaskCommandKind,
    pub target_state: Option<TaskState>,
    /// Present only on a `File` command.
    ///
    /// ⚠️ DEFAULTED SO AN OLDER NODE STILL PARSES THE REST, but an older Keeper
    /// cannot be fooled by one: it does not know the `file` KIND, so the
    /// command is rejected outright rather than applied with the filing
    /// silently dropped. Failing closed on the kind is what makes this field
    /// safe to add without a protocol break.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filing: Option<FederationTaskFiling>,
    /// What the closing Hive stood on, sent WITH the closure.
    ///
    /// ⚠️ ON THE SAME COMMAND ON PURPOSE. Sending evidence separately would
    /// leave a window where a dependent Hive sees the closure and not what is
    /// behind it, and that window is exactly when it resumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure: Option<ApiaryClosureEvidence>,
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

/// One Steward-originated request to create Keeper-canonical coordination work
/// for an explicitly managed Hive. The target Hive chooses its own private
/// worker and repository after receiving the task through the normal feed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTaskCommand {
    pub id: FederationStewardTaskCommandId,
    pub apiary_id: ApiaryId,
    pub target_hive_id: HiveId,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTaskOutcome {
    Applied,
    Rejected,
}

impl fmt::Display for FederationStewardTaskOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationStewardTaskOutcome {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            _ => Err(()),
        }
    }
}

/// Retry-stable Keeper evidence for one guarded Steward task command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTaskReceipt {
    pub command_id: FederationStewardTaskCommandId,
    pub outcome: FederationStewardTaskOutcome,
    pub stewardship_id: Option<StewardshipId>,
    pub task: Option<ApiaryTask>,
    pub processed_at: i64,
}

/// Keeper-visible, bounded audit evidence for one Steward task command. It
/// contains only public Apiary identity and Keeper-owned shared-work metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTaskAuditEntry {
    pub command_id: FederationStewardTaskCommandId,
    pub member_hive_id: HiveId,
    pub member_operator_id: OperatorId,
    pub target_hive_id: HiveId,
    pub stewardship_id: Option<StewardshipId>,
    pub task_id: Option<ApiaryTaskId>,
    pub title: String,
    pub priority: TaskPriority,
    pub outcome: FederationStewardTaskOutcome,
    pub processed_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTaskOutboxState {
    Queued,
    Applied,
    Rejected,
}

impl fmt::Display for FederationStewardTaskOutboxState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationStewardTaskOutboxState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTaskOutboxEntry {
    pub command: FederationStewardTaskCommand,
    pub state: FederationStewardTaskOutboxState,
    pub attempt_count: u32,
    pub last_attempt_at: Option<i64>,
    pub receipt: Option<FederationStewardTaskReceipt>,
}

/// One deliberately structured Steward request. It is addressed to the target
/// Hive's operator and Queen, never injected into a worker terminal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistRequest {
    pub id: FederationStewardAssistRequestId,
    pub apiary_id: ApiaryId,
    pub source_hive_id: HiveId,
    pub target_hive_id: HiveId,
    pub message: String,
    pub state: FederationStewardAssistState,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardAssistState {
    Pending,
    Accepted,
    Declined,
}

impl fmt::Display for FederationStewardAssistState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
        })
    }
}

impl FromStr for FederationStewardAssistState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "declined" => Ok(Self::Declined),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FederationStewardAssistAction {
    Request {
        target_hive_id: HiveId,
        message: String,
    },
    Respond {
        request_id: FederationStewardAssistRequestId,
        decision: FederationStewardAssistState,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistCommand {
    pub id: FederationStewardAssistCommandId,
    pub apiary_id: ApiaryId,
    pub action: FederationStewardAssistAction,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardAssistOutcome {
    Applied,
    Rejected,
}

impl fmt::Display for FederationStewardAssistOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationStewardAssistOutcome {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistReceipt {
    pub command_id: FederationStewardAssistCommandId,
    pub outcome: FederationStewardAssistOutcome,
    pub stewardship_id: Option<StewardshipId>,
    pub request: Option<FederationStewardAssistRequest>,
    pub processed_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardAssistOutboxState {
    Queued,
    Applied,
    Rejected,
}

impl fmt::Display for FederationStewardAssistOutboxState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        })
    }
}

impl FromStr for FederationStewardAssistOutboxState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistOutboxEntry {
    pub command: FederationStewardAssistCommand,
    pub state: FederationStewardAssistOutboxState,
    pub attempt_count: u32,
    pub last_attempt_at: Option<i64>,
    pub receipt: Option<FederationStewardAssistReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistInbox {
    pub requests: Vec<FederationStewardAssistRequest>,
    pub generated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardAssistLocalState {
    pub incoming: Vec<FederationStewardAssistRequest>,
    pub sent: Vec<FederationStewardAssistRequest>,
    pub outbox: Vec<FederationStewardAssistOutboxEntry>,
}

/// Keeper-authoritative lifecycle of one exclusive Steward control lease over
/// a managed Hive's Queen. Requested leases grant no terminal access.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTakeoverState {
    Requested,
    Active,
    Released,
    Reclaimed,
    Expired,
}

impl FederationStewardTakeoverState {
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Requested | Self::Active)
    }
}

impl fmt::Display for FederationStewardTakeoverState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Requested => "requested",
            Self::Active => "active",
            Self::Released => "released",
            Self::Reclaimed => "reclaimed",
            Self::Expired => "expired",
        })
    }
}

impl FromStr for FederationStewardTakeoverState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "requested" => Ok(Self::Requested),
            "active" => Ok(Self::Active),
            "released" => Ok(Self::Released),
            "reclaimed" => Ok(Self::Reclaimed),
            "expired" => Ok(Self::Expired),
            _ => Err(()),
        }
    }
}

/// Public Apiary control-plane evidence. It deliberately contains no worker
/// identity, provider conversation, terminal output, or terminal frame.
/// One takeover, as the Apiary may show it.
///
/// ⚠️ EVERY FIELD HERE IS ALREADY PUBLIC APIARY DATA. ADR 0036 limits the audit
/// to "Apiary, source Hive, target Hive, actor, reason, state, revision, and
/// timestamps", and this carries exactly that plus the reclaim reason, which is
/// the operator's own words about their own machine. No worker identity, no
/// repository, no terminal content — the audit says WHO held a Hive and WHY,
/// never what they did while holding it. That last part is the whole reason
/// frames are never stored.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TakeoverAuditEntry {
    pub lease: FederationStewardTakeoverLease,
    /// Why the local operator took their machine back, when they did.
    ///
    /// ⚠️ THE ONE THING THIS AUDIT CAN ANSWER THAT THE WATCH AUDIT CANNOT.
    /// Watching deliberately requires no reason (ADR 0107); takeover requires
    /// one both to start and to reclaim, and this is the reclaim half. A
    /// reclaimed lease with no reason here would mean the record lost it, not
    /// that none was given — it is enforced at the command boundary.
    pub reclaim_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverLease {
    pub id: FederationStewardTakeoverLeaseId,
    pub apiary_id: ApiaryId,
    pub source_hive_id: HiveId,
    pub target_hive_id: HiveId,
    pub source_operator_id: OperatorId,
    /// The stewardship this takeover was granted under.
    ///
    /// ⚠️ `None` MEANS KEEPER'S OWN AUTHORITY, exactly as `WatchAuthority::Keeper`
    /// does for watching. The 2026-09-21 interview settled that Keeper may take
    /// over on the same terms as a Steward, and Keeper holds no stewardship over
    /// its own Apiary — so the absence is the authority rather than missing data.
    ///
    /// It also means a Keeper-sourced lease can never be transitioned through a
    /// member credential: there is no stewardship for such a credential to
    /// match, and Keeper acts on its own store instead.
    pub stewardship_id: Option<StewardshipId>,
    pub reason: String,
    pub state: FederationStewardTakeoverState,
    pub revision: u64,
    pub requested_at: i64,
    pub acknowledged_at: Option<i64>,
    pub expires_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FederationStewardTakeoverAction {
    Request {
        target_hive_id: HiveId,
        reason: String,
        relay_protocol_version: u16,
        terminal_protocol_version: u16,
    },
    Acknowledge {
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        relay_protocol_version: u16,
        terminal_protocol_version: u16,
    },
    Renew {
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
    },
    Release {
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
    },
    Reclaim {
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverCommand {
    pub id: FederationStewardTakeoverCommandId,
    pub apiary_id: ApiaryId,
    pub action: FederationStewardTakeoverAction,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTakeoverOutcome {
    Applied,
    Rejected,
    Conflict,
}

impl fmt::Display for FederationStewardTakeoverOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Conflict => "conflict",
        })
    }
}

impl FromStr for FederationStewardTakeoverOutcome {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "conflict" => Ok(Self::Conflict),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverReceipt {
    pub command_id: FederationStewardTakeoverCommandId,
    pub outcome: FederationStewardTakeoverOutcome,
    pub lease: Option<FederationStewardTakeoverLease>,
    pub processed_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverInbox {
    pub leases: Vec<FederationStewardTakeoverLease>,
    pub generated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTakeoverOutboxState {
    Queued,
    Applied,
    Rejected,
    Conflict,
}

impl fmt::Display for FederationStewardTakeoverOutboxState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Conflict => "conflict",
        })
    }
}

impl FromStr for FederationStewardTakeoverOutboxState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "conflict" => Ok(Self::Conflict),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverOutboxEntry {
    pub command: FederationStewardTakeoverCommand,
    pub state: FederationStewardTakeoverOutboxState,
    pub attempt_count: u32,
    pub last_attempt_at: Option<i64>,
    pub receipt: Option<FederationStewardTakeoverReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverLocalState {
    pub leases: Vec<FederationStewardTakeoverLease>,
    pub outbox: Vec<FederationStewardTakeoverOutboxEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationStewardTakeoverRelayRole {
    Source,
    Target,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardTakeoverRelayAuthorization {
    pub lease: FederationStewardTakeoverLease,
    pub role: FederationStewardTakeoverRelayRole,
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
        // A newly promoted departmental project must not invalidate the projects
        // this member can already use. This is catalog status, not authorization
        // to claim a project or a relaxation of the signed join preflight.
        if !projects.is_empty() && !projects.iter().any(FederationProjectReadiness::is_ready) {
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

#[cfg(test)]
mod catalog_readiness_tests;

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
        // Membership enables Swarm shared work. Optional Jira participation is
        // admitted separately for each project using the member's credentials.
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

    /// Preserve old all-project assertions only when they are actually true.
    /// Older Keepers reject schema2 rather than assuming every project is ready.
    #[must_use]
    pub fn submission_schema_version(&self) -> u16 {
        if self.jira_connection != JiraConnectionState::Ready
            || self.projects.iter().any(|project| !project.is_ready())
        {
            FEDERATION_PROJECT_SCOPED_JOIN_SCHEMA_VERSION
        } else {
            FEDERATION_MEMBERSHIP_SCHEMA_VERSION
        }
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
    #[serde(default)]
    pub operator_email: Option<String>,
    pub role: LocalApiaryRole,
    pub is_local: bool,
}

/// Public destination identity available to an authenticated Apiary member
/// when she offers confirmed shared work to another Hive. Credentials and
/// private Hive state are deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationHandoffTarget {
    pub node_id: FederationNodeId,
    pub hive_id: HiveId,
    pub hive_name: String,
    pub operator_id: OperatorId,
    pub operator_display_name: String,
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

impl ApiaryJoinBlocker {
    /// The stable wire code for this blocker.
    ///
    /// ⚠️ THE REASON TRAVELS AS A CODE, NEVER AS THE KEEPER'S PROSE. A member
    /// Hive deliberately refuses to render another Hive's text — that boundary
    /// exists so a remote cannot write sentences onto this operator's screen —
    /// so the reason has to be something the member can map to its OWN words.
    /// These strings are that mapping, and renaming one silently degrades a
    /// member that has not been updated to the generic refusal, which is the
    /// safe direction.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::HiveAlreadyFederated => "apiary_join_blocked_hive_already_federated",
            Self::InvitationRequired => "apiary_join_blocked_invitation_required",
            Self::InvitationExpired => "apiary_join_blocked_invitation_expired",
            Self::IdentityNotVerified => "apiary_join_blocked_identity_not_verified",
            Self::IntegrationNotReady => "apiary_join_blocked_integration_not_ready",
            Self::ProjectAccessNotReady => "apiary_join_blocked_project_access_not_ready",
            Self::PolicyNotAccepted => "apiary_join_blocked_policy_not_accepted",
            Self::ProtocolMismatch => "apiary_join_blocked_protocol_mismatch",
        }
    }

    /// What this blocker means to the person who has to clear it.
    ///
    /// ⚠️ WRITTEN FOR THE OPERATOR READING A REFUSAL, not for a log. Each one
    /// names the thing to do, because a join refusal that cannot be acted on
    /// sends somebody to their Keeper for a fact their own screen already had.
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::HiveAlreadyFederated => {
                "this Hive already belongs to an Apiary, and must leave it before joining another"
            }
            Self::InvitationRequired => {
                "there is no pending invitation for this Hive; ask the Keeper to issue a new one"
            }
            Self::InvitationExpired => {
                "the invitation has expired; ask the Keeper to issue a new one"
            }
            Self::IdentityNotVerified => "this Hive's identity has not been verified yet",
            Self::IntegrationNotReady => "a required integration is not connected yet",
            Self::ProjectAccessNotReady => "the promoted projects are not all reachable yet",
            Self::PolicyNotAccepted => "the Apiary policy revision has not been accepted yet",
            Self::ProtocolMismatch => {
                "this Hive speaks a different federation protocol; update Swarm on one side"
            }
        }
    }
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

/// Keeper-known shared-work status for one Hive in a Steward's explicit scope.
/// This deliberately excludes private workers, repositories, terminals,
/// provider sessions, local tasks, and integration credentials.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FederationStewardHiveObservation {
    pub hive_id: HiveId,
    pub ready_swarm_task_count: usize,
    pub active_swarm_task_count: usize,
    pub blocked_swarm_task_count: usize,
    pub review_swarm_task_count: usize,
    pub active_jira_claim_count: usize,
    pub last_shared_activity_at: Option<i64>,
}

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
    /// Additive for rolling compatibility: older Keepers omit it and older
    /// Members ignore it. An empty list means no observation has arrived yet.
    #[serde(default)]
    pub observations: Vec<FederationStewardHiveObservation>,
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
