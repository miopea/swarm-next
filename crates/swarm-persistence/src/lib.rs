use std::{
    collections::HashSet,
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    Apiary, ApiaryId, ApiaryMemberSummary, ControlRoomEventKind, Hive, HiveId, HiveIdentity,
    LocalApiaryContext, LocalApiaryRole, Operator, OperatorId, SharedWorkBackend,
    StewardCapability, Stewardship, StewardshipId, Task, TaskActivity, TaskActivityActor,
    TaskActivityActorKind, TaskActivityKind, TaskActivityPage, TaskAmendment, TaskDetailsUpdate,
    TaskDispatchState, TaskId, TaskOutcomeDeliveryState, TaskPriority, TaskState, WorkerId,
    WorkerSessionId,
};
use thiserror::Error;
use uuid::Uuid;

mod apiary;
mod attention;
mod coordinator;
pub use coordinator::{
    AUTOMATIC_WAKE_BATCH_LIMIT, AssignedReadyWorkNotStartedCandidate, BackgroundWorkReading,
    CoordinatorAttention, CoordinatorRefusal, CoordinatorStatus, CoordinatorWorkerWake,
    ExitedWorkerOwnedWorkCandidate, OverdueDecisionCandidate, REFUSAL_DELIVERY_HELD,
    REFUSAL_DELIVERY_HELD_UNSENT_TEXT, REFUSAL_WAKE_NOT_ADMITTED, REFUSAL_WAKE_UNCERTAIN,
    StaleOwnedWorkCandidate, UnreachableAssignment,
};
pub use passkeys::RegisteredPasskey;
pub use task_dispatches::{DispatchHold, HeldTaskDispatch};
pub use task_outcomes::{
    CompletionEvidence, CompletionExemptionRecord, ReviewedSettlementPage, TaskEvidenceRecord,
};
mod decisions;
mod email;
mod events;
mod federation;
mod federation_handoff_reconciliation;
mod federation_handoffs;
mod federation_jira_claims;
mod federation_steward_assists;
mod federation_steward_takeovers;
mod federation_steward_tasks;
mod federation_stewardships;
pub use federation_steward_assists::MAX_FEDERATION_STEWARD_ASSIST_BATCH;
pub use federation_steward_takeovers::{
    MAX_FEDERATION_STEWARD_TAKEOVER_BATCH, STEWARD_TAKEOVER_RELAY_PROTOCOL_VERSION,
    STEWARD_TAKEOVER_TERMINAL_PROTOCOL_VERSION,
};
pub use federation_steward_tasks::MAX_FEDERATION_STEWARD_TASK_BATCH;
mod federation_tasks;
pub use federation_tasks::MAX_FEDERATION_TASK_COMMAND_BATCH;
mod feedback;
pub use federation::{
    MAX_CONNECTION_CARD_LIFETIME_SECONDS, MAX_FEDERATION_INVITATION_LIFETIME_SECONDS,
    MIN_CONNECTION_CARD_LIFETIME_SECONDS, MIN_FEDERATION_INVITATION_LIFETIME_SECONDS,
    verify_apiary_invitation_envelope, verify_federation_catalog_snapshot,
    verify_federation_departure_receipt, verify_federation_membership_receipt,
    verify_hive_connection_card,
};
pub use federation_handoff_reconciliation::{
    FederationHandoffIntent, FederationHandoffIntentPhase, MAX_FEDERATION_HANDOFF_BATCH,
};
pub use federation_jira_claims::{
    FederationJiraClaimIntent, FederationJiraClaimPhase, MAX_FEDERATION_JIRA_CLAIM_BATCH,
};
mod jira;
mod legacy_source;
mod migration;
pub use feedback::{DogfoodReport, MAX_DOGFOOD_REPORTS};
pub use jira::{
    JiraCommentDispatch, JiraIssueSnapshot, JiraProjectBindingInput, JiraTransitionDispatch,
    JiraTransitionFailure,
};
pub use legacy_source::{
    LegacySourceError, claude_project_directory, claude_project_slugs, read_legacy_migration_bundle,
};
pub use migration::{
    LEGACY_MIGRATION_FORMAT, LEGACY_MIGRATION_VERSION, LegacyImportDisposition,
    LegacyMigrationBundle, LegacyMigrationCommit, LegacyMigrationPreview, LegacyMigrationReceipt,
    LegacyMigrationRollback, LegacyMigrationSource, LegacyTaskPreview, LegacyTaskRecord,
    LegacyWorkerImportDisposition, LegacyWorkerMigrationCommit, LegacyWorkerMigrationPreview,
    LegacyWorkerMigrationReceipt, LegacyWorkerMigrationRollback, LegacyWorkerPreview,
    LegacyWorkerRecord,
};
mod conversation_recovery;
mod dogfood_evidence;
mod message_delivery;
mod operator_statements;
mod operator_submissions;
mod review_answers;
pub use message_delivery::{
    ClaimedTaskMessage, TASK_MESSAGE_BATCH_LIMIT, TASK_MESSAGE_QUEUE_LIMIT,
    TaskMessageAttentionPage, TaskMessageResult,
};
pub use operator_statements::{OperatorStatementError, VerifiedOperatorStatement};
pub use operator_submissions::{AuthoredOperatorSubmission, OperatorSubmissionIndexEntry};
pub use review_answers::ReturnedReviewRequest;
mod night_watch;
pub use dogfood_evidence::{EvidenceError, EvidenceWrite};
mod passkeys;
mod presence;
pub use decisions::{DecisionDeliveryFailure, DecisionDispatch, NewDecisionRequest};
pub use email::{
    EmailAttachmentSnapshot, EmailImport, EmailMessageSnapshot, EmailReplyDispatch,
    EmailReplyFailure, EmailReplyState, EmailReplyTarget, EmailReplyTargetDispatch,
    EmailTaskAttachment, EmailTaskDraft, EmailTaskLink, TaskDeploymentRecord, UnansweredEmailTask,
};
pub use night_watch::NightWatchConfiguration;
pub use presence::PresenceMutation;
mod notifications;
mod terminal_control_projection;
pub use notifications::{
    NotificationDeliveryFailure, NotificationDispatch, NotificationSettings, PushSubscriptionInput,
    VapidKeyMaterial,
};
pub use terminal_control_projection::TerminalControlProjection;
mod deployment_grants;
mod ops_tickets;
mod orchestration;
pub use ops_tickets::{OpsDeploymentPage, OpsDeploymentRecord, OpsTicketReceipt};
mod queen_conductor;
pub use queen_conductor::{QueenAutomationDelivery, QueenAutomationFailure, QueenAutomationFinish};
mod presentation;
pub use presentation::{PresentationColorTheme, PresentationDeviceClass, PresentationPreferences};
mod task_dispatches;
pub use task_dispatches::{TaskDispatch, TaskDispatchFailure, TaskRuling};
mod messages;
pub use messages::{
    MAX_TASK_MESSAGE_BYTES, MessageEnd, MessageParty, OperatorBroadcastDispatch, TaskMessage,
    TaskMessageDispatch,
};
mod task_outcomes;
pub use task_outcomes::{TaskOutcomeDispatch, TaskOutcomeFailure};
mod workers;
pub use decisions::{
    HeldForAnswer, INTERVIEW_ANSWERED_ACTION, MAX_DECISION_RESULTS, OPERATOR_ANSWER_HEADER,
};
use events::insert_control_room_event;
#[cfg(test)]
use events::{MAX_CONTROL_ROOM_EVENT_PAGE, MAX_CONTROL_ROOM_EVENTS};
pub use workers::{ActiveWorkerSession, ConnectionProfile, GeometryContention, ScoutRoutingFacts};
pub(crate) const MAX_TASK_TITLE_BYTES: usize = 240;
/// Matches the ceiling the Outlook fetcher accepts for a message body, because
/// an imported email becomes a description verbatim. Anything smaller fetches
/// the mail successfully and then fails to import it, which no operator sees.
pub const MAX_TASK_DESCRIPTION_BYTES: usize = 100_000;
const MAX_PUBLIC_IDENTITY_NAME_BYTES: usize = 120;
pub const MAX_TASK_ACTIVITY_NOTE_BYTES: usize = 4_000;
/// One line of operator direction. Long enough for "interview me before acting"
/// and short enough that it cannot quietly become a second description.
pub const MAX_OPERATOR_INSTRUCTION_BYTES: usize = 280;
const MAX_WORKSPACE_BYTES: usize = 4096;
const TERMINAL_GEOMETRY_SCHEMA_VERSION: i64 = 66;
const LEGACY_MIGRATION_SCHEMA_VERSION: i64 = 67;
const LEGACY_WORKER_MIGRATION_SCHEMA_VERSION: i64 = 68;
const TASK_REMOVAL_SCHEMA_VERSION: i64 = 69;
const DEPLOYMENT_GRANT_SCHEMA_VERSION: i64 = 70;
const LEGACY_PROVIDER_CONVERSATION_SCHEMA_VERSION: i64 = 71;
const LEGACY_EXISTING_CONVERSATION_SCHEMA_VERSION: i64 = 72;
const QUEEN_DELIVERY_SESSION_SCHEMA_VERSION: i64 = 73;
const PRESENCE_LAST_ACTIVE_SCHEMA_VERSION: i64 = 74;
const TASK_OPERATOR_INSTRUCTION_SCHEMA_VERSION: i64 = 75;
const WORKER_REVIVAL_INTENT_SCHEMA_VERSION: i64 = 76;
const DECISION_RESOLUTION_SURFACE_SCHEMA_VERSION: i64 = 77;
const DECISION_QUESTIONS_SCHEMA_VERSION: i64 = 78;
const DECISION_SUMMARY_SCHEMA_VERSION: i64 = 79;
const EMAIL_REPLY_FROM_REVIEW_SCHEMA_VERSION: i64 = 80;
const WORKER_FILED_DRAFT_SCHEMA_VERSION: i64 = 81;
const START_SURFACE_SCHEMA_VERSION: i64 = 82;
const RELEASE_CHECK_SCHEMA_VERSION: i64 = 83;
const COMPLETION_EXEMPTION_SCHEMA_VERSION: i64 = 84;
const DECISION_DEADLINE_ATTENTION_SCHEMA_VERSION: i64 = 85;
const COORDINATOR_REFUSAL_SCHEMA_VERSION: i64 = 86;
const OPERATOR_PASSKEY_SCHEMA_VERSION: i64 = 87;
const TERMINAL_GEOMETRY_LEDGER_SCHEMA_VERSION: i64 = 88;
const UNDELIVERED_BRIEF_ATTENTION_SCHEMA_VERSION: i64 = 89;
const REVIEWED_WORK_EVIDENCE_ATTENTION_SCHEMA_VERSION: i64 = 90;
const REVIEW_HOLD_SCHEMA_VERSION: i64 = 91;
const SESSION_END_REASON_SCHEMA_VERSION: i64 = 92;
const REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION: i64 = 93;
const ATTENTION_NOTIFICATION_SCHEMA_VERSION: i64 = 94;
const SUPERSEDED_EXEMPTION_SCHEMA_VERSION: i64 = 95;
const OPEN_PROVIDER_SET_SCHEMA_VERSION: i64 = 96;
const EPHEMERAL_WORKER_SCHEMA_VERSION: i64 = 97;
const TASK_AMENDMENT_SCHEMA_VERSION: i64 = 98;
const DECISION_COMMAND_GRANT_SCHEMA_VERSION: i64 = 99;
const UNATTENDED_BLOCK_SCHEMA_VERSION: i64 = 100;
const AMENDMENT_ACTIVITY_SCHEMA_VERSION: i64 = 101;
const BLOCK_DEADLINE_SCHEMA_VERSION: i64 = 102;
const WORKER_MARK_SCHEMA_VERSION: i64 = 103;
const CONNECTION_PRINCIPAL_SCHEMA_VERSION: i64 = 104;
/// An operator's record that finished work cannot now be shown to be live.
const UNVERIFIABLE_CLOSURE_SCHEMA_VERSION: i64 = 105;
/// Where a dogfood report went, when it went anywhere.
const FEEDBACK_ISSUE_SCHEMA_VERSION: i64 = 106;
/// GitHub issues that have already come down as tasks.
const GITHUB_ISSUE_INTAKE_SCHEMA_VERSION: i64 = 107;
/// The evidence rule moves off the draft and onto the send.
const REPLY_EVIDENCE_GUARDS_THE_SEND_SCHEMA_VERSION: i64 = 108;
/// A person's own GitHub account, so their feedback is filed as them.
const GITHUB_USER_CONNECTION_SCHEMA_VERSION: i64 = 109;
const ABANDONED_STATE_SCHEMA_VERSION: i64 = 110;
const TASK_COMMIT_REPORT_SCHEMA_VERSION: i64 = 111;
const COORDINATOR_SETTLEMENT_SCHEMA_VERSION: i64 = 112;
const EVIDENCED_WORK_NOT_CLOSED_SCHEMA_VERSION: i64 = 113;
const AWAITING_RELEASE_SCHEMA_VERSION: i64 = 114;
const RETURNED_REVIEW_SCHEMA_VERSION: i64 = 115;
const TASK_MESSAGE_SCHEMA_VERSION: i64 = 116;
const APPROVAL_BASIS_SCHEMA_VERSION: i64 = 117;
const PARTIAL_DEPLOYMENT_SCHEMA_VERSION: i64 = 118;
const OPERATOR_BROADCAST_SCHEMA_VERSION: i64 = 119;
const BROADCAST_EXPIRY_SCHEMA_VERSION: i64 = 120;
const MESSAGE_DELIVERY_SESSION_SCHEMA_VERSION: i64 = 121;
const DELIVERY_COOLDOWN_SCHEMA_VERSION: i64 = 122;
const CLAIM_WITHDRAWAL_SCHEMA_VERSION: i64 = 123;
const TERMINAL_CONTROL_PROJECTION_SCHEMA_VERSION: i64 = 124;
const NIGHT_WATCH_SCHEMA_VERSION: i64 = 125;
const DOGFOOD_EVIDENCE_SCHEMA_VERSION: i64 = 126;
const CONVERSATION_RECOVERY_SCHEMA_VERSION: i64 = 127;
const CONVERSATION_SELECTION_SCHEMA_VERSION: i64 = 128;
const TASK_DISPATCH_GENERATION_SCHEMA_VERSION: i64 = 129;
const OPERATOR_STATEMENTS_SCHEMA_VERSION: i64 = 130;
const OPERATOR_STATEMENT_RESOLUTIONS_SCHEMA_VERSION: i64 = 131;
const OPERATOR_SUBMISSIONS_SCHEMA_VERSION: i64 = 132;
const REVIEW_ANSWERS_SCHEMA_VERSION: i64 = 133;
const MESSAGE_DELIVERY_SCHEMA_VERSION: i64 = 134;
// Upstream introduced Ops tickets as schema 124 while this branch already
// owned 124-134. Keep every migration identity unique in the combined history.
const OPS_TICKETS_SCHEMA_VERSION: i64 = 135;
// A database which ran upstream's original schema 124 could already claim the
// maturity migration's number without carrying its terminal-control table.
// Repair that published collision explicitly rather than trusting user_version.
const TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION: i64 = 136;
const CURRENT_SCHEMA_VERSION: i64 = TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION;

/// How long a terminal is left alone after coordination has written to it.
///
/// MEASURED, NOT CHOSEN BY FEEL. Across 331 delivery events on this Hive the
/// gaps between consecutive deliveries to one recipient ran: 10% under a
/// minute, 39% between one and three minutes, 14% between three and five, and
/// the rest longer. The complaint is about the middle band — the operator,
/// watching a worker: "It sure feels like you are getting flooded each time you
/// stop for even a second." A sixty second cooldown would have merged 33 of
/// those 331 events; five minutes merges 208.
///
/// The cost is latency and it is real: a message that would have arrived in
/// forty seconds can now wait five minutes. The operator chose that trade with
/// the cost stated.
pub const COORDINATION_DELIVERY_COOLDOWN_SECONDS: i64 = 300;
pub const MAX_TASK_ACTIVITY_PAGE: usize = 100;
pub const MAX_OPEN_TASKS_PER_ORDER: usize = 1_000;

pub(crate) fn normalize_public_identity_name(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= MAX_PUBLIC_IDENTITY_NAME_BYTES
        && !value.chars().any(char::is_control))
    .then_some(value)
}

/// Whether this Hive checks for releases, and what the last check saw.
///
/// `mode` is `unset` until the operator answers, which is not the same as
/// `off`: one is a Hive that was never asked, the other a Hive that said no.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseCheckState {
    pub mode: String,
    pub last_checked_at: Option<i64>,
    pub last_outcome: Option<String>,
    /// The verified offer as it was last seen, kept so the card says something
    /// on a machine that is currently offline.
    pub last_offer: Option<String>,
}

impl Default for ReleaseCheckState {
    fn default() -> Self {
        Self {
            mode: "unset".to_owned(),
            last_checked_at: None,
            last_outcome: None,
            last_offer: None,
        }
    }
}

#[derive(Clone)]
pub struct TaskStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Error)]
pub enum TaskStoreError {
    #[error("review reply does not match the current request, worker, or saved answer")]
    InvalidReviewReply,
    #[error("task persistence filesystem failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("task persistence failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("task persistence lock was poisoned")]
    LockPoisoned,
    #[error("Apiary invitation is invalid")]
    InvalidApiaryInvitation,
    #[error("Apiary configuration is invalid")]
    InvalidApiary,
    #[error("Hive identity is invalid")]
    InvalidHiveIdentity,
    #[error("this connection was revoked")]
    ConnectionRevoked,
    #[error("this Hive must be personal before it can found an Apiary")]
    ApiaryMembershipConflict,
    #[error("Apiary was not found")]
    ApiaryNotFound,
    #[error("Apiary invitation was not found")]
    ApiaryInvitationNotFound,
    #[error("Apiary invitation is no longer pending")]
    ApiaryInvitationResolved,
    #[error("Apiary join readiness is incomplete")]
    ApiaryJoinNotReady,
    #[error("Apiary cannot collapse until all federation state is clear")]
    ApiaryCollapseNotReady,
    #[error("Jira project is not ready for Apiary promotion")]
    ApiaryProjectPromotionNotReady,
    #[error("Hive connection card is invalid or expired")]
    InvalidFederationConnectionCard,
    #[error("the local federation identity is corrupt")]
    InvalidFederationIdentity,
    #[error("secure entropy is unavailable for the local federation identity")]
    FederationEntropyUnavailable,
    #[error("Only the active Apiary Keeper can pin Hive identities")]
    ApiaryKeeperRequired,
    #[error("The Stewardship scope or capabilities are invalid")]
    InvalidStewardship,
    #[error("The Stewardship was not found")]
    StewardshipNotFound,
    #[error("The Hive identity conflicts with a previously pinned key")]
    HiveCandidateIdentityConflict,
    #[error("The pinned Hive identity was not found")]
    HiveCandidateNotFound,
    #[error("The Apiary invitation envelope is invalid or expired")]
    InvalidFederationInvitation,
    #[error("The Apiary join link is invalid or expired")]
    InvalidApiaryJoinLink,
    #[error("The Apiary join link was not found")]
    ApiaryJoinLinkNotFound,
    #[error("The Apiary join link cannot accept that transition")]
    ApiaryJoinLinkResolved,
    #[error("This Apiary already has the maximum number of active join links")]
    ApiaryJoinLinkLimit,
    #[error("The federation node credential is invalid or expired")]
    InvalidFederationCredential,
    #[error("The federation project catalog is invalid, stale, or misaddressed")]
    InvalidFederationCatalog,
    #[error("The federation shared-work claim is invalid")]
    InvalidFederationClaim,
    #[error("The federation claim handoff is invalid")]
    InvalidFederationHandoff,
    #[error("The local federation synchronization state is invalid")]
    InvalidFederationSync,
    #[error("This Hive still owns or is sending shared Apiary work")]
    ApiaryDepartureNotReady,
    #[error("The Apiary departure receipt or state is invalid")]
    InvalidFederationDeparture,
    #[error("The federated Jira claim state is invalid")]
    InvalidFederationJiraClaim,
    #[error("This Hive already has the maximum number of pending federated Jira claims")]
    FederationJiraClaimQueueFull,
    #[error("The Apiary task or task feed is invalid")]
    InvalidFederationTask,
    #[error("The Steward task command is invalid")]
    InvalidFederationStewardTask,
    #[error("The Steward assistance request is invalid")]
    InvalidFederationStewardAssist,
    #[error("The Steward takeover command or lease is invalid")]
    InvalidFederationStewardTakeover,
    #[error("The synchronized Steward scope does not allow that action")]
    StewardActionDenied,
    #[error("This Hive already has the maximum number of queued Steward tasks")]
    FederationStewardTaskQueueFull,
    #[error("This Hive already has the maximum number of queued Steward assistance requests")]
    FederationStewardAssistQueueFull,
    #[error("This Hive already has the maximum number of queued Steward takeover commands")]
    FederationStewardTakeoverQueueFull,
    #[error("The Jira issue is already claimed by another Hive")]
    FederationClaimConflict,
    #[error("The federation claim already has a conflicting handoff")]
    FederationHandoffConflict,
    #[error("A current invitation already exists for this pinned Hive")]
    FederationInvitationConflict,
    #[error("task was not found")]
    NotFound,
    // Not an authorisation failure, which is what it used to say. A task with
    // no Jira issue behind it is an ordinary state with an obvious remedy, and
    // reporting it as forbidden sent the reader to check their permissions.
    #[error("this task has no Jira issue linked to it")]
    TaskHasNoJiraIssue,
    #[error("decision request was not found")]
    DecisionNotFound,
    #[error("decision request content is invalid")]
    InvalidDecisionContent,
    #[error(
        "a decision request needs a summary of at most 400 characters saying what the operator is deciding and what turns on it"
    )]
    InvalidDecisionSummary,
    #[error("decision request must offer 1 to 6 unique actions")]
    InvalidDecisionActions,
    #[error(
        "an interview asks at most 4 questions, each with 2 to 4 unique options and a unique header, and offers no actions"
    )]
    InvalidDecisionQuestions,
    #[error("an interview is answered by answering every question it asks")]
    IncompleteDecisionAnswers,
    #[error("dismissing an interview needs a reason the asking worker can act on")]
    DismissedInterviewNeedsReason,
    #[error("decision request deadline is invalid")]
    InvalidDecisionDeadline,
    #[error("decision request is already resolved")]
    DecisionAlreadyResolved,
    #[error("decision resolution must use one of the allowed actions")]
    InvalidDecisionResolution,
    #[error("this Hive already has the maximum number of pending decisions")]
    DecisionInboxFull,
    #[error("this Hive already has the maximum number of pending task briefings")]
    TaskDispatchQueueFull,
    #[error("this Hive already tracks the maximum of 16 presence devices")]
    PresenceDeviceLimit,
    #[error("notification subscription material is invalid")]
    InvalidNotificationSubscription,
    #[error("this Hive already has the maximum of 8 notification subscriptions")]
    NotificationSubscriptionLimit,
    #[error("the bounded notification delivery queue is full")]
    NotificationQueueFull,
    #[error("the installation notification signing key is invalid")]
    InvalidVapidKey,
    #[error(
        "operator instruction must be a single line of at most {MAX_OPERATOR_INSTRUCTION_BYTES} bytes"
    )]
    InvalidOperatorInstruction,
    #[error("task handoff note must not exceed {MAX_TASK_ACTIVITY_NOTE_BYTES} bytes")]
    InvalidTaskActivityNote,
    #[error("completed work requires concise verification evidence")]
    CompletionEvidenceRequired,
    // ⚠️ NAMES THE MISSING RECORD, NOT THE BASIS. This case used to return
    // CompletionEvidenceRequired, whose text is "completed work requires
    // concise verification evidence" -- so an approver with a perfectly good
    // basis was told their basis was inadequate, and the actual cause was that
    // there was no claim to countersign at all.
    //
    // Measured cost: Queen attempted swarm_approve_no_deployment FOUR TIMES on
    // 01a06b37-eac8, rewriting the basis each time, before working out that
    // `evidence.exemption` was NULL because the worker's claim had been
    // REFUSED. The error pointed at the one thing that was fine.
    //
    // A refused claim leaves no row, so there is nothing to approve and the
    // remedy is not a better basis -- it is the worker recording a claim, or
    // the operator writing the task off as unverifiable.
    #[error(
        "no nothing-to-deploy claim has been recorded for this task, so there is nothing to approve. The worker's claim was refused or never made -- have the worker record one, or write the task off as unverifiable"
    )]
    NoCompletionExemptionToApprove,
    // NAMES THE CONTRADICTION, not the rule. A worker reading "not authorized"
    // or "evidence required" would go looking at permissions or at what it
    // wrote; the thing to look at is the commits it reported.
    // NAMES THE WAY FORWARD, not only the objection. A refusal whose remedy
    // nobody can find from the message is barely better than no route at all --
    // the protocol-migration refusal cost this Hive three hours by describing a
    // migration without naming the command that performed it.
    //
    // The routes named are the ones that stay open: the claim is refused, so
    // there is no exemption row for anyone to approve, and saying "ask Queen to
    // approve" would send a worker after a record that does not exist.
    #[error(
        "the commits recorded for this task touch code, which contradicts a claim that nothing was deployed. Record where it is running with a deployment, or ask the operator to write it off as unverifiable, or close it as abandoned if it was superseded"
    )]
    CommitsContradictNoDeployment,
    // ⚠️ THE REFUSAL THAT MAKES HONESTY THE CHEAP PATH. Until 2026-09-04 a task
    // with NO commit report at all could claim "nothing was deployed" freely,
    // while a worker who reported its commits could be refused on them. So
    // reporting was the only route to a refusal and silence always passed --
    // measured on two tasks that made the same comment-only .ts change minutes
    // apart, where the honest worker was refused and the silent one approved.
    //
    // NAMES THE ONE-CALL REMEDY, and it is genuinely one call: record_task_commits
    // documents an empty list as the way to say nothing was built, which settles
    // as NothingBuilt and passes. A worker outside a checkout is NOT caught by
    // this -- a report that cannot be checked is Unestablished and still claims.
    #[error(
        "no commits have been reported for this task, so there is nothing for a claim of 'nothing was deployed' to stand on. Report them with swarm_record_task_commits first -- an EMPTY list is a valid answer and means the task built nothing"
    )]
    CommitsNotReported,
    #[error("this Hive already has the maximum number of pending Queen handoffs")]
    TaskOutcomeQueueFull,
    #[error("Jira comment content is invalid")]
    InvalidJiraComment,
    #[error("this Hive already has the maximum number of pending Jira comments")]
    JiraCommentQueueFull,
    #[error("email message metadata or content is invalid")]
    InvalidEmailMessage,
    /// Carries WHICH messages are already attached and to what.
    ///
    /// It used to be a bare sentence. Accurate, and unactionable: the operator
    /// was told a selection conflicted and not which of six messages caused it,
    /// so the only way forward was to deselect them one at a time.
    #[error("{0}")]
    EmailMergeConflict(String),
    #[error("email attachment metadata exceeds its private bounds")]
    InvalidEmailAttachment,
    #[error("email source was not found")]
    EmailSourceNotFound,
    #[error(
        "a deployment reference and environment must each be present and no more than \
         {max} bytes. Nothing about the shape is checked: a bare commit, \
         a bare URL and a sentence are all accepted, so say whatever a third party could use to \
         confirm this is running."
    )]
    InvalidTaskDeployment { max: usize },
    #[error("deployment evidence belongs on work that is finished: move this task to review first")]
    DeploymentEvidenceTooEarly,
    #[error("email resolution reply content is invalid")]
    InvalidEmailReply,
    // NAMES BOTH CONDITIONS, because the old wording cost eleven days. It read
    // "email resolution replies require completed and deployed work", which is
    // true and unhelpful: it parses as a SEQUENCING instruction — finish the
    // work, then deploy, then come back — so two different sessions hit it on
    // the same task, both concluded they had simply arrived too early, and
    // neither checked what the gate actually tested. Ryan Denee had written on
    // 2026-08-14 about spam arriving through literature orders. He was right,
    // the scoping was done, the operator ruled, and the task was completed
    // specifically so the reply could be drafted. He heard nothing for eleven
    // days because a message described the wrong precondition.
    #[error(
        "an email reply cannot be SENT until the task is in review or completed AND has either a recorded deployment or an approved no-deployment exemption; this task has neither. The draft is kept — only sending is held."
    )]
    EmailReplyNotReady,
    // Drafting asks far less than sending, and says so separately. The two used
    // to share one message, which is why a worker that hit it concluded it had
    // simply arrived too early and gave up instead of writing the reply.
    #[error(
        "an email reply can be drafted once the task is in review or completed; this task is neither"
    )]
    EmailDraftNotReady,
    // Named separately because it is not a state problem and cannot be waited
    // out. A task with no inbound email has nobody to reply to, and telling
    // somebody to finish the work first would send them to fix the wrong thing.
    #[error("this task did not come from an email, so there is no thread to reply to")]
    TaskHasNoEmailThread,
    #[error("this task already has an email resolution reply")]
    EmailReplyAlreadyExists,
    #[error("this Hive already has the maximum number of pending email replies")]
    EmailReplyQueueFull,
    #[error("task title must contain 1 to {MAX_TASK_TITLE_BYTES} bytes")]
    InvalidTitle,
    #[error("the Ops request already has a different submitted command")]
    OpsTicketConflict,
    #[error("task description must not exceed {MAX_TASK_DESCRIPTION_BYTES} bytes")]
    InvalidDescription,
    #[error("task details update must contain at least one field")]
    EmptyTaskDetailsUpdate,
    #[error("workspace must contain 1 to {MAX_WORKSPACE_BYTES} bytes")]
    InvalidWorkspace,
    #[error("task cannot move from {from} to {to}")]
    InvalidTransition { from: TaskState, to: TaskState },
    /// A completed task asked to complete again, with the reason it is already
    /// closed rather than the rule that forbids saying so twice.
    ///
    /// "task cannot move from completed to completed" is true and names
    /// nothing. What happened is that something closed this task seconds
    /// earlier on evidence someone else recorded, and a reader given the rule
    /// goes looking for a lifecycle bug instead. Twice on 2026-08-25.
    #[error(
        "this task was already completed {seconds_ago}s ago by {closed_by}, so this note was not attached to it. The work is closed rather than blocked, and nothing is wrong. Evidence can still be recorded against a closed task with swarm_record_deployment; a handoff note cannot, so anything you still hold needs a durable home of its own."
    )]
    TaskAlreadyCompleted { seconds_ago: i64, closed_by: String },
    #[error("completed tasks cannot be assigned")]
    CompletedTask,
    #[error("work in progress must be stopped or completed before it can be removed")]
    ActiveTaskCannotBeRemoved,
    // NAMES THE TASK, because the refusal already has it and withholding it
    // cost two agents half an hour on 2026-09-01. The worker was told a slot
    // was occupied and not by what, guessed from its own board -- which had
    // several tickets in Review -- and concluded that Review gates Active. It
    // does not; the gate is `state = 'active'` alone. That guess was relayed as
    // a first-hand account and believed, and a second worker was told its queue
    // was blocked when it was not.
    //
    // The id was one column away in the query that produced this error, so the
    // fix is to say what is already in hand rather than to look anything up.
    #[error(
        "this worker already has work in progress: {holding_task} ({holding_title}); \
         finish or move that one, or leave additional assigned work Ready"
    )]
    WorkerAlreadyHasActiveTask {
        holding_task: String,
        holding_title: String,
    },
    #[error("Jira work must be restored from Jira so its remote state remains authoritative")]
    JiraTaskCannotBeRestored,
    #[error("worker was not found")]
    WorkerNotFound,
    #[error("worker name is invalid")]
    InvalidWorkerName,
    #[error("worker name already exists")]
    DuplicateWorkerName,
    #[error("a task message must be 1 to {max} bytes and name a recipient")]
    InvalidTaskMessage { max: usize },
    #[error(
        "task message queue is full; Queen must reconcile pending deliveries before new messages can be admitted"
    )]
    TaskMessageQueueFull,
    #[error(
        "a task-scoped worker question must go to that task's current assignee; assign the work first, or explicitly request a managed Scout second opinion"
    )]
    QueenMessageRecipientNotAssigned,
    #[error("a second opinion may be requested only from this Hive's managed Scout")]
    ScoutSecondOpinionRequiresManagedScout,
    #[error("Scout has no open session; do not wake Scout solely for a second opinion")]
    ScoutSecondOpinionScoutSleeping,
    #[error("Scout is engaged with the operator; wait rather than competing for the terminal")]
    ScoutSecondOpinionOperatorEngaged,
    #[error("Scout already owns active work; do not interrupt it for a second opinion")]
    ScoutSecondOpinionActiveWork,
    /// Refused by rule, not by accident.
    ///
    /// A worker's claim about authority reaching another worker with no board
    /// record turns "anything a sender can write, a sender can fabricate" from
    /// a discipline into an attack surface. Queen relays instead, and the relay
    /// is on the task.
    #[error("workers cannot message each other; send it to Queen")]
    WorkerToWorkerMessageRefused,
    #[error("worker description must not exceed 2000 bytes or contain control characters")]
    InvalidWorkerDescription,
    #[error("worker update must contain a name, description, provider, or startup preference")]
    EmptyWorkerUpdate,
    #[error("the Queen profile is managed by Swarm and cannot be edited")]
    QueenProfileImmutable,
    #[error("Scout is a managed Hive worker and cannot be renamed or removed")]
    ScoutIdentityImmutable,
    #[error("the Queen profile already exists")]
    QueenAlreadyExists,
    #[error("worker already has an active session")]
    WorkerAlreadyRunning,
    #[error("the worker must be sleeping before changing provider or removing it")]
    WorkerMustBeSleeping,
    #[error("reassign or complete this worker's open tasks before removing it")]
    WorkerOwnsOpenTasks,
    #[error("agent credential digest must be exactly 32 bytes")]
    InvalidAgentCredentialDigest,
    #[error("worker session is not active")]
    WorkerSessionNotActive,
    #[error("provider conversation cannot be assigned after worker history exists")]
    ProviderConversationUnavailable,
    #[error("task order must contain every open task exactly once")]
    InvalidTaskOrder,
    #[error("dogfood report notes and evidence are missing or exceed their private bounds")]
    InvalidDogfoodReport,
    #[error("dogfood report attachment identity is invalid")]
    InvalidDogfoodAttachment,
    #[error("dogfood report limit must be from 1 through 50")]
    InvalidDogfoodReportLimit,
    #[error("Jira project metadata is invalid")]
    InvalidJiraProject,
    #[error("Jira workflow mapping is invalid")]
    InvalidJiraWorkflowMapping,
    #[error("Jira project binding was not found")]
    JiraProjectBindingNotFound,
    #[error("worker order must contain every operator-ordered worker exactly once")]
    InvalidWorkerOrder,
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("database integrity check failed: {0}")]
    IntegrityFailure(String),
    #[error("the Legacy migration package is invalid or unsupported")]
    InvalidMigrationBundle,
    #[error("the Legacy migration package changed after preview")]
    MigrationBundleChanged,
    #[error("the Legacy migration selection is empty, duplicated, or no longer eligible")]
    InvalidMigrationSelection,
    /// A selected worker had an open session when the import ran.
    ///
    /// Distinct from [`Self::MigrationBundleChanged`] because nothing about the
    /// package changed: the operator was told to re-scan, which never helped.
    #[error(
        "worker '{0}' has an open session; put it to sleep, refresh the preview, and import again"
    )]
    MigrationWorkerAwake(String),
    /// A selected worker collides with a worker Swarm already has, by name or
    /// by repository — including one imported earlier in the same batch.
    #[error("Swarm already has a worker for '{0}'; rename or deselect it, then import again")]
    MigrationWorkerDuplicate(String),
    #[error("the Legacy migration batch was not found or was already rolled back")]
    MigrationBatchNotFound,
    #[error("the Legacy migration batch contains work that has changed and cannot be rolled back")]
    MigrationBatchChanged,
}

fn rearm_briefing_for_returned_work(
    transaction: &rusqlite::Transaction<'_>,
    id: TaskId,
) -> Result<(), TaskStoreError> {
    // assignment_id is the primary key of task_dispatches, so there is at most
    // one row per assignment and a second briefing is a re-arm rather than an
    // insert. Both halves of the delivered CHECK move together.
    transaction.execute(
        "UPDATE task_dispatches
         SET state = 'queued', delivered_at = NULL, attempts = 0, generation = generation + 1,
             updated_at = unixepoch()
         WHERE task_id = ?1
           AND assignment_id IN (
               SELECT assignment.id FROM task_assignments assignment
               JOIN worker_sessions session
                 ON session.session_id = assignment.worker_session_id
                AND session.ended_at IS NULL
               WHERE assignment.task_id = ?1 AND assignment.released_at IS NULL
           )",
        [id.to_string()],
    )?;
    Ok(())
}

/// The deadline a transition should store, which is None unless it is a block.
///
/// Separated so the write stays one statement and the reasoning has somewhere
/// to live: `blocked_until` moves WITH the state and is cleared on the way out.
/// A deadline left behind by an earlier block would silently suppress the next
/// escalation, which is the worst direction for this to fail -- the task goes
/// quiet and nothing says why.
fn block_deadline_for(target: TaskState, note: &str) -> Option<i64> {
    (target == TaskState::Blocked)
        .then(|| parse_block_deadline(note))
        .flatten()
}

/// The moment a block says it is waiting for, if it names one.
///
/// READ FROM THE NOTE RATHER THAN A TOOL PARAMETER, deliberately. The obvious
/// design is a new argument on the transition tool, and it is wrong here: an
/// MCP client asks for its tool schema once when it connects and caches it, so
/// no session running today could send a new parameter. The field would sit
/// empty for exactly the population that needs it now. A line in the note works
/// from every session that already exists.
///
/// A MARKER, NOT PROSE. "Blocked until: <RFC3339>" on its own line. A note that
/// merely mentions a date is not a note that names its own deadline, and the
/// difference decides whether the operator hears about a stalled task -- so it
/// is stated explicitly rather than inferred from whatever timestamps appear.
///
/// Anything unparseable yields None, which escalates. Failing toward speaking
/// is the right direction: a missed escalation is silent, and a spurious one is
/// visible and can be corrected.
fn parse_block_deadline(note: &str) -> Option<i64> {
    note.lines()
        .filter_map(|line| line.trim().strip_prefix("Blocked until:"))
        .find_map(|value| parse_rfc3339_seconds(value.trim()))
}

/// Seconds since the epoch for an RFC3339 instant, without pulling in a clock
/// crate for one field.
///
/// Deliberately strict: it accepts what a worker is told to write and refuses
/// the rest, because a half-understood timestamp is worse than none. Only the
/// `Z` form, because an offset that is silently ignored would shift a deadline
/// by hours in whichever direction nobody checked.
fn parse_rfc3339_seconds(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date = date.split('-');
    let (year, month, day) = (
        date.next()?.parse::<i64>().ok()?,
        date.next()?.parse::<i64>().ok()?,
        date.next()?.parse::<i64>().ok()?,
    );
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut time = time.split(':');
    let (hour, minute, second) = (
        time.next()?.parse::<i64>().ok()?,
        time.next()?.parse::<i64>().ok()?,
        time.next()?.split('.').next()?.parse::<i64>().ok()?,
    );
    if time.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Days from the civil calendar, Howard Hinnant's algorithm. Exact for every
    // date this will ever see and it needs no dependency.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

impl TaskStore {
    /// Opens, migrates, and integrity-checks a file-backed task database.
    ///
    /// # Errors
    /// Returns an error when the path, schema, migration, or integrity check is invalid.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    /// Opens a migrated in-memory store for isolated tests and ephemeral runtimes.
    ///
    /// # Errors
    /// Returns an error when `SQLite` initialization or migration fails.
    pub fn in_memory() -> Result<Self, TaskStoreError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, TaskStoreError> {
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match schema_version {
            found if found < CURRENT_SCHEMA_VERSION => {
                // Foreign keys OFF for the duration of the migration, exactly as
                // SQLite's own twelve-step ALTER procedure requires, and OFF
                // BEFORE the transaction begins because `PRAGMA foreign_keys` is
                // a no-op inside one.
                //
                // This is not a relaxation. A migration that REBUILDS a table --
                // the only way to change a CHECK constraint -- has to drop the
                // old copy, and with enforcement on, DROP TABLE runs an implicit
                // DELETE FROM that trips every child row pointing at the parent.
                // `defer_foreign_keys` was tried first and merely moves the same
                // failure to COMMIT.
                //
                // Integrity is not taken on trust: foreign_key_check runs after
                // the commit and refuses the open if the migration left a
                // dangling reference, which is a stronger guarantee than
                // per-statement enforcement gave, because it examines EVERY row
                // rather than only the ones a migration happened to touch.
                let foreign_keys_were_on: bool =
                    connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
                if foreign_keys_were_on {
                    connection.pragma_update(None, "foreign_keys", "OFF")?;
                }
                let transaction = connection.transaction()?;
                if schema_version == 0 {
                    transaction.execute_batch(
                        "
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                workspace TEXT NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed')),
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS task_assignments (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                worker_session_id TEXT NOT NULL,
                assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                released_at INTEGER
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_active_assignment_per_task
                ON task_assignments(task_id) WHERE released_at IS NULL;
            CREATE TABLE IF NOT EXISTS task_activity (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                from_state TEXT,
                to_state TEXT,
                occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            ",
                    )?;
                }
                migrate_schema(&transaction, schema_version)?;
                transaction.commit()?;
                if foreign_keys_were_on {
                    connection.pragma_update(None, "foreign_keys", "ON")?;
                    let dangling: Option<String> = connection
                        .query_row(
                            "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
                            [],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if let Some(table) = dangling {
                        return Err(TaskStoreError::IntegrityFailure(format!(
                            "migration left a dangling foreign key in {table}"
                        )));
                    }
                }
            }
            CURRENT_SCHEMA_VERSION => {}
            found => {
                return Err(TaskStoreError::UnsupportedSchemaVersion {
                    found,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
        }
        let integrity: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if integrity != "ok" {
            return Err(TaskStoreError::IntegrityFailure(integrity));
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Returns the durable operator and Hive owned by this local installation.
    ///
    /// # Errors
    /// Returns an error when identity persistence is unavailable or invalid.
    pub fn local_hive_identity(&self) -> Result<HiveIdentity, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT o.id, o.display_name, h.id, h.name, h.apiary_id
                FROM local_hive_identity l
                JOIN hives h ON h.id = l.hive_id
                JOIN operators o ON o.id = h.operator_id
                WHERE l.singleton = 1
                ",
                [],
                |row| {
                    let operator_id = parse_domain_id::<OperatorId>(&row.get::<_, String>(0)?)?;
                    let hive_id = parse_domain_id::<HiveId>(&row.get::<_, String>(2)?)?;
                    let apiary_id = row
                        .get::<_, Option<String>>(4)?
                        .map(|value| parse_domain_id::<ApiaryId>(&value))
                        .transpose()?;
                    Ok(HiveIdentity {
                        operator: Operator {
                            id: operator_id,
                            display_name: row.get(1)?,
                        },
                        hive: Hive {
                            id: hive_id,
                            name: row.get(3)?,
                            operator_id,
                            apiary_id,
                        },
                    })
                },
            )
            .map_err(TaskStoreError::from)
    }

    /// Renames only the Hive owned by this installation. Membership, operator,
    /// federation keys, workers, tasks, and repositories are unchanged.
    ///
    /// # Errors
    /// Rejects blank, oversized, control-character, or invalid-time input and
    /// unavailable persistence.
    pub fn rename_local_hive(&self, name: &str, now: i64) -> Result<HiveIdentity, TaskStoreError> {
        let name =
            normalize_public_identity_name(name).ok_or(TaskStoreError::InvalidHiveIdentity)?;
        if now < 0 {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if transaction.execute(
            "UPDATE hives SET name = ?1, updated_at = ?2
             WHERE id = ?3 AND operator_id = ?4",
            params![
                name,
                now,
                identity.hive.id.to_string(),
                identity.operator.id.to_string()
            ],
        )? != 1
        {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        drop(connection);
        self.local_hive_identity()
    }

    /// Returns the local Hive's optional federation without inferring any
    /// Steward authority that has not been durably granted.
    ///
    /// # Errors
    /// Returns an error when identity or Apiary persistence is unavailable or invalid.
    pub fn local_apiary_context(&self) -> Result<LocalApiaryContext, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT a.id, a.name, a.keeper_operator_id, a.shared_work_backend,
                       h.operator_id, a.policy_revision
                FROM local_hive_identity l
                JOIN hives h ON h.id = l.hive_id
                LEFT JOIN apiaries a ON a.id = h.apiary_id
                WHERE l.singleton = 1
                ",
                [],
                |row| {
                    let Some(apiary_id) = row.get::<_, Option<String>>(0)? else {
                        return Ok(LocalApiaryContext::Personal);
                    };
                    let apiary_id = parse_domain_id::<ApiaryId>(&apiary_id)?;
                    let keeper_operator_id =
                        parse_domain_id::<OperatorId>(&row.get::<_, String>(2)?)?;
                    let local_operator_id =
                        parse_domain_id::<OperatorId>(&row.get::<_, String>(4)?)?;
                    let backend = row
                        .get::<_, String>(3)?
                        .parse::<SharedWorkBackend>()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(LocalApiaryContext::Federated {
                        apiary: Apiary::persisted(
                            apiary_id,
                            row.get::<_, String>(1)?,
                            keeper_operator_id,
                            backend,
                            row.get::<_, u64>(5)?,
                        ),
                        local_role: if keeper_operator_id == local_operator_id {
                            LocalApiaryRole::Keeper
                        } else {
                            LocalApiaryRole::Member
                        },
                    })
                },
            )
            .map_err(TaskStoreError::from)
    }

    /// Lists the durable public identities in the local Apiary view. Both a
    /// Keeper and a joined member can inspect this roster; private federation
    /// material never leaves its dedicated tables.
    ///
    /// # Errors
    /// Rejects personal Hives and invalid or unavailable persistence.
    pub fn list_apiary_members(&self) -> Result<Vec<ApiaryMemberSummary>, TaskStoreError> {
        let LocalApiaryContext::Federated { apiary, .. } = self.local_apiary_context()? else {
            return Err(TaskStoreError::InvalidApiary);
        };
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT h.id, h.name, o.id, o.display_name
             FROM hives h
             JOIN operators o ON o.id = h.operator_id
             WHERE h.apiary_id = ?1
             ORDER BY CASE WHEN o.id = ?2 THEN 0 ELSE 1 END, lower(h.name), h.id",
        )?;
        let rows = statement.query_map(
            [apiary.id.to_string(), apiary.keeper_operator_id.to_string()],
            |row| {
                let hive_id = parse_domain_id::<HiveId>(&row.get::<_, String>(0)?)?;
                let operator_id = parse_domain_id::<OperatorId>(&row.get::<_, String>(2)?)?;
                Ok(ApiaryMemberSummary {
                    hive_id,
                    hive_name: row.get(1)?,
                    operator_id,
                    operator_display_name: row.get(3)?,
                    role: if operator_id == apiary.keeper_operator_id {
                        LocalApiaryRole::Keeper
                    } else {
                        LocalApiaryRole::Member
                    },
                    is_local: hive_id == identity.hive.id,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Loads only active, explicitly persisted Steward grants for one Apiary.
    /// Missing grants return an empty set and never imply authority.
    ///
    /// # Errors
    /// Returns an error when persisted identifiers or capabilities are invalid.
    pub fn stewardships_for_apiary(
        &self,
        apiary_id: ApiaryId,
    ) -> Result<Vec<Stewardship>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, steward_operator_id
             FROM stewardships
             WHERE apiary_id = ?1 AND revoked_at IS NULL
             ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([apiary_id.to_string()], |row| {
            Ok((
                parse_domain_id::<StewardshipId>(&row.get::<_, String>(0)?)?,
                parse_domain_id::<OperatorId>(&row.get::<_, String>(1)?)?,
            ))
        })?;
        let grants = rows.collect::<Result<Vec<_>, _>>()?;
        grants
            .into_iter()
            .map(|(id, steward_operator_id)| {
                let managed_hive_ids = {
                    let mut statement = connection.prepare(
                        "SELECT hive_id FROM stewardship_hive_grants
                         WHERE stewardship_id = ?1 ORDER BY hive_id",
                    )?;
                    statement
                        .query_map([id.to_string()], |row| {
                            parse_domain_id::<HiveId>(&row.get::<_, String>(0)?)
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                let capabilities = {
                    let mut statement = connection.prepare(
                        "SELECT capability FROM stewardship_capability_grants
                         WHERE stewardship_id = ?1 ORDER BY capability",
                    )?;
                    statement
                        .query_map([id.to_string()], |row| {
                            row.get::<_, String>(0)?
                                .parse::<StewardCapability>()
                                .map_err(|_| rusqlite::Error::InvalidQuery)
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                Ok(Stewardship {
                    id,
                    apiary_id,
                    steward_operator_id,
                    managed_hive_ids,
                    capabilities,
                })
            })
            .collect()
    }

    /// Creates a validated draft and its first activity event atomically.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task(&self, title: &str, workspace: &str) -> Result<Task, TaskStoreError> {
        self.create_task_with_details(title, "", TaskPriority::Normal, workspace)
    }

    /// Creates a validated draft with operator-facing context and priority.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task_with_details(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, TaskStoreError> {
        self.create_task_with_details_as(
            title,
            description,
            priority,
            workspace,
            &TaskActivityActor::system(),
        )
    }

    /// The screen Swarm opens on, and the default when the operator has not
    /// chosen one.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn start_surface(&self) -> Result<String, TaskStoreError> {
        let connection = self.connection()?;
        let surface = connection
            .query_row(
                "SELECT preference.start_surface
                 FROM operator_preferences preference
                 JOIN local_hive_identity local ON local.singleton = 1
                 JOIN hives hive ON hive.id = local.hive_id
                 WHERE preference.operator_id = hive.operator_id",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(surface.unwrap_or_else(|| "tasks".to_owned()))
    }

    /// Chooses the screen Swarm opens on, for every device.
    ///
    /// # Errors
    /// Rejects a surface that is not one of the product's own.
    pub fn set_start_surface(&self, surface: &str) -> Result<String, TaskStoreError> {
        if !matches!(
            surface,
            "decisions" | "tasks" | "workers" | "apiary" | "settings"
        ) {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{surface} is not a screen this product opens on"
            )));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO operator_preferences (operator_id, start_surface, updated_at)
             SELECT hive.operator_id, ?1, unixepoch()
             FROM local_hive_identity local
             JOIN hives hive ON hive.id = local.hive_id
             WHERE local.singleton = 1
             ON CONFLICT(operator_id) DO UPDATE
                 SET start_surface = excluded.start_surface, updated_at = excluded.updated_at",
            [surface],
        )?;
        // The guard is released before anything else asks for it: reading back
        // through `start_surface` here would take the same connection lock and
        // wait on this call forever.
        drop(connection);
        self.start_surface()
    }

    /// Whether this Hive checks for releases, and what the last check saw.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn release_check_state(&self) -> Result<ReleaseCheckState, TaskStoreError> {
        let connection = self.connection()?;
        let state = connection
            .query_row(
                "SELECT preference.mode, preference.last_checked_at,
                        preference.last_outcome, preference.last_offer
                 FROM release_check_preferences preference
                 JOIN local_hive_identity local ON local.singleton = 1
                 JOIN hives hive ON hive.id = local.hive_id
                 WHERE preference.operator_id = hive.operator_id",
                [],
                |row| {
                    Ok(ReleaseCheckState {
                        mode: row.get(0)?,
                        last_checked_at: row.get(1)?,
                        last_outcome: row.get(2)?,
                        last_offer: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(state.unwrap_or_default())
    }

    /// Chooses whether this Hive contacts a release origin at all.
    ///
    /// # Errors
    /// Rejects a mode this product does not offer.
    pub fn set_release_check_mode(&self, mode: &str) -> Result<ReleaseCheckState, TaskStoreError> {
        if !matches!(mode, "off" | "daily") {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{mode} is not a release check mode"
            )));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO release_check_preferences (operator_id, mode, updated_at)
             SELECT hive.operator_id, ?1, unixepoch()
             FROM local_hive_identity local
             JOIN hives hive ON hive.id = local.hive_id
             WHERE local.singleton = 1
             ON CONFLICT(operator_id) DO UPDATE
                 SET mode = excluded.mode, updated_at = excluded.updated_at",
            [mode],
        )?;
        // Released before reading back, which would otherwise wait on the
        // connection lock this call still holds.
        drop(connection);
        self.release_check_state()
    }

    /// Records what a check saw, without disturbing the operator's choice.
    ///
    /// A failed check keeps the previous offer rather than erasing it: an
    /// origin that is unreachable today does not make yesterday's answer
    /// untrue, and blanking the card would read as "nothing available".
    ///
    /// # Errors
    /// Rejects an outcome this product does not record.
    pub fn record_release_check(
        &self,
        outcome: &str,
        offer: Option<&str>,
        now: i64,
    ) -> Result<ReleaseCheckState, TaskStoreError> {
        if !matches!(outcome, "offered" | "current" | "unreachable" | "rejected") {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{outcome} is not a release check outcome"
            )));
        }
        let connection = self.connection()?;
        connection.execute(
            "UPDATE release_check_preferences
             SET last_checked_at = ?2,
                 last_outcome = ?1,
                 last_offer = COALESCE(?3, last_offer),
                 updated_at = ?2
             WHERE operator_id = (
                 SELECT hive.operator_id
                 FROM local_hive_identity local
                 JOIN hives hive ON hive.id = local.hive_id
                 WHERE local.singleton = 1
             )",
            params![outcome, now, offer],
        )?;
        drop(connection);
        self.release_check_state()
    }

    /// Creates a validated draft and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task_with_details_as(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let title = title.trim();
        let description = description.trim();
        let workspace = workspace.trim();
        validate_text(title, workspace)?;
        validate_description(description)?;
        let id = TaskId::new();
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO tasks (id, hive_id, title, description, priority, workspace, state, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'draft',
                     COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
            params![
                id.to_string(),
                hive_id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, actor_kind, actor_id)
             VALUES (?1, 'created', 'draft', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Appends a correction to a task's record WITHOUT changing its state.
    ///
    /// A handoff that was true when written stops being true, and until now the
    /// only way to say so was to leave the state and come back — Review to
    /// Active to Review. That works, and a worker did exactly that on
    /// 2026-08-26, but it takes finished work out of Queen's review queue and
    /// makes it read as restarted. The cost of correcting yourself should not
    /// be losing your place.
    ///
    /// APPENDS, NEVER REPLACES. The original note was not wrong, it was
    /// outdated, and those are different things worth keeping apart. Anyone
    /// reading later needs to see what was believed and when, not a tidied
    /// version where the belief was always current. That is the same reason a
    /// superseded exemption keeps its reason with a prefix rather than losing
    /// it.
    ///
    /// Deliberately not a state transition. Same-state moves are refused across
    /// the whole machine, and that refusal does real work — it is what makes
    /// "the state changed" mean something. Widening it so a note could be
    /// corrected would trade a narrow gap for a weaker rule.
    ///
    /// # Errors
    /// Returns an error when the note is empty or over the limit, or when the
    /// task cannot be read.
    pub fn append_task_correction(
        &self,
        id: TaskId,
        note: &str,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let note = note.trim();
        if note.is_empty() || note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: String = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        // The state is recorded on the correction so a reader can see the
        // record was amended in place rather than moved.
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, note, actor_kind, actor_id)
             VALUES (?1, 'corrected', ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                state,
                note,
                actor.kind.to_string(),
                actor.id.as_deref()
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Appends a correction of FACT to a task's description.
    ///
    /// The operator's ruling on decision 01a04108: "Facts govern, scope and
    /// acceptance never do", by "Worker and Queen, always attributed".
    ///
    /// APPEND ONLY. There is no update and no delete, deliberately and for the
    /// author too: a second thought is another amendment. That is what keeps the
    /// property immutability was protecting — what a worker was told when it
    /// picked the task up can still be reconstructed exactly.
    ///
    /// WHAT THIS CANNOT DO, said here because the limit is easy to forget once
    /// the tool exists: it cannot tell a correction of fact from an attempt to
    /// change what the task is FOR. Both are free text from the same author.
    /// Any classifier would be a heuristic over prose and would fail silently
    /// toward accepting a scope change, which is the failure being removed. What
    /// IS structural is that the original can never be erased and never stops
    /// governing scope, and that every amendment carries its author.
    ///
    /// # Errors
    /// Returns an error when the task or worker is unknown, or the body is empty
    /// or over the note limit.
    pub fn amend_task_facts(
        &self,
        id: TaskId,
        author: WorkerId,
        body: &str,
    ) -> Result<TaskAmendment, TaskStoreError> {
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let author_name: String = transaction
            .query_row(
                "SELECT name FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL",
                [author.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND removed_at IS NULL)",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        let amendment_id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT INTO task_amendments (id, task_id, author_worker_id, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![amendment_id, id.to_string(), author.to_string(), body],
        )?;
        let created_at: i64 = transaction.query_row(
            "SELECT created_at FROM task_amendments WHERE id = ?1",
            [&amendment_id],
            |row| row.get(0),
        )?;
        // The SAME transaction and the SAME timestamp, deliberately. Two writes
        // that can disagree are how the trail and the table drift apart, and
        // occurred_at is read as a clock by two attention flags -- stamping it
        // with unixepoch() here instead of the amendment's own created_at would
        // shift those clocks by however long the transaction took.
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, occurred_at, actor_kind, actor_id)
             VALUES (?1, 'amended', ?2, ?3, 'worker', ?4)",
            params![id.to_string(), body, created_at, author.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(TaskAmendment {
            id: amendment_id,
            author_worker_id: author,
            author_name,
            body: body.to_owned(),
            created_at,
        })
    }

    /// Appends a worker's note to a task's trail, changing nothing else.
    ///
    /// A worker mid-task had exactly two ways to say anything: finish, or
    /// change state. So one that was asked to state a prediction BEFORE
    /// writing the code moved its own task to Blocked, wrote the note, and
    /// moved it back — leaving the board saying BLOCKED about work that was
    /// not blocked, which `blocked_work_unattended_attention` and Queen's
    /// triage both read. The discipline the fleet most wants cost a false
    /// attention row to exercise.
    ///
    /// DELIBERATELY NOT AN AMENDMENT, though the plumbing is nearly identical.
    /// `amend_task_facts` also writes `task_amendments`, which every task
    /// listing carries beside the description under "believe the amendment
    /// where it contradicts". A prediction is not a correction of fact: the
    /// outcome may falsify it, and filing it there would tell every later
    /// reader to believe something that turned out wrong. This writes the
    /// trail only.
    ///
    /// AND DELIBERATELY NOT AN ACTION. `last_task_action_source!` counts
    /// `corrected`, `details_updated` and `amended`; `noted` is absent, so a
    /// note does not push back the stale-work clock. A worker that writes
    /// notes and does nothing else is still reported as unchanged after the
    /// threshold. That is the mechanical answer to "do not let this become a
    /// way to look busy" — the note buys the record and nothing else.
    ///
    /// # Errors
    /// Returns an error when the note is empty or oversized, the author is not
    /// a live worker, or the task does not exist.
    pub fn record_task_note(
        &self,
        id: TaskId,
        author: WorkerId,
        body: &str,
    ) -> Result<i64, TaskStoreError> {
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let author_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_profiles WHERE id = ?1 AND archived_at IS NULL)",
            [author.to_string()],
            |row| row.get(0),
        )?;
        if !author_exists {
            return Err(TaskStoreError::WorkerNotFound);
        }
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND removed_at IS NULL)",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             VALUES (?1, 'noted', ?2, 'worker', ?3)",
            params![id.to_string(), body, author.to_string()],
        )?;
        // The task's own row is untouched: no state, and no updated_at. Both
        // are load-bearing. updated_at is the stale-work clock, and moving it
        // here would let a note buy the quiet the note is not supposed to buy.
        let sequence = transaction.last_insert_rowid();
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(sequence)
    }

    /// Records that finished work cannot now be shown to be live.
    ///
    /// The operator's ruling, decision 01a04d9f-da18-7d02-b47b-978f0a6b9a01:
    /// "Add a control that records them UNVERIFIABLE". Nineteen tasks finished
    /// before the 2026-08-21 evidence gate and sat in a panel that asked for
    /// evidence while offering no way to give any. The work is ten days old and
    /// was done by workers against other repositories, so an operator asserting
    /// "deployed, sha abc123" today would be attesting to something they did
    /// not do and cannot check -- which is the failure the gate exists to
    /// prevent, arriving through the button meant to help.
    ///
    /// So this asserts the only thing that is actually true: nobody can now
    /// establish where this went. It is NOT evidence and must never be counted
    /// as any; `closed_on_evidence` stays false, and the badge stays honest.
    ///
    /// Deliberately does NOT move the task's state. These are already
    /// completed, and a record about what is knowable should not rewrite what
    /// happened.
    ///
    /// # Errors
    /// Returns an error when the note is empty or oversized, the task does not
    /// exist, or it already carries real evidence -- work whose deployment IS
    /// recorded is not unverifiable, and saying so would be false.
    pub fn record_task_unverifiable(
        &self,
        id: TaskId,
        note: &str,
        recorded_at: i64,
    ) -> Result<bool, TaskStoreError> {
        let note = note.trim();
        if note.is_empty() || note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND removed_at IS NULL)",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        // Work that HAS evidence is not unverifiable. Refusing here rather than
        // silently overwriting keeps the two records from ever disagreeing.
        let has_evidence: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_deployments d WHERE d.task_id = ?1)
                 OR EXISTS(SELECT 1 FROM task_completion_exemptions e
                           WHERE e.task_id = ?1 AND e.approved_at IS NOT NULL AND e.withdrawn_at IS NULL)",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if has_evidence {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO task_unverifiable_closures (task_id, note, recorded_at)
             VALUES (?1, ?2, ?3)",
            params![id.to_string(), note, recorded_at],
        )? == 1;
        if changed {
            transaction.execute(
                "INSERT INTO task_activity (task_id, kind, note, occurred_at, actor_kind)
                 VALUES (?1, 'noted', ?2, ?3, 'operator')",
                params![
                    id.to_string(),
                    format!("Recorded as unverifiable: {note}"),
                    recorded_at
                ],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Amendments for MANY tasks at once, keyed by task.
    ///
    /// One query rather than one per task: the listing that needs this is
    /// Queen's whole queue, and an N+1 there would make reading the board more
    /// expensive the more work it holds.
    ///
    /// # Errors
    /// Returns an error when the query fails.
    pub fn amendments_for_tasks(
        &self,
        ids: &[TaskId],
    ) -> Result<std::collections::HashMap<TaskId, Vec<TaskAmendment>>, TaskStoreError> {
        let mut grouped: std::collections::HashMap<TaskId, Vec<TaskAmendment>> =
            std::collections::HashMap::new();
        if ids.is_empty() {
            return Ok(grouped);
        }
        let connection = self.connection()?;
        let placeholders = vec!["?"; ids.len()].join(",");
        let mut statement = connection.prepare(&format!(
            "SELECT a.task_id, a.id, a.author_worker_id, w.name, a.body, a.created_at
             FROM task_amendments a
             JOIN worker_profiles w ON w.id = a.author_worker_id
             WHERE a.task_id IN ({placeholders})
             ORDER BY a.created_at, a.id"
        ))?;
        let bound = ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                TaskAmendment {
                    id: row.get(1)?,
                    author_worker_id: WorkerId::from_str(&row.get::<_, String>(2)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    author_name: row.get(3)?,
                    body: row.get(4)?,
                    created_at: row.get(5)?,
                },
            ))
        })?;
        for row in rows {
            let (task_id, amendment) = row?;
            let Ok(task_id) = TaskId::from_str(&task_id) else {
                continue;
            };
            grouped.entry(task_id).or_default().push(amendment);
        }
        Ok(grouped)
    }

    /// Every amendment on a task, oldest first.
    ///
    /// Oldest first because they are read as a sequence: the original, then what
    /// was learned, in the order it was learned.
    ///
    /// # Errors
    /// Returns an error when the query fails.
    pub fn task_amendments(&self, id: TaskId) -> Result<Vec<TaskAmendment>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT a.id, a.author_worker_id, w.name, a.body, a.created_at
             FROM task_amendments a
             JOIN worker_profiles w ON w.id = a.author_worker_id
             WHERE a.task_id = ?1
             ORDER BY a.created_at, a.id",
        )?;
        let amendments = statement
            .query_map([id.to_string()], |row| {
                Ok(TaskAmendment {
                    id: row.get(0)?,
                    author_worker_id: WorkerId::from_str(&row.get::<_, String>(1)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    author_name: row.get(2)?,
                    body: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(amendments)
    }

    /// Returns an open task to the Hive queue without stopping its former worker.
    ///
    /// # Errors
    ///
    /// Returns an error when the task does not exist, is already completed, or
    /// the unassignment transaction cannot be committed.
    pub fn unassign_task(&self, id: TaskId) -> Result<Task, TaskStoreError> {
        self.unassign_task_as(id, &TaskActivityActor::system())
    }

    /// Returns an open task to the Hive queue and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for an unknown or completed task or unavailable persistence.
    pub fn unassign_task_as(
        &self,
        id: TaskId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let state = state.ok_or(TaskStoreError::NotFound)?;
        if state == TaskState::Completed.to_string() {
            return Err(TaskStoreError::CompletedTask);
        }
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments WHERE task_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        review_answers::invalidate_pending_request(&transaction, id, None)?;
        transaction.execute(
            "UPDATE tasks SET assigned_worker_id = NULL, updated_at = unixepoch() WHERE id = ?1",
            [id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'unassigned', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Lists tasks with their current active assignment.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    /// The task projection, shared so the board list and the settled list cannot
    /// drift apart in what they return.
    const TASK_PROJECTION: &'static str = "
            SELECT t.id, t.hive_id, t.title, t.description, t.priority, t.workspace, t.state,
                   t.assigned_worker_id, a.worker_session_id,
                   (SELECT state FROM task_dispatches td WHERE td.assignment_id = a.id),
                   (SELECT state FROM task_outcome_deliveries outcome WHERE outcome.task_id = t.id
                    AND outcome.target_state = t.state
                    ORDER BY outcome.activity_sequence DESC LIMIT 1),
                   t.position, t.created_at, t.updated_at, t.operator_instruction,
                   EXISTS(SELECT 1 FROM task_deployments d WHERE d.task_id = t.id),
                   EXISTS(SELECT 1 FROM task_deployments d WHERE d.task_id = t.id)
                     OR EXISTS(SELECT 1 FROM task_completion_exemptions e
                               WHERE e.task_id = t.id AND e.approved_at IS NOT NULL AND e.withdrawn_at IS NULL),
                   EXISTS(SELECT 1 FROM task_activity worked
                          WHERE worked.task_id = t.id AND worked.actor_kind = 'worker'),
                   EXISTS(SELECT 1 FROM task_unverifiable_closures u WHERE u.task_id = t.id),
                   EXISTS(SELECT 1 FROM task_returned_reviews r
                          WHERE r.task_id = t.id AND r.answered_at IS NULL
                            AND r.request_message_id IS NOT NULL
                            AND r.request_worker_id = t.assigned_worker_id),
                   -- Reviewed work waiting on a ruling is the OPERATOR's, not
                   -- Queen's. Read from the decision rather than stored beside
                   -- it, so it unsets itself the moment they answer.
                   EXISTS(SELECT 1 FROM decision_requests dr
                          WHERE dr.task_id = t.id AND dr.state = 'pending')
            FROM tasks t
            LEFT JOIN task_assignments a
              ON a.task_id = t.id AND a.released_at IS NULL
";

    /// Work that is finished and needs nobody: abandoned, or completed with
    /// evidence recorded, or completed and recorded unverifiable.
    ///
    /// This is the disjunction `closed_on_evidence` and `closed_unverifiable`
    /// are derived from in the projection above, and it deliberately does NOT
    /// include the Jira-owned case the board also files under completed. That
    /// one depends on Jira link data the server does not hold here, and a task
    /// this query hides that the board would have shown as unverified lands in
    /// neither list and disappears from the screen. Leaving those few in the
    /// board list is the safe direction to be wrong in.
    const SETTLED_PREDICATE: &'static str = "
        (t.state = 'abandoned'
         OR (t.state = 'completed'
             AND (EXISTS(SELECT 1 FROM task_deployments d WHERE d.task_id = t.id)
                  OR EXISTS(SELECT 1 FROM task_completion_exemptions e
                            WHERE e.task_id = t.id AND e.approved_at IS NOT NULL AND e.withdrawn_at IS NULL)
                  OR EXISTS(SELECT 1 FROM task_unverifiable_closures u WHERE u.task_id = t.id))))
    ";

    /// THE BROWSER BOARD'S working set: everything except settled work.
    ///
    /// Deliberately NOT `list_tasks`. That name is what the agent surface reads
    /// through `list_visible_tasks`, and narrowing it would have quietly taken
    /// settled work out of what Queen and every worker can see — a change to
    /// the agent surface that nobody asked for, to fix a cost in the browser.
    /// The two callers want different things and now say so.
    ///
    /// Settled work is the large majority of a long-lived Hive and the board
    /// renders it inside a collapsed panel. Measured on the operator's Hive
    /// 2026-09-02: 561 tasks and 1,711 KB of title and description text, of
    /// which 462 tasks and 1,411 KB were settled. This endpoint is polled every
    /// 30 seconds, so shipping them cost about 3.4 MB a minute to render a
    /// board whose actionable half is 99 rows.
    ///
    /// `list_settled_tasks` serves the rest, once, rather than twice a minute.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_board_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        self.list_tasks_where(&format!("AND NOT {}", Self::SETTLED_PREDICATE))
    }

    /// Every task on the Hive. What the agent surface reads.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        self.list_tasks_where("")
    }

    /// Settled work, which the board fetches when its completed panel is opened
    /// rather than on every poll.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_settled_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        self.list_tasks_where(&format!("AND {}", Self::SETTLED_PREDICATE))
    }

    fn list_tasks_where(&self, extra: &str) -> Result<Vec<Task>, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "{projection}\n            WHERE t.removed_at IS NULL {extra}\n{ordering}",
            projection = Self::TASK_PROJECTION,
            ordering = "            ORDER BY CASE t.state WHEN 'completed' THEN 1 ELSE 0 END,\n                     CASE t.state WHEN 'completed' THEN -t.updated_at ELSE t.position END,\n                     t.id",
        );
        let mut statement = connection.prepare(&sql)?;
        statement
            .query_map([], task_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Lists the newest removed local tasks that can safely return to this Hive.
    ///
    /// Jira-backed work is deliberately absent because Jira remains its lifecycle
    /// authority. The bounded result prevents old cleanup history from becoming
    /// another unbounded board.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_removed_local_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "{projection}
            WHERE t.removed_at IS NOT NULL
              AND NOT EXISTS (SELECT 1 FROM jira_issue_links jira WHERE jira.task_id = t.id)
            ORDER BY t.removed_at DESC, t.id
            LIMIT 100",
            projection = Self::TASK_PROJECTION,
        );
        let mut statement = connection.prepare(&sql)?;
        statement
            .query_map([], task_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Loads one task and its current active assignment.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown task or a persistence error.
    pub fn get_task(&self, id: TaskId) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "{projection}\n            WHERE t.id = ?1 AND t.removed_at IS NULL",
            projection = Self::TASK_PROJECTION,
        );
        connection
            .query_row(&sql, [id.to_string()], task_from_row)
            .optional()?
            .ok_or(TaskStoreError::NotFound)
    }

    /// Lists a bounded, chronological activity history for one task.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown task or a persistence error.
    pub fn list_task_activity(
        &self,
        id: TaskId,
        limit: usize,
    ) -> Result<TaskActivityPage, TaskStoreError> {
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM tasks WHERE id = ?1",
                [id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        let limit = limit.clamp(1, MAX_TASK_ACTIVITY_PAGE);
        let query_limit = i64::try_from(limit.saturating_add(1))
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT sequence, task_id, kind, from_state, to_state, note, occurred_at,
                    actor_kind, actor_id
             FROM task_activity WHERE task_id = ?1
             ORDER BY sequence DESC LIMIT ?2",
        )?;
        let mut activity = statement
            .query_map(params![id.to_string(), query_limit], task_activity_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let truncated = activity.len() > limit;
        activity.truncate(limit);
        activity.reverse();
        Ok(TaskActivityPage {
            events: activity,
            truncated,
        })
    }

    /// Lists the newest durable task events across the local Hive.
    ///
    /// # Errors
    /// Returns a persistence error when the local Hive identity or activity rows
    /// cannot be read.
    pub fn list_recent_task_activity(
        &self,
        limit: usize,
    ) -> Result<TaskActivityPage, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let connection = self.connection()?;
        let limit = limit.clamp(1, MAX_TASK_ACTIVITY_PAGE);
        let query_limit = i64::try_from(limit.saturating_add(1))
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT activity.sequence, activity.task_id, activity.kind,
                    activity.from_state, activity.to_state, activity.note,
                    activity.occurred_at, activity.actor_kind, activity.actor_id
             FROM task_activity activity
             JOIN tasks task ON task.id = activity.task_id
             WHERE task.hive_id = ?1 AND task.removed_at IS NULL
             ORDER BY activity.sequence DESC LIMIT ?2",
        )?;
        let mut activity = statement
            .query_map(
                params![hive_id.to_string(), query_limit],
                task_activity_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let truncated = activity.len() > limit;
        activity.truncate(limit);
        activity.reverse();
        Ok(TaskActivityPage {
            events: activity,
            truncated,
        })
    }

    /// Moves one open task to the front of the delivery order.
    ///
    /// DELIVERY ORDER IS `position`, NOT `priority`. `deliverable_briefings`
    /// orders by `t.position`, and the head-of-line rule that holds a briefing
    /// back orders by `earlier.position` — neither consults priority anywhere.
    /// Priority is carried into the brief so the worker knows how urgent the
    /// work is; it does not decide what arrives first.
    ///
    /// Which left a reviewer with no lever at all. Queen sets priority
    /// deliberately, watched a HIGH item sit eight deep behind five normal ones
    /// because she filed it last, and the only way she found to move it forward
    /// was to BLOCK a lower-value task to shorten the queue ahead of it. That is
    /// an honest use of Blocked and plainly a workaround — it makes the board
    /// lie about why something is waiting.
    ///
    /// Built on `reorder_open_tasks` rather than writing positions directly, so
    /// the same validation applies: the full open set, no duplicates, nothing
    /// invented. A promote is a reorder with one element moved, and it cannot
    /// corrupt an order the way a hand-supplied list can.
    ///
    /// # Errors
    /// Returns `NotFound` when the task is not open, and propagates the
    /// reordering rules otherwise.
    pub fn promote_open_task(&self, task_id: TaskId) -> Result<Vec<Task>, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let order = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT id FROM tasks
                 WHERE hive_id = ?1 AND state NOT IN ('completed','abandoned')
                       AND removed_at IS NULL
                 ORDER BY position, id",
            )?;
            statement
                .query_map([hive_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let wanted = task_id.to_string();
        if !order.iter().any(|id| id == &wanted) {
            return Err(TaskStoreError::NotFound);
        }
        let mut promoted = vec![task_id];
        promoted.extend(
            order
                .iter()
                .filter(|id| *id != &wanted)
                .filter_map(|id| TaskId::from_str(id).ok()),
        );
        self.reorder_open_tasks(&promoted)
    }

    /// Replaces the complete open-task order for the local Hive atomically.
    ///
    /// # Errors
    /// Rejects incomplete, duplicate, oversized, foreign-Hive, or completed-task input.
    pub fn reorder_open_tasks(&self, task_ids: &[TaskId]) -> Result<Vec<Task>, TaskStoreError> {
        if task_ids.len() > MAX_OPEN_TASKS_PER_ORDER {
            return Err(TaskStoreError::InvalidTaskOrder);
        }
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let expected = {
            let mut statement = transaction.prepare(
                "SELECT id FROM tasks
                 WHERE hive_id = ?1 AND state NOT IN ('completed','abandoned')
                       AND removed_at IS NULL
                 ORDER BY position, id",
            )?;
            statement
                .query_map([hive_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let supplied = task_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        let unique = supplied.iter().collect::<HashSet<_>>();
        let expected_set = expected.iter().collect::<HashSet<_>>();
        if supplied.len() != expected.len()
            || unique.len() != supplied.len()
            || unique != expected_set
        {
            return Err(TaskStoreError::InvalidTaskOrder);
        }
        for (position, task_id) in supplied.iter().enumerate() {
            let position = i64::try_from(position)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
            transaction.execute(
                "UPDATE tasks SET position = ?2, updated_at = unixepoch() WHERE id = ?1",
                params![task_id, position],
            )?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.list_tasks()
    }

    /// Removes one task from the active Hive without deleting its source,
    /// attachments, Jira identity, or audit history.
    ///
    /// Active and review work must first be deliberately stopped or completed
    /// so a running worker cannot continue work the operator can no longer see.
    /// Jira-backed tasks retain their source link and therefore cannot be
    /// recreated by the next synchronization pass.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown/already removed task and rejects work
    /// currently in progress or review.
    pub fn remove_task_as(
        &self,
        id: TaskId,
        actor: &TaskActivityActor,
        reason: &str,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        if matches!(state.as_str(), "active" | "review") {
            return Err(TaskStoreError::ActiveTaskCannotBeRemoved);
        }
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments WHERE task_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM task_outcome_deliveries WHERE task_id = ?1 AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE tasks SET assigned_worker_id = NULL, removed_at = unixepoch(),
                 updated_at = unixepoch() WHERE id = ?1 AND removed_at IS NULL",
            [id.to_string()],
        )?;
        // The reason is the record. A retired task that does not say why it was
        // retired is the same dead end as one mislabelled Blocked: the next
        // reader has to guess whether it can come back.
        let reason = reason.trim();
        let note = if reason.is_empty() {
            "Removed from this Hive".to_owned()
        } else {
            format!("Removed from this Hive: {reason}")
        };
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             VALUES (?1, 'removed', ?2, ?3, ?4)",
            params![
                id.to_string(),
                note,
                actor.kind.to_string(),
                actor.id.as_deref()
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(())
    }

    /// Restores one removed local task to the active Hive board.
    ///
    /// Jira-backed work cannot use this local recovery path because its remote
    /// issue remains authoritative. Open work returns at the end of the queue;
    /// completed work returns to completed history.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown or already-active task, and refuses
    /// Jira-backed work.
    pub fn restore_task_as(
        &self,
        id: TaskId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let jira_backed = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM jira_issue_links WHERE task_id = ?1)
                 FROM tasks WHERE id = ?1 AND removed_at IS NOT NULL",
                [id.to_string()],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        if jira_backed {
            return Err(TaskStoreError::JiraTaskCannotBeRestored);
        }
        transaction.execute(
            "UPDATE tasks
             SET removed_at = NULL,
                 position = CASE WHEN state IN ('completed','abandoned') THEN position ELSE (
                     SELECT COALESCE(MAX(active.position), -1) + 1
                     FROM tasks active
                     WHERE active.hive_id = tasks.hive_id
                       AND active.removed_at IS NULL
                       AND active.state NOT IN ('completed','abandoned')
                 ) END,
                 updated_at = unixepoch()
             WHERE id = ?1 AND removed_at IS NOT NULL",
            [id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
             VALUES (?1, 'restored', 'Restored to this Hive', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Replaces the supplied task details and records one atomic activity event.
    ///
    /// # Errors
    /// Returns an error for an empty update, invalid content, an unknown task, or unavailable persistence.
    pub fn update_task_details(
        &self,
        id: TaskId,
        update: &TaskDetailsUpdate,
    ) -> Result<Task, TaskStoreError> {
        self.update_task_details_as(id, update, &TaskActivityActor::system())
    }

    /// Replaces supplied task details and records their authenticated origin.
    ///
    /// # Errors
    /// Returns an error for an invalid or empty update, unknown task, or unavailable persistence.
    pub fn update_task_details_as(
        &self,
        id: TaskId,
        update: &TaskDetailsUpdate,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        if update.title.is_none()
            && update.description.is_none()
            && update.priority.is_none()
            && update.workspace.is_none()
            && update.operator_instruction.is_none()
        {
            return Err(TaskStoreError::EmptyTaskDetailsUpdate);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT title, description, priority, workspace, operator_instruction
                 FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        let title = update
            .title
            .as_deref()
            .map_or(current.0.as_str(), str::trim);
        let description = update
            .description
            .as_deref()
            .map_or(current.1.as_str(), str::trim);
        let priority = update.priority.unwrap_or(
            TaskPriority::from_str(&current.2)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
        );
        let workspace = update
            .workspace
            .as_deref()
            .map_or(current.3.as_str(), str::trim);
        let operator_instruction = update
            .operator_instruction
            .as_deref()
            .map_or(current.4.as_str(), str::trim);
        validate_text(title, workspace)?;
        validate_description(description)?;
        validate_operator_instruction(operator_instruction)?;
        transaction.execute(
            "UPDATE tasks
             SET title = ?2, description = ?3, priority = ?4, workspace = ?5,
                 operator_instruction = ?6, updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace,
                operator_instruction
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'details_updated', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Writes a consistent online backup to a separate `SQLite` file.
    ///
    /// # Errors
    /// Returns an error when the destination or `SQLite` backup operation fails.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = self.connection()?;
        connection.backup("main", path, None)?;
        Ok(())
    }

    /// The schema version this database is actually at, right now.
    ///
    /// Exposed because a reload has to compare the LIVE version against the one
    /// the checkout declares, and it cannot use `CURRENT_SCHEMA_VERSION` for the
    /// live side: this binary migrated the database to its own version at
    /// startup, so that comparison reports "no migration" for every reload,
    /// including the ones that carry one.
    ///
    /// # Errors
    /// Returns a persistence error when the pragma cannot be read.
    pub fn schema_version(&self) -> Result<i64, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Runs `SQLite`'s quick integrity check against the live database.
    ///
    /// # Errors
    /// Returns an integrity or persistence error when the check is not successful.
    pub fn verify_integrity(&self) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        let result: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(TaskStoreError::IntegrityFailure(result))
        }
    }

    /// Applies one permitted task transition without a handoff note.
    ///
    /// # Errors
    /// Returns an error for an unknown task, rejected transition, or persistence failure.
    pub fn transition_task(&self, id: TaskId, target: TaskState) -> Result<Task, TaskStoreError> {
        self.transition_task_inner(id, target, "", None, &TaskActivityActor::system())
    }

    /// Applies an operator or Queen transition with a bounded audit note.
    ///
    /// # Errors
    /// Returns an error for invalid content, lifecycle, or persistence.
    pub fn transition_task_with_note(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
    ) -> Result<Task, TaskStoreError> {
        self.transition_task_with_note_as(id, target, note, &TaskActivityActor::system())
    }

    /// Applies a task transition and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for invalid content, lifecycle, or persistence.
    pub fn transition_task_with_note_as(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        self.transition_task_inner(id, target, note, None, actor)
    }

    /// Applies a transition only while the task remains bound to one live worker session.
    ///
    /// This is the guarded coordination path for Queen: a stale session or a
    /// concurrent worker exit fails before lifecycle state changes.
    ///
    /// # Errors
    /// Returns `WorkerSessionNotActive` for a stale assignment and otherwise
    /// propagates lifecycle, note, capacity, or persistence failures.
    pub fn transition_assigned_task_with_note_as(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        session_id: WorkerSessionId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        self.transition_task_inner(id, target, note, Some(session_id), actor)
    }

    /// Applies an assigned worker transition and queues Blocked or Review for Queen atomically.
    ///
    /// # Errors
    /// Returns an error for a stale assignment, invalid content, capacity, or persistence.
    pub fn transition_worker_task(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        session_id: WorkerSessionId,
    ) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        let worker_id = connection
            .query_row(
                "SELECT worker_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        drop(connection);
        self.transition_task_inner(
            id,
            target,
            note,
            Some(session_id),
            &TaskActivityActor::worker(worker_id),
        )
    }

    fn transition_task_inner(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        reporting_session_id: Option<WorkerSessionId>,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        if note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = reportable_task_state(&transaction, id, reporting_session_id)?;
        if !current.can_transition_to(target) {
            // ONLY the completed-to-completed case gets the richer answer.
            // ready to ready and blocked to blocked are ordinary mistakes, and
            // the plain rule is the right thing to tell someone who made one —
            // dressing every same-state refusal up as a race would bury the one
            // case that actually is one.
            if current == TaskState::Completed
                && target == TaskState::Completed
                && let Some((seconds_ago, closed_by)) = completion_provenance(&transaction, id)?
            {
                return Err(TaskStoreError::TaskAlreadyCompleted {
                    seconds_ago,
                    closed_by,
                });
            }
            return Err(TaskStoreError::InvalidTransition {
                from: current,
                to: target,
            });
        }
        if target == TaskState::Active {
            ensure_worker_has_no_other_active_task(&transaction, id)?;
        }
        jira::queue_jira_transition(&transaction, id, target)?;
        if current == TaskState::Review && target != TaskState::Review {
            review_answers::invalidate_pending_request(&transaction, id, None)?;
        }
        transaction.execute(
            "DELETE FROM task_outcome_deliveries WHERE task_id = ?1 AND state = 'queued'",
            [id.to_string()],
        )?;
        let block_deadline = block_deadline_for(target, note);
        transaction.execute(
            "UPDATE tasks SET state = ?2, blocked_until = ?3, updated_at = unixepoch()
             WHERE id = ?1",
            params![id.to_string(), target.to_string(), block_deadline],
        )?;
        // Work sent back to a worker owes it a briefing again. Queen's only
        // non-completing exit from Review used to be Active, and that
        // transition enqueued nothing: the task changed column, the worker was
        // never told, and it sat in Active looking like work nobody was doing.
        // The same held for Blocked -> Active. Delivery already accepts an
        // active task — `deliverable_briefings` takes 'ready' or 'active', and
        // still holds a brief back while the worker has other active work or
        // the operator is at its terminal — so this only closes the enqueue
        // side.
        //
        // Re-entry only. A worker moving its own Ready -> Active is starting
        // the work it was just briefed on, and re-arming there would replay a
        // briefing it has already acted on.
        let returning_to_a_worker = target == TaskState::Active
            && matches!(current, TaskState::Review | TaskState::Blocked);
        if returning_to_a_worker {
            rearm_briefing_for_returned_work(&transaction, id)?;
        }
        if target == TaskState::Ready || returning_to_a_worker {
            let queued: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
                [],
                |row| row.get(0),
            )?;
            if queued >= 256 {
                return Err(TaskStoreError::TaskDispatchQueueFull);
            }
            transaction.execute(
                "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
                 SELECT assignment.id, assignment.task_id, session.worker_id, 'queued'
                 FROM task_assignments assignment
                 JOIN worker_sessions session
                   ON session.session_id = assignment.worker_session_id
                  AND session.ended_at IS NULL
                 WHERE assignment.task_id = ?1 AND assignment.released_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM task_dispatches dispatch
                       WHERE dispatch.assignment_id = assignment.id
                   )",
                [id.to_string()],
            )?;
        }
        // A transition from the assigned worker is stronger acknowledgement
        // than an ambiguous PTY submit receipt: she could only advance this
        // task after retrieving its authoritative assignment through MCP.
        if reporting_session_id.is_some() {
            acknowledge_task_dispatch(&transaction, id)?;
        }
        federation_tasks::record_local_apiary_task_lifecycle_intent(&transaction, id, target)?;
        transaction.execute(
            "INSERT INTO task_activity (
                 task_id, kind, from_state, to_state, note, actor_kind, actor_id
             ) VALUES (?1, 'state_changed', ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                current.to_string(),
                target.to_string(),
                note,
                actor.kind.to_string(),
                actor.id.as_deref(),
            ],
        )?;
        let activity_sequence = transaction.last_insert_rowid();
        if let Some(session_id) = reporting_session_id
            && matches!(target, TaskState::Blocked | TaskState::Review)
        {
            insert_task_outcome(&transaction, id, target, session_id, activity_sequence)?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }
    /// Replaces the current durable worker owner and binds its active session when available.
    ///
    /// # Errors
    /// Returns an error for an unknown or completed task or unavailable persistence.
    pub fn assign_task(
        &self,
        id: TaskId,
        session_id: WorkerSessionId,
    ) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        let worker_id = connection
            .query_row(
                "SELECT worker_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        drop(connection);
        self.assign_task_to_worker(id, worker_id)
    }

    /// Assigns a task to a stable worker profile, including while she is sleeping.
    ///
    /// A running incarnation receives one queued briefing. A sleeping worker is
    /// bound and briefed atomically the next time her profile starts.
    ///
    /// # Errors
    /// Returns an error for unknown workers, completed tasks, exhausted queue
    /// capacity, invalid persisted identities, or unavailable storage.
    pub fn assign_task_to_worker(
        &self,
        id: TaskId,
        worker_id: WorkerId,
    ) -> Result<Task, TaskStoreError> {
        self.assign_task_to_worker_as(id, worker_id, &TaskActivityActor::system())
    }

    /// Assigns a task and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for unknown workers, completed work, queue capacity, or persistence.
    pub fn assign_task_to_worker_as(
        &self,
        id: TaskId,
        worker_id: WorkerId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let state = state.ok_or(TaskStoreError::NotFound)?;
        if state == TaskState::Completed.to_string() {
            return Err(TaskStoreError::CompletedTask);
        }
        let worker: Option<Option<String>> = transaction
            .query_row(
                "SELECT session.session_id
                 FROM worker_profiles profile
                 LEFT JOIN worker_sessions session
                   ON session.worker_id = profile.id AND session.ended_at IS NULL
                 WHERE profile.id = ?1 AND profile.role != 'queen'",
                [worker_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let session_id = worker.ok_or(TaskStoreError::WorkerNotFound)?;
        let worker_is_sleeping = session_id.is_none();
        transaction.execute(
            "DELETE FROM task_outcome_deliveries WHERE task_id = ?1 AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments WHERE task_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        review_answers::invalidate_pending_request(&transaction, id, Some(worker_id))?;
        transaction.execute(
            "UPDATE tasks
             SET assigned_worker_id = ?2,
                 workspace = (SELECT workspace FROM worker_profiles WHERE id = ?2),
                 updated_at = unixepoch()
             WHERE id = ?1",
            params![id.to_string(), worker_id.to_string()],
        )?;
        if let Some(session_id) = session_id {
            let assignment_id = Uuid::now_v7().to_string();
            transaction.execute(
                "INSERT INTO task_assignments (id, task_id, worker_session_id)
                 VALUES (?1, ?2, ?3)",
                params![assignment_id, id.to_string(), session_id],
            )?;
            // Active work is briefed on re-assignment too, and that is the
            // repair rather than a side effect.
            //
            // Re-assigning is Queen's documented lever for stranded work, and
            // for a task already Active it did nothing: it bound a session and
            // created no briefing, so the worker woke into silence. The only
            // route that actually redelivered was walking the task
            // active -> blocked -> ready, which lies on the board while it
            // happens, because there is no active -> ready edge. Measured
            // 2026-08-19: a high-priority brief undelivered for 27 hours, and
            // it landed one second after finally being queued.
            //
            // Safe because delivery already covers work that has started —
            // claim_task_dispatches takes 'ready' and 'active' — and because a
            // worker genuinely holding the work is not re-assigned by accident:
            // this runs only when somebody asked for it.
            if state == TaskState::Ready.to_string() || state == TaskState::Active.to_string() {
                let queued: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
                    [],
                    |row| row.get(0),
                )?;
                if queued >= 256 {
                    return Err(TaskStoreError::TaskDispatchQueueFull);
                }
                transaction.execute(
                    "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
                     VALUES (?1, ?2, ?3, 'queued')",
                    params![assignment_id, id.to_string(), worker_id.to_string()],
                )?;
            }
        }
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT assignment_id FROM task_dispatches
                 WHERE state IN ('delivered','uncertain')
                 ORDER BY updated_at DESC, assignment_id DESC LIMIT -1 OFFSET 1024
             )",
            [],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'assigned', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        let assignment_sequence = transaction.last_insert_rowid();
        coordinator::enqueue_queen_worker_wake(
            &transaction,
            id,
            worker_id,
            actor.id.as_deref(),
            assignment_sequence,
            &state,
            worker_is_sleeping,
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Detaches every process binding owned by one stopped worker session.
    ///
    /// Stable worker ownership remains on the task and is rebound on restart.
    ///
    /// # Errors
    /// Returns an error when the assignment history cannot be updated atomically.
    pub fn release_session_assignments(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut task_ids = {
            let mut statement = transaction.prepare(
                "SELECT task_id FROM task_assignments
                 WHERE worker_session_id = ?1 AND released_at IS NULL",
            )?;
            statement
                .query_map([session_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        task_ids.sort_unstable();
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments
                 WHERE worker_session_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [session_id.to_string()],
        )?;
        for task_id in &task_ids {
            transaction.execute(
                "UPDATE task_assignments SET released_at = unixepoch()
                 WHERE task_id = ?1 AND worker_session_id = ?2 AND released_at IS NULL",
                params![task_id, session_id.to_string()],
            )?;
            transaction.execute(
                "UPDATE tasks SET updated_at = unixepoch() WHERE id = ?1",
                [task_id],
            )?;
        }
        if !task_ids.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(task_ids.len())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
}

/// Proves a backup file is the database it claims to be, WITHOUT migrating it.
///
/// Deliberately not `TaskStore::open`: opening runs migrations forward, so
/// verifying a backup that way would rewrite the very thing being kept as the
/// escape route from a migration. This opens the file plainly and asks it two
/// questions.
///
/// Writing bytes is not the same as having a backup. A file that exists,
/// nobody reads, and turns out to be short is worse than a refusal at the time
/// it was taken, because by then the migration has already run.
///
/// # Errors
/// Returns a persistence error when the file cannot be opened, reports a
/// different schema version than expected, or fails an integrity check.
pub fn verify_backup_at(path: &Path, expected_version: i64) -> Result<(), TaskStoreError> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != expected_version {
        return Err(TaskStoreError::IntegrityFailure(format!(
            "the backup reports schema version {version} where the live database is at {expected_version}"
        )));
    }
    let check: String = connection.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
    if check == "ok" {
        Ok(())
    } else {
        Err(TaskStoreError::IntegrityFailure(check))
    }
}

/// When this task was closed and by whom, for an error that names the event
/// rather than the rule.
///
/// Read from `task_activity` rather than from the task row, because the row
/// records only that it is closed. The activity ledger records the transition
/// that closed it, the actor who caused it, and when — which is exactly the
/// three things a reader needs and none of what they were being given.
///
/// Both timestamps come from the database clock, so the elapsed figure is
/// self-consistent even where callers elsewhere inject their own `now`.
fn completion_provenance(
    transaction: &rusqlite::Transaction<'_>,
    id: TaskId,
) -> Result<Option<(i64, String)>, TaskStoreError> {
    Ok(transaction
        .query_row(
            "SELECT MAX(0, unixepoch() - occurred_at), actor_kind, actor_id
             FROM task_activity
             WHERE task_id = ?1 AND to_state = 'completed'
             ORDER BY sequence DESC LIMIT 1",
            [id.to_string()],
            |row| {
                let seconds: i64 = row.get(0)?;
                let kind: String = row.get(1)?;
                let actor: Option<String> = row.get(2)?;
                Ok((seconds, describe_actor(&kind, actor.as_deref())))
            },
        )
        .optional()?)
}

/// Who closed it, in words a reader can act on.
///
/// A bare uuid identifies the actor without telling anyone anything; "the
/// shipped-work sweep" says which mechanism to go and look at.
fn describe_actor(kind: &str, actor: Option<&str>) -> String {
    match (kind, actor) {
        ("worker", Some(id)) => format!("worker {id}"),
        ("worker", None) => "a worker".to_owned(),
        ("operator", _) => "the operator".to_owned(),
        ("jira", _) => "the Jira sync".to_owned(),
        ("email", _) => "the email import".to_owned(),
        // The sweep that closes reviewed work once its evidence lands, which is
        // the actor in every instance of this reported so far.
        _ => "the shipped-work sweep".to_owned(),
    }
}

fn insert_task_outcome(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    target: TaskState,
    session_id: WorkerSessionId,
    activity_sequence: i64,
) -> Result<(), TaskStoreError> {
    let queued: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM task_outcome_deliveries
         WHERE state IN ('queued','dispatching')",
        [],
        |row| row.get(0),
    )?;
    if queued >= 256 {
        return Err(TaskStoreError::TaskOutcomeQueueFull);
    }
    let inserted = transaction.execute(
        "INSERT INTO task_outcome_deliveries (
             id, task_id, activity_sequence, reporting_worker_id,
             recipient_worker_id, target_state, state
         )
         SELECT ?1, ?2, ?3, reporter.id, queen.id, ?5, 'queued'
         FROM worker_sessions session
         JOIN worker_profiles reporter ON reporter.id = session.worker_id
         JOIN worker_profiles queen ON queen.hive_id = reporter.hive_id
             AND queen.role = 'queen'
         WHERE session.session_id = ?4 AND session.ended_at IS NULL",
        params![
            Uuid::now_v7().to_string(),
            task_id.to_string(),
            activity_sequence,
            session_id.to_string(),
            target.to_string(),
        ],
    )?;
    if inserted != 1 {
        return Err(TaskStoreError::IntegrityFailure(
            "worker outcome could not resolve its Queen".into(),
        ));
    }
    transaction.execute(
        "DELETE FROM task_outcome_deliveries WHERE id IN (
             SELECT id FROM task_outcome_deliveries
             WHERE state IN ('delivered','uncertain')
             ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET 1024
         )",
        [],
    )?;
    Ok(())
}

/// Which screen Swarm opens on, chosen once by the operator.
///
/// Not a presentation preference: those are per device class, and the operator
/// asked for one choice used everywhere. A phone landing somewhere a desktop
/// would not is the problem being solved, so storing it per device class would
/// build the problem into the schema.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_start_surface(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             start_surface TEXT NOT NULL DEFAULT 'tasks'
                 CHECK (start_surface IN ('decisions','tasks','workers','apiary','settings')),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );",
    )?;
    transaction.pragma_update(None, "user_version", START_SURFACE_SCHEMA_VERSION)
}

/// Whether this Hive checks an origin for new releases, and what it last saw.
///
/// `mode` starts at `unset` rather than `off` on purpose. ADR 0050 requires
/// that a Hive never contact an origin its owner did not choose, and an
/// unanswered question is not a choice — but neither is a silent default the
/// operator never sees. `unset` lets the control room ask once and lets a
/// Hive that is never asked stay silent, which `off` alone cannot express.
///
/// The last result is cached so the card says something on a machine that is
/// offline, and so a restart does not look like a fresh check.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_release_check(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS release_check_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             mode TEXT NOT NULL DEFAULT 'unset'
                 CHECK (mode IN ('unset','off','daily')),
             last_checked_at INTEGER,
             last_outcome TEXT
                 CHECK (last_outcome IS NULL OR last_outcome IN ('offered','current','unreachable','rejected')),
             last_offer TEXT,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );",
    )?;
    transaction.pragma_update(None, "user_version", RELEASE_CHECK_SCHEMA_VERSION)
}

/// A worker's claim that a task has nothing to deploy, and Queen's approval of
/// it.
///
/// Kept in its own table rather than as a row in `task_deployments` with an
/// empty reference. A completion nobody deployed and a completion that shipped
/// are different claims, and the board has to be able to tell them apart — a
/// blank reference in the deployments table would read as the second while
/// meaning the first.
///
/// The reason is required because "nothing to deploy" is an assertion about the
/// work, and an assertion with no argument behind it is what this whole gate
/// exists to stop.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_completion_exemption(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_completion_exemptions (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             reason TEXT NOT NULL,
             claimed_by_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
             claimed_at INTEGER NOT NULL DEFAULT (unixepoch()),
             approved_at INTEGER,
             approved_by TEXT
                 CHECK (approved_by IS NULL OR approved_by IN ('queen','operator'))
         );",
    )?;
    transaction.pragma_update(None, "user_version", COMPLETION_EXEMPTION_SCHEMA_VERSION)
}

/// Records when a no-deployment claim was overtaken by a recorded deployment.
///
/// A claim that nothing shipped and a deployment record on the same task are a
/// contradiction, and it was invisible: five tasks on this board carry both,
/// none ever examined. The sharpest still reads "PR #418 is open" for work that
/// merged and deployed, and that task was later cited as the gate for the step
/// after it.
///
/// Guarded on the column being absent AND the table existing, because the
/// migration tests rewind `user_version` without rewinding tables — they model
/// a database restored from a backup or half-upgraded, and a migration that
/// assumes the old shape fails on exactly that.
fn migrate_superseded_exemptions(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'task_completion_exemptions')",
        [],
        |row| row.get(0),
    )?;
    let has_column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_completion_exemptions')
                        WHERE name = 'superseded_at')",
        [],
        |row| row.get(0),
    )?;
    if present && !has_column {
        transaction.execute_batch(
            "ALTER TABLE task_completion_exemptions ADD COLUMN superseded_at INTEGER;",
        )?;
    }
    transaction.pragma_update(None, "user_version", SUPERSEDED_EXEMPTION_SCHEMA_VERSION)
}

/// Records that the author of a no-deployment claim took it back.
///
/// WITHDRAWAL IS NOT SUPERSESSION AND MUST NOT REUSE `superseded_at`. That
/// column already means one specific thing — a recorded deployment contradicted
/// the claim — and it is written from two directions: `claim_completion_exemption`
/// marks a claim born onto an already-deployed task, and `record_deployment`
/// marks a standing claim the deployment overtook. Both are statements of FACT
/// made by the store: the claim said nothing shipped, and something did.
///
/// A withdrawal is a different act by a different party. It is a RETRACTION —
/// the author, or a coordinator, saying the claim should never have been made or
/// has stopped being true. Nothing shipped, so supersession cannot express it,
/// and bolting a second meaning onto that column would destroy the distinction
/// the task that asked for this named in its own title.
///
/// Three shapes it has to carry, all observed on this board in one afternoon:
/// a claim FALSE when made and the author could not take it back; a claim
/// APPROVED before anyone noticed it was false, which is the invisible case
/// because approving removes the task from the detector; and a claim TRUE when
/// written and invalidated by the author's own later work.
///
/// Guarded on the columns being absent AND the table existing, because the
/// migration tests rewind `user_version` without rewinding tables.
fn migrate_claim_withdrawal(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'task_completion_exemptions')",
        [],
        |row| row.get(0),
    )?;
    if present {
        let has_withdrawn_at: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_completion_exemptions')
                            WHERE name = 'withdrawn_at')",
            [],
            |row| row.get(0),
        )?;
        if !has_withdrawn_at {
            transaction.execute_batch(
                "ALTER TABLE task_completion_exemptions ADD COLUMN withdrawn_at INTEGER;",
            )?;
        }
        let has_withdrawn_by: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_completion_exemptions')
                            WHERE name = 'withdrawn_by')",
            [],
            |row| row.get(0),
        )?;
        if !has_withdrawn_by {
            transaction.execute_batch(
                "ALTER TABLE task_completion_exemptions ADD COLUMN withdrawn_by TEXT;",
            )?;
        }
    }
    // THE SEND GUARD IS A TRIGGER, AND A TRIGGER CANNOT SEE A COLUMN THAT DOES
    // NOT EXIST YET. `email_reply_send_requires_evidence` was written at schema
    // 108 and lets a reply go out on an approved exemption; a withdrawn one must
    // stop counting there too, or a reply ships to a person on evidence its own
    // author has retracted.
    //
    // It is recreated HERE rather than edited in place at 108. Editing the older
    // migration makes it reference `withdrawn_at` while replaying a database that
    // has not reached 123 — every fresh build runs 108 first, and the trigger
    // then fails at the next write with "no such column". That is not
    // hypothetical: it is what this migration did on its first run.
    //
    // Dropped by name before creating, for the reason spelled out at 108: the
    // migration tests tear a modern store back and migrate forward again, and a
    // CREATE with no DROP fails there with "already exists".
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS email_reply_send_requires_evidence;
         CREATE TRIGGER email_reply_send_requires_evidence
             BEFORE UPDATE OF state ON email_reply_deliveries
             WHEN NEW.state = 'queued' AND OLD.state <> 'queued'
              AND NOT EXISTS (
                 SELECT 1 FROM tasks task
                 WHERE task.id = NEW.task_id
                   AND task.state IN ('completed', 'review')
                   AND (
                       EXISTS (SELECT 1 FROM task_deployments deployment
                                WHERE deployment.task_id = task.id)
                       OR EXISTS (SELECT 1 FROM task_completion_exemptions exemption
                                   WHERE exemption.task_id = task.id
                                     AND exemption.approved_at IS NOT NULL
                                     AND exemption.withdrawn_at IS NULL)
                   )
             )
             BEGIN SELECT RAISE(ABORT, 'An email reply cannot be sent without a recorded deployment or an approved no-deployment exemption'); END;",
    )?;
    transaction.pragma_update(None, "user_version", CLAIM_WITHDRAWAL_SCHEMA_VERSION)
}

/// The columns of `worker_profiles` in creation order, for a rebuild.
///
/// `SQLite` cannot ALTER a CHECK constraint, so changing one means the whole
/// twelve-step rebuild. Naming the columns explicitly rather than `SELECT *`
/// keeps the copy honest if this is ever run against a database whose column
/// order differs.
const WORKER_PROFILE_COLUMNS: &str = "id, name, role, provider, workspace, autostart, position, \
     created_at, updated_at, hive_id, provider_conversation_id, description, archived_at, \
     system_role, provider_conversation_resume";

/// Rebuilds `worker_profiles` without the closed provider list.
///
/// The column carried `CHECK (provider IN ('claude_code','codex'))`, which made
/// adding a provider a SCHEMA change rather than a code change. The operator
/// chose to drop it outright rather than widen it: the domain already refuses an
/// unknown provider at every write boundary through `FromStr`, so the constraint
/// was a second opinion about a question already answered, and it was the only
/// reason each new provider needed a migration.
///
/// That is a real reduction in defence in depth and it was taken deliberately.
/// The database will now accept any string in this column; `ProviderKind` is the
/// only thing standing between an operator and a nonsense provider.
///
/// `legacy_alter_table` is the load-bearing part. SEVENTEEN tables carry foreign
/// keys into `worker_profiles`. With the pragma OFF, renaming the table rewrites
/// every one of those references to follow the new name, which would leave them
/// all pointing at the temporary table this drops at the end. With it ON the
/// references keep naming `worker_profiles`, so they resolve to the rebuilt table
/// and nothing downstream notices.
///
/// Guarded on the constraint's presence rather than on a column, because the
/// migration tests rewind `user_version` WITHOUT rewinding tables: this has to be
/// safe to run against a database that already has the rebuilt shape.
fn migrate_open_provider_set(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let constrained: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'worker_profiles'
                          AND sql LIKE '%provider IN (%')",
        [],
        |row| row.get(0),
    )?;
    if constrained {
        transaction.pragma_update(None, "legacy_alter_table", true)?;
        let columns = WORKER_PROFILE_COLUMNS;
        let rebuild = format!(
            "ALTER TABLE worker_profiles RENAME TO worker_profiles_pre_open_provider;
             CREATE TABLE worker_profiles (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                 role TEXT NOT NULL CHECK (role IN ('queen','worker')),
                 provider TEXT NOT NULL,
                 workspace TEXT NOT NULL,
                 autostart INTEGER NOT NULL CHECK (autostart IN (0,1)),
                 position INTEGER NOT NULL,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 hive_id TEXT REFERENCES hives(id),
                 provider_conversation_id TEXT,
                 description TEXT NOT NULL DEFAULT '',
                 archived_at INTEGER,
                 system_role TEXT CHECK (system_role IS NULL OR system_role = 'scout'),
                 provider_conversation_resume INTEGER NOT NULL DEFAULT 0
                     CHECK (provider_conversation_resume IN (0, 1))
             );
             INSERT INTO worker_profiles ({columns})
                 SELECT {columns} FROM worker_profiles_pre_open_provider;
             DROP TABLE worker_profiles_pre_open_provider;
             CREATE UNIQUE INDEX one_queen_profile
                 ON worker_profiles(role) WHERE role = 'queen';
             CREATE INDEX worker_profiles_by_hive ON worker_profiles(hive_id);
             CREATE INDEX worker_profiles_active_roster
                 ON worker_profiles(role, position, created_at, id)
                 WHERE archived_at IS NULL;
             CREATE UNIQUE INDEX one_scout_per_hive
                 ON worker_profiles(hive_id)
                 WHERE system_role = 'scout' AND archived_at IS NULL;"
        );
        let result = transaction.execute_batch(&rebuild);
        transaction.pragma_update(None, "legacy_alter_table", false)?;
        result?;
    }
    transaction.pragma_update(None, "user_version", OPEN_PROVIDER_SET_SCHEMA_VERSION)
}

/// Marks a worker as temporary until it is adopted or released.
///
/// A temporary worker is a real row rather than an anonymous session because it
/// holds the full tool surface: it can transition tasks, file new ones and
/// record deployments. Anything that writes to the durable record has to stay
/// attributable, or its writes outlive it pointing at an author that never
/// existed.
///
/// A plain ADD COLUMN with a default, so no table rebuild and none of migration
/// 96's difficulty. Guarded on the column's absence because the migration tests
/// rewind `user_version` without rewinding tables.
fn migrate_ephemeral_workers(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // Table existence AND column absence. Older synthetic schemas in the
    // migration tests have no worker_profiles at all, and pragma_table_info on a
    // missing table returns no rows rather than failing -- so checking only the
    // column reads "not present" and then ALTERs a table that is not there.
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    let has_column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_profiles')
                        WHERE name = 'ephemeral')",
        [],
        |row| row.get(0),
    )?;
    if present && !has_column {
        transaction.execute_batch(
            "ALTER TABLE worker_profiles ADD COLUMN ephemeral INTEGER NOT NULL DEFAULT 0
                 CHECK (ephemeral IN (0, 1));",
        )?;
    }
    transaction.pragma_update(None, "user_version", EPHEMERAL_WORKER_SCHEMA_VERSION)
}

/// Corrections of FACT appended to a task's description, never replacing it.
///
/// The operator's ruling, decision 01a04108: "Facts govern, scope and acceptance
/// never do", and "Worker and Queen, always attributed".
///
/// The defect this closes is an asymmetry rather than an absence. A task could
/// already be corrected — in a NOTE, which is subordinate to the description it
/// corrects. So the error sat in the authoritative place and its correction sat
/// three screens below it, and a correction system whose corrections carry less
/// standing than the thing they correct reliably loses.
///
/// APPEND ONLY, enforced by there being no update or delete path. Not even the
/// author can revise an amendment; they append another. That is what preserves
/// the property immutability was protecting — you can still reconstruct exactly
/// what a worker was told when it picked the task up.
///
/// `author_worker_id` is NOT NULL on purpose. An unattributed amendment to the
/// governing text would be strictly worse than the stale text it replaces.
fn migrate_task_amendments(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    if present {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_amendments (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 author_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 body TEXT NOT NULL,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS task_amendments_by_task
                 ON task_amendments(task_id, created_at);",
        )?;
    }
    transaction.pragma_update(None, "user_version", TASK_AMENDMENT_SCHEMA_VERSION)
}

/// One command an operator approved, and the grant that makes it runnable.
///
/// The operator ruled on this: build it, and "one decision, one use, dies with
/// the task".
///
/// WHY THIS DOES NOT WIDEN WHO CAN AUTHORISE ANYTHING. Only the operator can
/// resolve a decision, so only the operator can create a grant. The chain is the
/// same act they already perform; what changes is that the classifier can see
/// it. A worker cannot mint one, and neither can Queen.
///
/// `requested_command` lives on the DECISION rather than here so the operator
/// reads the exact text before approving. Approving "the one contact
/// formula-column test" is not approving a regex, and a decision that silently
/// compiled to a permission pattern would trade a visible block for an invisible
/// grant — worse than the gap being closed.
///
/// `consumed_at` is honest about what it can enforce. The classifier reads a
/// settings file at process start and reports nothing back, so exactly-once
/// cannot be enforced AT THE CLASSIFIER. What is enforced is that the grant
/// appears in one session's settings, is marked consumed when that session ends,
/// and dies with the task regardless.
fn migrate_decision_command_grants(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'decision_requests')",
        [],
        |row| row.get(0),
    )?;
    if present {
        let has_column: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('decision_requests')
                            WHERE name = 'requested_command')",
            [],
            |row| row.get(0),
        )?;
        if !has_column {
            transaction.execute_batch(
                "ALTER TABLE decision_requests ADD COLUMN requested_command TEXT;",
            )?;
        }
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS decision_command_grants (
                 decision_id TEXT PRIMARY KEY
                     REFERENCES decision_requests(id) ON DELETE CASCADE,
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                 command TEXT NOT NULL,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 consumed_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS decision_command_grants_live
                 ON decision_command_grants(worker_id) WHERE consumed_at IS NULL;",
        )?;
    }
    transaction.pragma_update(None, "user_version", DECISION_COMMAND_GRANT_SCHEMA_VERSION)
}

fn acknowledge_task_dispatch(
    transaction: &rusqlite::Transaction<'_>,
    id: TaskId,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE task_dispatches
         SET state = 'delivered', delivered_at = COALESCE(delivered_at, unixepoch()),
             updated_at = unixepoch()
         WHERE assignment_id IN (
             SELECT assignment.id FROM task_assignments assignment
             WHERE assignment.task_id = ?1 AND assignment.released_at IS NULL
         ) AND state IN ('queued', 'dispatching', 'uncertain')",
        [id.to_string()],
    )?;
    Ok(())
}

fn migrate_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < 2 {
        migrate_worker_roster(transaction)?;
    }
    if schema_version < 3 {
        migrate_task_details(transaction)?;
    }
    if schema_version < 4 {
        migrate_hive_identity(transaction)?;
    }
    if schema_version < 5 {
        migrate_control_room_events(transaction)?;
    }
    if schema_version < 6 {
        migrate_task_ordering(transaction)?;
    }
    if schema_version < 7 {
        migrate_provider_conversations(transaction)?;
    }
    if schema_version < 8 {
        migrate_worker_engagements(transaction)?;
    }
    if schema_version < 9 {
        migrate_agent_credentials(transaction)?;
    }
    if schema_version < 10 {
        migrate_decision_requests(transaction)?;
    }
    if schema_version < 11 {
        migrate_decision_deliveries(transaction)?;
    }
    if schema_version < 12 {
        migrate_task_dispatches(transaction)?;
    }
    if schema_version < 13 {
        migrate_task_outcomes(transaction)?;
    }
    if schema_version < 14 {
        migrate_operator_presence(transaction)?;
    }
    if schema_version < 15 {
        migrate_notifications(transaction)?;
    }
    if schema_version < 16 {
        migrate_engagement_ownership(transaction)?;
    }
    if schema_version < 17 {
        migrate_queen_autonomy(transaction)?;
    }
    if schema_version < 18 {
        migrate_presentation_preferences(transaction)?;
    }
    if schema_version < 19 {
        migrate_durable_task_ownership(transaction)?;
    }
    if schema_version < 20 {
        migrate_dogfood_reports(transaction)?;
    }
    if schema_version < 21 {
        migrate_jira_bindings(transaction)?;
    }
    if schema_version < 22 {
        migrate_jira_transition_deliveries(transaction)?;
    }
    if schema_version < 23 {
        migrate_jira_comment_deliveries(transaction)?;
    }
    if schema_version < 24 {
        migrate_jira_assigned_sync_preference(transaction)?;
    }
    if schema_version < 25 {
        migrate_apiary_stewardships(transaction)?;
    }
    if schema_version < 26 {
        apiary::migrate_apiary_invitations(transaction)?;
    }
    if schema_version < 27 {
        apiary::migrate_apiary_jira_projects(transaction)?;
    }
    if schema_version < 28 {
        apiary::migrate_apiary_policy_acceptance(transaction)?;
    }
    if schema_version < 29 {
        apiary::migrate_apiary_lifecycle(transaction)?;
    }
    migrate_recent_schema(transaction, schema_version)
}

fn migrate_recent_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    migrate_federation_schema(transaction, schema_version)?;
    if schema_version < 40 {
        email::migrate_email_intake(transaction)?;
    } else if schema_version < 41 {
        email::migrate_email_multi_source(transaction)?;
    }
    if schema_version < 42 {
        apiary::migrate_apiary_identity_events(transaction)?;
    }
    if schema_version < 43 {
        email::migrate_email_reply_targets(transaction)?;
    }
    if schema_version < 44 {
        migrate_task_activity_actors(transaction)?;
    }
    if schema_version < 45 {
        federation::migrate_apiary_join_links(transaction)?;
    }
    if schema_version < 46 {
        federation::migrate_local_apiary_keeper_links(transaction)?;
    }
    if schema_version < 47 {
        federation_tasks::migrate_federation_tasks(transaction)?;
    }
    if schema_version < 48 {
        federation_tasks::migrate_federation_task_commands(transaction)?;
    }
    if schema_version < 49 {
        migrate_worker_profile_metadata(transaction)?;
    }
    if schema_version < 50 {
        federation_jira_claims::migrate_federation_jira_claims(transaction)?;
    }
    if schema_version < 51 {
        migrate_managed_worker_roles(transaction)?;
    }
    if schema_version < 52 {
        federation_stewardships::migrate_federation_stewardship_projection(transaction)?;
    }
    if schema_version < 53 {
        federation::migrate_federation_departures(transaction)?;
    }
    if schema_version < 54 {
        federation_handoffs::migrate_federation_handoffs(transaction)?;
    }
    if schema_version < 55 {
        federation_handoff_reconciliation::migrate_federation_handoff_reconciliation(transaction)?;
    }
    if schema_version < 56 {
        federation_tasks::migrate_local_apiary_task_executions(transaction)?;
    }
    if schema_version < 57 {
        federation_tasks::migrate_local_apiary_task_lifecycle_intents(transaction)?;
    }
    if schema_version < 58 {
        federation_steward_tasks::migrate_federation_steward_task_commands(transaction)?;
    }
    if schema_version < 59 {
        federation_steward_assists::migrate_federation_steward_assists(transaction)?;
    }
    if schema_version < 60 {
        federation_steward_takeovers::migrate_federation_steward_takeovers(transaction)?;
    }
    if schema_version < 61 {
        queen_conductor::migrate_queen_conductor(transaction)?;
    }
    if schema_version < 62 {
        coordinator::migrate_coordinator(transaction)?;
    }
    if schema_version < 63 {
        coordinator::migrate_coordinator_attention(transaction)?;
    }
    if schema_version < 64 {
        coordinator::migrate_coordinator_worker_exit_attention(transaction)?;
    }
    if schema_version < 65 {
        coordinator::migrate_coordinator_unstarted_work_attention(transaction)?;
    }
    migrate_named_schema_steps(transaction, schema_version)?;
    Ok(())
}

/// The steps identified by a named ceiling rather than a bare number.
///
/// Split from the numbered chain so each stays readable, following the same
/// grouping the federation steps already use. Order still matters: every step
/// runs against the schema the ones above it left behind.
fn migrate_named_schema_steps(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < TERMINAL_GEOMETRY_SCHEMA_VERSION {
        migrate_terminal_geometry_ownership(transaction)?;
    }
    if schema_version < LEGACY_MIGRATION_SCHEMA_VERSION {
        migration::migrate_legacy_migration_batches(transaction)?;
    }
    if schema_version < LEGACY_WORKER_MIGRATION_SCHEMA_VERSION {
        migration::migrate_legacy_worker_migrations(transaction)?;
    }
    if schema_version < TASK_REMOVAL_SCHEMA_VERSION {
        migrate_task_removal(transaction)?;
    }
    if schema_version < DEPLOYMENT_GRANT_SCHEMA_VERSION {
        deployment_grants::migrate_deployment_grants(transaction)?;
    }
    if schema_version < LEGACY_PROVIDER_CONVERSATION_SCHEMA_VERSION {
        migration::migrate_legacy_provider_conversations(transaction)?;
    }
    if schema_version < LEGACY_EXISTING_CONVERSATION_SCHEMA_VERSION {
        migration::migrate_legacy_existing_conversations(transaction)?;
    }
    if schema_version < QUEEN_DELIVERY_SESSION_SCHEMA_VERSION {
        queen_conductor::migrate_queen_delivery_session(transaction)?;
    }
    if schema_version < PRESENCE_LAST_ACTIVE_SCHEMA_VERSION {
        presence::migrate_presence_last_active(transaction)?;
    }
    if schema_version < TASK_OPERATOR_INSTRUCTION_SCHEMA_VERSION {
        migrate_task_operator_instruction(transaction)?;
    }
    if schema_version < WORKER_REVIVAL_INTENT_SCHEMA_VERSION {
        workers::migrate_worker_revival_intents(transaction)?;
    }
    if schema_version < DECISION_RESOLUTION_SURFACE_SCHEMA_VERSION {
        decisions::migrate_decision_resolution_surface(transaction)?;
    }
    if schema_version < DECISION_QUESTIONS_SCHEMA_VERSION {
        decisions::migrate_decision_questions(transaction)?;
    }
    if schema_version < DECISION_SUMMARY_SCHEMA_VERSION {
        decisions::migrate_decision_summary(transaction)?;
    }
    if schema_version < EMAIL_REPLY_FROM_REVIEW_SCHEMA_VERSION {
        email::migrate_email_reply_from_review(transaction)?;
    }
    if schema_version < WORKER_FILED_DRAFT_SCHEMA_VERSION {
        coordinator::migrate_worker_filed_draft_attention(transaction)?;
    }
    if schema_version < START_SURFACE_SCHEMA_VERSION {
        migrate_start_surface(transaction)?;
    }
    if schema_version < RELEASE_CHECK_SCHEMA_VERSION {
        migrate_release_check(transaction)?;
    }
    if schema_version < COMPLETION_EXEMPTION_SCHEMA_VERSION {
        migrate_completion_exemption(transaction)?;
    }
    if schema_version < DECISION_DEADLINE_ATTENTION_SCHEMA_VERSION {
        coordinator::migrate_decision_deadline_attention(transaction)?;
        transaction.pragma_update(
            None,
            "user_version",
            DECISION_DEADLINE_ATTENTION_SCHEMA_VERSION,
        )?;
    }
    if schema_version < COORDINATOR_REFUSAL_SCHEMA_VERSION {
        migrate_coordinator_refusals(transaction)?;
    }
    if schema_version < OPERATOR_PASSKEY_SCHEMA_VERSION {
        migrate_operator_passkeys(transaction)?;
    }
    if schema_version < TERMINAL_GEOMETRY_LEDGER_SCHEMA_VERSION {
        migrate_terminal_geometry_ledger(transaction)?;
    }
    if schema_version < UNDELIVERED_BRIEF_ATTENTION_SCHEMA_VERSION {
        coordinator::migrate_undelivered_brief_attention(transaction)?;
    }
    if schema_version < REVIEWED_WORK_EVIDENCE_ATTENTION_SCHEMA_VERSION {
        coordinator::migrate_reviewed_work_evidence_attention(transaction)?;
    }
    migrate_newest_schema_steps(transaction, schema_version)
}

/// The steps from the review hold onwards.
///
/// Split from `migrate_named_schema_steps` purely because that function reached
/// the length limit, and split HERE rather than at an arbitrary line: 91 is
/// where this Hive started recording why work was held rather than only that it
/// was, so everything below is the run of steps about evidence and provenance.
///
/// The order is still the order. Adding a step means appending to the end of
/// this list and to `RECENT_SCHEMA_STEPS`, whichever function it lands in.
fn migrate_newest_schema_steps(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < REVIEW_HOLD_SCHEMA_VERSION {
        task_outcomes::migrate_review_holds(transaction)?;
    }
    if schema_version < SESSION_END_REASON_SCHEMA_VERSION {
        workers::migrate_session_end_reason(transaction)?;
    }
    if schema_version < REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION {
        email::migrate_reply_allows_approved_exemption(transaction)?;
    }
    if schema_version < ATTENTION_NOTIFICATION_SCHEMA_VERSION {
        notifications::migrate_attention_notifications(transaction)?;
    }
    if schema_version < SUPERSEDED_EXEMPTION_SCHEMA_VERSION {
        migrate_superseded_exemptions(transaction)?;
    }
    if schema_version < OPEN_PROVIDER_SET_SCHEMA_VERSION {
        migrate_open_provider_set(transaction)?;
    }
    if schema_version < EPHEMERAL_WORKER_SCHEMA_VERSION {
        migrate_ephemeral_workers(transaction)?;
    }
    if schema_version < TASK_AMENDMENT_SCHEMA_VERSION {
        migrate_task_amendments(transaction)?;
    }
    if schema_version < DECISION_COMMAND_GRANT_SCHEMA_VERSION {
        migrate_decision_command_grants(transaction)?;
    }
    if schema_version < UNATTENDED_BLOCK_SCHEMA_VERSION {
        coordinator::migrate_unattended_block_attention(transaction)?;
    }
    if schema_version < AMENDMENT_ACTIVITY_SCHEMA_VERSION {
        migrate_amendment_activity(transaction)?;
    }
    if schema_version < BLOCK_DEADLINE_SCHEMA_VERSION {
        migrate_block_deadline(transaction)?;
    }
    if schema_version < WORKER_MARK_SCHEMA_VERSION {
        migrate_worker_mark(transaction)?;
    }
    if schema_version < CONNECTION_PRINCIPAL_SCHEMA_VERSION {
        migrate_connection_principal(transaction)?;
    }
    // LAST, because every step above ends by stamping its own user_version and
    // the final one wins. Dispatched from migrate_schema instead, this ran
    // before the 40-104 chain and its stamp was overwritten by a lower number.
    if schema_version < UNVERIFIABLE_CLOSURE_SCHEMA_VERSION {
        migrate_unverifiable_closures(transaction)?;
    }
    if schema_version < FEEDBACK_ISSUE_SCHEMA_VERSION {
        migrate_feedback_issue(transaction)?;
    }
    if schema_version < GITHUB_ISSUE_INTAKE_SCHEMA_VERSION {
        migrate_github_issue_intake(transaction)?;
    }
    if schema_version < REPLY_EVIDENCE_GUARDS_THE_SEND_SCHEMA_VERSION {
        email::migrate_reply_evidence_guards_the_send(transaction)?;
    }
    if schema_version < GITHUB_USER_CONNECTION_SCHEMA_VERSION {
        feedback::migrate_github_user_connection(transaction)?;
    }
    // LAST ON PURPOSE. Every step stamps user_version and the last one
    // wins, so dispatching this above another step would leave the
    // database claiming a version it has not reached.
    if schema_version < ABANDONED_STATE_SCHEMA_VERSION {
        migrate_abandoned_state(transaction)?;
    }
    if schema_version < TASK_COMMIT_REPORT_SCHEMA_VERSION {
        migrate_task_commit_reports(transaction)?;
    }
    if schema_version < COORDINATOR_SETTLEMENT_SCHEMA_VERSION {
        migrate_coordinator_approves_settlements(transaction)?;
    }
    // LAST, because it stamps the ceiling. A migration that sets user_version
    // runs its pragma unconditionally, so one placed mid-chain has its stamp
    // overwritten by every lower-numbered migration that follows it.
    if schema_version < EVIDENCED_WORK_NOT_CLOSED_SCHEMA_VERSION {
        coordinator::migrate_evidenced_work_not_closed_attention(transaction)?;
    }
    if schema_version < AWAITING_RELEASE_SCHEMA_VERSION {
        migrate_awaiting_release_state(transaction)?;
    }
    if schema_version < RETURNED_REVIEW_SCHEMA_VERSION {
        migrate_returned_reviews(transaction)?;
    }
    if schema_version < TASK_MESSAGE_SCHEMA_VERSION {
        migrate_task_messages(transaction)?;
    }
    if schema_version < APPROVAL_BASIS_SCHEMA_VERSION {
        migrate_approval_basis(transaction)?;
    }
    if schema_version < PARTIAL_DEPLOYMENT_SCHEMA_VERSION {
        migrate_partial_deployments(transaction)?;
    }
    if schema_version < OPERATOR_BROADCAST_SCHEMA_VERSION {
        migrate_operator_broadcasts(transaction)?;
    }
    if schema_version < BROADCAST_EXPIRY_SCHEMA_VERSION {
        migrate_broadcast_expiry(transaction)?;
    }
    // LAST, and it has to stay last. Every migration sets user_version to its
    // OWN number as its final act, so one running after this one winds the
    // recorded version backwards. Adding this above migrate_broadcast_expiry
    // left a fully migrated database reporting 120, and twelve ceiling tests
    // said so at once. Fifth time that family of tests has caught a step here.
    if schema_version < MESSAGE_DELIVERY_SESSION_SCHEMA_VERSION {
        migrate_message_delivery_session(transaction)?;
    }
    if schema_version < DELIVERY_COOLDOWN_SCHEMA_VERSION {
        migrate_delivery_cooldown(transaction)?;
    }
    migrate_ops_intake_schema_steps(transaction, schema_version)
}

fn migrate_ops_intake_schema_steps(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    // Include the immediately preceding step to preserve strict version order.
    // LAST, and it has to stay last. Every migration sets user_version to its
    // own number as its final act, so one running after this one winds the
    // recorded version backwards.
    if schema_version < CLAIM_WITHDRAWAL_SCHEMA_VERSION {
        migrate_claim_withdrawal(transaction)?;
    }
    migrate_maturity_schema_steps(transaction, schema_version)?;
    if schema_version < OPS_TICKETS_SCHEMA_VERSION {
        ops_tickets::migrate_ops_tickets(transaction)?;
    }
    // LAST. This repairs databases which received upstream's former schema 124
    // before the maturity branch was combined and therefore skipped the other
    // schema-124 artifact while still advancing through schema 135.
    terminal_control_projection::repair_version_collision(transaction, schema_version)
}

/// Checks that a recovery candidate already contains a supported Hive schema.
/// This must precede normal opening, which may create or migrate a database.
///
/// # Errors
/// Rejects missing, empty, unrelated, future-schema, or corrupt candidates
/// without creating or changing the selected file.
pub fn verify_existing_hive_backup(path: &Path) -> Result<(), TaskStoreError> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let has_tasks: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    if version <= 0 || version > CURRENT_SCHEMA_VERSION || !has_tasks {
        return Err(TaskStoreError::IntegrityFailure(
            "recovery candidate is not an existing supported Hive database".into(),
        ));
    }
    drop(connection);
    verify_backup_at(path, version)
}

fn migrate_maturity_schema_steps(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    terminal_control_projection::migrate(transaction, schema_version)?;
    night_watch::migrate(transaction, schema_version)?;
    dogfood_evidence::migrate(transaction, schema_version)?;
    conversation_recovery::migrate(transaction, schema_version)?;
    conversation_recovery::migrate_selection(transaction, schema_version)?;
    task_dispatches::migrate_generation(transaction, schema_version)?;
    operator_statements::migrate(transaction, schema_version)?;
    operator_statements::migrate_resolutions(transaction, schema_version)?;
    operator_submissions::migrate(transaction, schema_version)?;
    review_answers::migrate(transaction, schema_version)?;
    message_delivery::migrate(transaction, schema_version)
}

/// Work closed for a reason other than success gets its own state.
///
/// A REBUILD, because `SQLite` cannot alter a CHECK constraint and `tasks`
/// carries one enumerating every state it accepts. This is the same shape as
/// the `worker_profiles` rebuild that failed once on the operator's real
/// database, and bigger: 43 tables hold a foreign key into `tasks(id)`.
///
/// `legacy_alter_table` is ON for the rename ON PURPOSE. Modern `SQLite`
/// rewrites
/// other tables' REFERENCES clauses to follow a renamed table, which is exactly
/// wrong here -- all 43 would end up pointing at `tasks_v109`, and then at
/// nothing once it is dropped. Legacy mode leaves them naming `tasks`, so they
/// land on the table created below.
///
/// The partial index is NOT copied verbatim. `task_owner_queue` was written
/// `WHERE state != 'completed'` back when completed was the only terminal
/// state; carried over unchanged it would have quietly enrolled abandoned work
/// in every owner queue that index serves.
fn migrate_abandoned_state(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // TWO QUESTIONS, ASKED SEPARATELY. "Not yet migrated" and "not there at
    // all" are different answers that a single EXISTS-with-LIKE collapses into
    // one, and the collapse rebuilds a table that does not exist: the schema-v23
    // test builds a database holding one unrelated table and migrates it
    // forward, which is a shape every step from here to 110 has to survive.
    let table_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    let already: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'tasks' AND sql LIKE '%abandoned%')",
        [],
        |row| row.get(0),
    )?;
    // AND ITS FOREIGN KEY TARGETS, for the same reason the apiary rebuild
    // checks its own: re-issuing this CREATE names `hives` and
    // `worker_profiles`, and the rename that precedes it reparses the schema.
    let targets_exist: bool = transaction.query_row(
        "SELECT (SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('hives','worker_profiles')) = 2",
        [],
        |row| row.get(0),
    )?;
    if table_exists && targets_exist && !already {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS tasks_by_hive;
             DROP INDEX IF EXISTS tasks_by_hive_position;
             DROP INDEX IF EXISTS task_owner_queue;
             DROP INDEX IF EXISTS tasks_visible_queue;
             DROP TRIGGER IF EXISTS tasks_require_hive_insert;
             DROP TRIGGER IF EXISTS tasks_require_hive_update;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE tasks RENAME TO tasks_v109;
             CREATE TABLE tasks (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 workspace TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed','abandoned')),
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 description TEXT NOT NULL DEFAULT '',
                 priority TEXT NOT NULL DEFAULT 'normal'
                     CHECK (priority IN ('low','normal','high','urgent')),
                 hive_id TEXT REFERENCES hives(id),
                 position INTEGER NOT NULL DEFAULT 0,
                 assigned_worker_id TEXT REFERENCES worker_profiles(id),
                 removed_at INTEGER,
                 operator_instruction TEXT NOT NULL DEFAULT '',
                 blocked_until INTEGER
             );
             INSERT INTO tasks
                 (id, title, workspace, state, created_at, updated_at, description,
                  priority, hive_id, position, assigned_worker_id, removed_at,
                  operator_instruction, blocked_until)
             SELECT id, title, workspace, state, created_at, updated_at, description,
                    priority, hive_id, position, assigned_worker_id, removed_at,
                    operator_instruction, blocked_until
               FROM tasks_v109;
             DROP TABLE tasks_v109;
             CREATE INDEX tasks_by_hive ON tasks(hive_id);
             CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
             CREATE INDEX task_owner_queue
                 ON tasks(assigned_worker_id, state)
                 WHERE assigned_worker_id IS NOT NULL
                   AND state NOT IN ('completed','abandoned');
             CREATE INDEX tasks_visible_queue
                 ON tasks(hive_id, state) WHERE removed_at IS NULL;
             CREATE TRIGGER tasks_require_hive_insert
                 BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
                 BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
             CREATE TRIGGER tasks_require_hive_update
                 BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
                 BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    federation_tasks::migrate_abandoned_apiary_task_state(transaction)?;
    transaction.pragma_update(None, "user_version", ABANDONED_STATE_SCHEMA_VERSION)
}

/// An approval records what it rests on, not merely that it happened.
///
/// The invariant's promise is that somebody OTHER THAN THE AUTHOR checked. On
/// 2026-09-01 at 04:25 an exemption was approved whose work had an open pull
/// request — the rule was satisfied while nobody checked anything. Under load a
/// second pair of eyes degrades into a rubber stamp, and load is exactly when
/// it is needed.
///
/// A cited basis turns a click into a claim somebody can later be wrong about,
/// which is the only thing that makes a review real. "I could not verify" is a
/// legitimate basis; leaving it unsaid is not.
fn migrate_approval_basis(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_completion_exemptions')
         WHERE name = 'approved_basis')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        transaction.execute_batch(
            "ALTER TABLE task_completion_exemptions ADD COLUMN approved_basis TEXT;",
        )?;
    }
    transaction.pragma_update(None, "user_version", crate::APPROVAL_BASIS_SCHEMA_VERSION)
}

/// A deployment can say "part of this shipped" instead of "this shipped".
///
/// The deterministic sweep closes a task in Review the moment a deployment is
/// recorded against it, because a deployment normally IS the completion
/// evidence. Partial delivery is common and the record could not express it: on
/// 2026-09-02 at 02:08 a worker moved B7 to Review with a handoff opening "TWO
/// OF THE THREE ACCEPTANCE LINES ARE NOT MET AND I AM NOT CLAIMING THEM",
/// recorded a true deployment for the half that had shipped, and the sweep
/// closed the whole ticket one second later. Completed is terminal.
///
/// This is NOT the reviewer's-hold case, which the operator ruled the sweep
/// must keep winning. A hold is somebody's OPINION that work is unfinished, and
/// obeying it lets a forgotten hold strand work forever. This is the EVIDENCE
/// itself saying it does not cover the whole ticket, recorded by the author of
/// the deployment at the moment they record it. The sweep's premise is "a
/// deployment means this shipped"; against a partial deployment that premise is
/// simply absent, so there is nothing for it to act on.
fn migrate_partial_deployments(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_deployments')
         WHERE name = 'delivers_whole_task')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        // DEFAULT 1, so every deployment already recorded keeps meaning what it
        // meant when it was written. A backfill to 0 would retroactively reopen
        // nothing and unsettle everything.
        transaction.execute_batch(
            "ALTER TABLE task_deployments ADD COLUMN delivers_whole_task INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::PARTIAL_DEPLOYMENT_SCHEMA_VERSION,
    )
}

/// A governed channel between Queen and a worker, durable on the task.
///
/// Queen was already asking workers questions — through Claude Code's own
/// session channel, which Swarm did not build, cannot see, cannot record, and
/// which one of the active workers cannot receive at all. The exchange existed
/// only in two terminal scrollbacks, which contradicts the premise that what is
/// not on the board did not happen.
///
/// NO WORKER-TO-WORKER LEG, enforced by a CHECK rather than by convention. A
/// worker's claim about authority reaching another worker with no board record
/// turns "anything a sender can write, a sender can fabricate" from a
/// discipline into an attack surface. The operator's words: "No worker to
/// worker communication, but queen<->worker communication is fine in both
/// directions."
/// The operator says one thing to every worker at once.
///
/// Asked for on 2026-09-02: "Is there a way I can as operator broadcast
/// something to all workers? For instance I need pause workers to do a worker
/// reload, and I have to do it one by one."
///
/// TWO TABLES, NOT ONE, AND THE SECOND IS THE POINT. A broadcast is one message
/// with many outcomes, and the outcomes differ: measured when this was built,
/// 13 of 45 workers had an open session. The other 32 are not slow, they are
/// unreachable — the dispatch join requires a live session, so a message to a
/// worker without one is excluded outright rather than queued. Recording one
/// row per recipient is what lets the operator be told "13 of 45" instead of
/// being allowed to believe it reached everyone.
fn migrate_operator_broadcasts(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_broadcasts (
             id TEXT PRIMARY KEY,
             body TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS operator_broadcast_deliveries (
             broadcast_id TEXT NOT NULL REFERENCES operator_broadcasts(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
             session_id TEXT NOT NULL,
             delivered_at INTEGER,
             PRIMARY KEY (broadcast_id, worker_id)
         );
         CREATE INDEX IF NOT EXISTS operator_broadcast_deliveries_pending
             ON operator_broadcast_deliveries(broadcast_id) WHERE delivered_at IS NULL;",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::OPERATOR_BROADCAST_SCHEMA_VERSION,
    )
}

/// A broadcast that missed its window is expired, not stranded.
///
/// Deliveries were pinned to the session that existed when the broadcast was
/// written, so a worker restart left them matching nothing: never delivered,
/// never retried, never expired, never reported. Measured on the first real
/// broadcast, 2026-09-02 — 14 queued, 0 delivered, all 14 pointing at sessions
/// a force reload had ended.
///
/// The delivery now follows the WORKER, and this column records the ones that
/// ran out of time on the way. The operator ruled the window: deliver to a
/// worker that comes back within ten minutes, expire it after that, because a
/// broadcast describes now and "pause work so I can reload" arriving after the
/// reload is worse than not arriving.
fn migrate_broadcast_expiry(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // EACH COLUMN GUARDED SEPARATELY, because the undo drops only the artifact
    // the schema step NAMES. Guarding both on the presence of `expired_at`
    // meant that after an undo removed just that one, re-running tried to add
    // `expiry_reason` again and failed with "duplicate column name" — caught by
    // the migration ceiling test, which is the fourth time that test has caught
    // a step of mine.
    for (column, definition) in [
        (
            "expired_at",
            "ALTER TABLE operator_broadcast_deliveries ADD COLUMN expired_at INTEGER",
        ),
        (
            "expiry_reason",
            "ALTER TABLE operator_broadcast_deliveries ADD COLUMN expiry_reason TEXT",
        ),
    ] {
        let present: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('operator_broadcast_deliveries')
             WHERE name = ?1)",
            [column],
            |row| row.get(0),
        )?;
        if !present {
            transaction.execute_batch(definition)?;
        }
    }
    transaction.pragma_update(None, "user_version", crate::BROADCAST_EXPIRY_SCHEMA_VERSION)
}

/// A delivered message records WHERE it went, not only when.
///
/// `delivered_at` alone made a message written into a session that then exited
/// indistinguishable from one the running worker read and acted on. The sender
/// saw "delivered" for both, so it stopped chasing the one nobody living had
/// been told about. Queen caught the case that prompted this only by reading a
/// delivery timestamp against the session id from `swarm_list_workers` BY HAND.
///
/// THE BROADCAST PATH ALREADY DID THIS. `operator_broadcast_deliveries` has
/// carried `session_id` since schema 119, and `task_messages` did not — the same
/// feature area at two levels of record-keeping, which nobody decided.
///
/// Existing rows keep NULL, and NULL means "delivered before this column
/// existed" rather than "delivered nowhere". Backfilling a session id onto
/// history would be inventing one: nothing in the record says which session
/// took a message that predates the column, and a guessed answer here is worse
/// than an absent one because it reads exactly like a measured answer.
/// A terminal remembers when coordination last wrote to it.
///
/// DURABLE ON PURPOSE, and the reason is the principle this Hive spent the day
/// on: a commitment with nowhere durable to live is not a commitment. Holding
/// this in memory would work until the next API restart, and then every session
/// in the Hive would be delivered to at once — the exact flood the cooldown
/// exists to prevent, triggered by the fix for it.
///
/// NULL means "coordination has not written here since this column existed",
/// which correctly reads as "no cooldown in effect" rather than as a delivery
/// at the epoch.
fn migrate_delivery_cooldown(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // GUARDED ON THE TABLE EXISTING AS WELL AS THE COLUMN, which is the house
    // rule stated on migrate_superseded_exemptions and which I did not read
    // before writing this: the migration tests rewind `user_version` WITHOUT
    // rewinding tables, so a step can run against a database that never had the
    // table it wants to alter. worker_sessions is created at schema 2, and the
    // v23 fixture starts above that with only its own table — so this failed
    // with "no such table: worker_sessions" and the ceiling tests said so.
    let table: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'worker_sessions')",
        [],
        |row| row.get(0),
    )?;
    let has_column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_sessions')
                        WHERE name = 'last_coordination_delivery_at')",
        [],
        |row| row.get(0),
    )?;
    if table && !has_column {
        transaction.execute_batch(
            "ALTER TABLE worker_sessions ADD COLUMN last_coordination_delivery_at INTEGER",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::DELIVERY_COOLDOWN_SCHEMA_VERSION,
    )
}

fn migrate_message_delivery_session(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    // Guarded on its own presence, for the reason spelled out in
    // migrate_broadcast_expiry: the undo drops only the column the schema step
    // names, so a guard keyed to anything else fails the re-migration with a
    // duplicate column.
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_messages')
         WHERE name = 'delivered_session_id')",
        [],
        |row| row.get(0),
    )?;
    if !present {
        transaction
            .execute_batch("ALTER TABLE task_messages ADD COLUMN delivered_session_id TEXT")?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        crate::MESSAGE_DELIVERY_SESSION_SCHEMA_VERSION,
    )
}

fn migrate_task_messages(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_messages (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             sender TEXT NOT NULL CHECK (sender IN ('queen','worker','operator')),
             recipient TEXT NOT NULL CHECK (recipient IN ('queen','worker')),
             sender_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
             recipient_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
             body TEXT NOT NULL,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             delivered_at INTEGER,
             CHECK (NOT (sender = 'worker' AND recipient = 'worker'))
         );
         CREATE INDEX IF NOT EXISTS task_messages_by_task
             ON task_messages(task_id, created_at);
         CREATE INDEX IF NOT EXISTS task_messages_undelivered
             ON task_messages(recipient_worker_id) WHERE delivered_at IS NULL;",
    )?;
    transaction.pragma_update(None, "user_version", crate::TASK_MESSAGE_SCHEMA_VERSION)
}

/// Queen hands reviewed work back without moving it backwards.
///
/// A NEW TABLE RATHER THAN A COLUMN, because the request has a body: what is
/// missing is the whole point, and a boolean would record that something was
/// asked while losing what it was.
///
/// The work stays in Review. Returning it to Ready is what invalidated a valid
/// evidence claim on 2026-09-01 — Ready means UNSTARTED to everything that
/// reads it — and returning it to Active makes finished work look unfinished.
/// Industry practice agrees: a pull request with changes requested stays open
/// and gains a state, and Kanban guidance is explicit that backward column
/// moves disguise rework as new work.
fn migrate_returned_reviews(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_returned_reviews (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             request TEXT NOT NULL,
             returned_at INTEGER NOT NULL DEFAULT (unixepoch()),
             answered_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS task_returned_reviews_open
             ON task_returned_reviews(task_id) WHERE answered_at IS NULL;",
    )?;
    transaction.pragma_update(None, "user_version", crate::RETURNED_REVIEW_SCHEMA_VERSION)
}

/// Work that is finished and waiting only to ship gets its own resting state.
///
/// A REBUILD, because `SQLite` cannot alter a CHECK and `tasks` carries one
/// enumerating every state it accepts. Same shape as the `abandoned` step at
/// 110, including its two-questions-asked-separately guard: "not yet migrated"
/// and "not there at all" are different answers, and collapsing them rebuilds a
/// table that does not exist.
fn migrate_awaiting_release_state(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let table_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    let already: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'tasks' AND sql LIKE '%awaiting_release%')",
        [],
        |row| row.get(0),
    )?;
    let targets_exist: bool = transaction.query_row(
        "SELECT (SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('hives','worker_profiles')) = 2",
        [],
        |row| row.get(0),
    )?;
    if table_exists && targets_exist && !already {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS tasks_by_hive;
             DROP INDEX IF EXISTS tasks_by_hive_position;
             DROP INDEX IF EXISTS task_owner_queue;
             DROP INDEX IF EXISTS tasks_visible_queue;
             DROP TRIGGER IF EXISTS tasks_require_hive_insert;
             DROP TRIGGER IF EXISTS tasks_require_hive_update;
             PRAGMA legacy_alter_table = ON;
             ALTER TABLE tasks RENAME TO tasks_v113;
             CREATE TABLE tasks (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 workspace TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','awaiting_release','completed','abandoned')),
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 description TEXT NOT NULL DEFAULT '',
                 priority TEXT NOT NULL DEFAULT 'normal'
                     CHECK (priority IN ('low','normal','high','urgent')),
                 hive_id TEXT REFERENCES hives(id),
                 position INTEGER NOT NULL DEFAULT 0,
                 assigned_worker_id TEXT REFERENCES worker_profiles(id),
                 removed_at INTEGER,
                 operator_instruction TEXT NOT NULL DEFAULT '',
                 blocked_until INTEGER
             );
             INSERT INTO tasks
                 (id, title, workspace, state, created_at, updated_at, description,
                  priority, hive_id, position, assigned_worker_id, removed_at,
                  operator_instruction, blocked_until)
             SELECT id, title, workspace, state, created_at, updated_at, description,
                    priority, hive_id, position, assigned_worker_id, removed_at,
                    operator_instruction, blocked_until
               FROM tasks_v113;
             DROP TABLE tasks_v113;
             CREATE INDEX tasks_by_hive ON tasks(hive_id);
             CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
             CREATE INDEX task_owner_queue
                 ON tasks(assigned_worker_id, state)
                 WHERE assigned_worker_id IS NOT NULL
                   AND state NOT IN ('completed','abandoned');
             CREATE INDEX tasks_visible_queue
                 ON tasks(hive_id, state) WHERE removed_at IS NULL;
             CREATE TRIGGER tasks_require_hive_insert
                 BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
                 BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
             CREATE TRIGGER tasks_require_hive_update
                 BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
                 BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(None, "user_version", crate::AWAITING_RELEASE_SCHEMA_VERSION)
}

/// A task records the commits it produced, and what checking them found.
///
/// TWO TABLES BECAUSE THERE ARE TWO FACTS. The report says a worker answered
/// the question and whether the repository could be read at all; the rows say
/// what it answered. A task with no report row has been asked nothing, which is
/// NOT the same as a worker reporting that nothing was built -- and the whole
/// value of this record is that the next step can tell those apart. Collapsing
/// them would let unreported work read as an investigation that produced
/// nothing, and close automatically on a question never asked.
fn migrate_task_commit_reports(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_commit_reports (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             workspace TEXT NOT NULL,
             repository_state TEXT NOT NULL
                 CHECK (repository_state IN ('read','not_a_repository')),
             reported_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE TABLE IF NOT EXISTS task_commits (
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             sha TEXT NOT NULL,
             verdict TEXT NOT NULL
                 CHECK (verdict IN ('present','unreachable','missing','unchecked')),
             subject TEXT NOT NULL DEFAULT '',
             -- Newline separated, and stored as fact. Which paths count as
             -- documentation is a policy applied later, not a judgement baked
             -- in here where it could never be revisited.
             changed_paths TEXT NOT NULL DEFAULT '',
             recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
             PRIMARY KEY (task_id, sha)
         );
         CREATE INDEX IF NOT EXISTS task_commits_by_task ON task_commits(task_id);",
    )?;
    transaction.pragma_update(None, "user_version", TASK_COMMIT_REPORT_SCHEMA_VERSION)
}

/// The coordinator may approve what it settled, and is named as itself.
///
/// A REBUILD, because the vocabulary was enforced in two places and only one of
/// them is Rust. Relaxing the guard in `approve_completion_exemption` left this
/// CHECK standing, and the database refused the write — correctly. Writing
/// "queen" instead would have passed both and been a lie: the sweep would be
/// claiming a person looked at it.
///
/// No table holds a foreign key into this one and it carries no index or
/// trigger, so the rebuild is the plain twelve-step form.
fn migrate_coordinator_approves_settlements(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let table_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'task_completion_exemptions')",
        [],
        |row| row.get(0),
    )?;
    let already: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'task_completion_exemptions'
           AND sql LIKE '%coordinator%')",
        [],
        |row| row.get(0),
    )?;
    let targets_exist: bool = transaction.query_row(
        "SELECT (SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('tasks','worker_profiles')) = 2",
        [],
        |row| row.get(0),
    )?;
    if table_exists && targets_exist && !already {
        transaction.execute_batch(
            "PRAGMA legacy_alter_table = ON;
             ALTER TABLE task_completion_exemptions RENAME TO task_completion_exemptions_v111;
             CREATE TABLE task_completion_exemptions (
                 task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
                 reason TEXT NOT NULL,
                 claimed_by_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
                 claimed_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 approved_at INTEGER,
                 approved_by TEXT
                     CHECK (approved_by IS NULL
                            OR approved_by IN ('queen','operator','coordinator')),
                 superseded_at INTEGER
             );
             INSERT INTO task_completion_exemptions
                 (task_id, reason, claimed_by_worker_id, claimed_at,
                  approved_at, approved_by, superseded_at)
             SELECT task_id, reason, claimed_by_worker_id, claimed_at,
                    approved_at, approved_by, superseded_at
               FROM task_completion_exemptions_v111;
             DROP TABLE task_completion_exemptions_v111;
             PRAGMA legacy_alter_table = OFF;",
        )?;
    }
    transaction.pragma_update(None, "user_version", COORDINATOR_SETTLEMENT_SCHEMA_VERSION)
}

/// The bee an operator chose for a worker.
///
/// NULLABLE, AND NULL IS THE ORDINARY CASE. A worker with no choice draws a mark
/// derived from its id, so every Hive is dressed without anybody setting
/// anything and this column stays empty until somebody disagrees with the
/// derivation. That is why there is no default and no backfill: writing a
/// derived value into every row would freeze today's derivation into the
/// database and make the set impossible to extend without a second migration.
///
/// UNCONSTRAINED ON PURPOSE. A CHECK listing the marks would make adding one a
/// schema change, which is exactly the trap migration 96 was written to remove
/// for providers. The reader validates instead: an unrecognised mark falls back
/// to the derived one, so a value from a newer build, or one since retired,
/// costs a worker nothing.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_worker_mark(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // Table existence AND column absence, for the reason migrate_ephemeral_workers
    // records: the migration tests rewind user_version WITHOUT rewinding tables,
    // and pragma_table_info on a missing table returns no rows rather than
    // failing — so checking only the column reads "not present" and then ALTERs
    // a table that is not there.
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    let has_column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_profiles')
                        WHERE name = 'mark')",
        [],
        |row| row.get(0),
    )?;
    if present && !has_column {
        transaction.execute_batch("ALTER TABLE worker_profiles ADD COLUMN mark TEXT;")?;
    }
    transaction.pragma_update(None, "user_version", WORKER_MARK_SCHEMA_VERSION)
}

/// Which outside tool a profile IS, when it is not a person's worker.
///
/// ONE NULLABLE COLUMN, AND NULL IS EVERY EXISTING ROW. An outside tool that
/// files work needs a durable identity: `worker_profiles` is referenced by
/// foreign keys from tasks, activity, briefings, decisions and a dozen other
/// tables, so a connection that writes to the board must BE one of these rows
/// or the writes have nothing to point at. Flagging the row is what keeps it
/// out of the roster and the live-worker count without inventing a second kind
/// of author the rest of the schema has never heard of.
///
/// DELIBERATELY NOT A WIDENED `system_role` CHECK. That column already exists
/// and already means "a profile the Hive made rather than the operator", so it
/// looked like the natural home. `SQLite` cannot alter a CHECK constraint: taking
/// that route means rebuilding `worker_profiles` — the table seventeen others
/// hold foreign keys into, and the one whose rebuild already failed once on the
/// operator's real database. An added nullable column needs no rebuild, no
/// backfill and no data movement, so the worst case is a column nothing reads.
///
/// # Errors
///
/// Returns an error when the step cannot be applied.
fn migrate_connection_principal(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // Table existence AND column absence, for the reason migrate_worker_mark
    // records above: the migration tests rewind user_version without rewinding
    // tables, and pragma_table_info on a missing table returns no rows rather
    // than failing.
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get(0),
    )?;
    let has_column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_profiles')
                        WHERE name = 'connection_client_id')",
        [],
        |row| row.get(0),
    )?;
    if present && !has_column {
        transaction
            .execute_batch("ALTER TABLE worker_profiles ADD COLUMN connection_client_id TEXT;")?;
    }
    if present {
        // One profile per client. A second registration by the same client must
        // find the identity it already has rather than quietly grow another,
        // which is how a board fills with duplicate authors nobody can tell
        // apart.
        transaction.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS one_profile_per_connection
                 ON worker_profiles(connection_client_id)
                 WHERE connection_client_id IS NOT NULL;",
        )?;
    }
    transaction.pragma_update(None, "user_version", CONNECTION_PRINCIPAL_SCHEMA_VERSION)
}

/// Which GitHub issues have already arrived, so none arrives twice.
///
/// The intake polls; a poll that could not remember what it had seen would file
/// the same issue on every tick and bury the board it is meant to feed. Keyed on
/// the issue's own URL because that is stable, unique per repository, and is
/// what a person clicks.
fn migrate_github_issue_intake(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let tasks: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    if tasks {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS github_issue_tasks (
                 issue_url TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 imported_at INTEGER NOT NULL DEFAULT (unixepoch())
             );",
        )?;
    }
    transaction.pragma_update(None, "user_version", GITHUB_ISSUE_INTAKE_SCHEMA_VERSION)
}

/// Where a dogfood report went, when it went anywhere.
///
/// Feedback saved locally and stopped. A person who submitted one had no way to
/// tell afterwards whether it had reached anybody — a colleague filed a report,
/// believed she had raised an issue, and it sat in a Hive nobody was watching.
/// Recording the issue is what makes the answer readable off the report itself
/// rather than inferred from whether a button once said "Saved".
///
/// A plain ADD COLUMN, guarded on the column's absence and on the table's, for
/// the reason `migrate_block_deadline` records.
fn migrate_feedback_issue(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let table: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'dogfood_reports')",
        [],
        |row| row.get(0),
    )?;
    if table {
        let present: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('dogfood_reports') WHERE name = 'github_issue_url')",
            [],
            |row| row.get(0),
        )?;
        if !present {
            transaction
                .execute_batch("ALTER TABLE dogfood_reports ADD COLUMN github_issue_url TEXT;")?;
        }
    }
    transaction.pragma_update(None, "user_version", FEEDBACK_ISSUE_SCHEMA_VERSION)
}

/// The operator's record that finished work cannot now be shown to be live.
///
/// A SEPARATE TABLE FROM `task_completion_exemptions` ON PURPOSE, and the
/// distinction is the whole reason the operator asked for this. An exemption
/// says there was nothing to ship — somebody looked and agreed. This says the
/// opposite shape: something may well have shipped, and nobody can now prove
/// it. Overloading one table with both would collapse exactly the difference
/// the record exists to preserve, and the board would show "verified" for work
/// nobody verified.
///
/// Both guards are required, for the reason `migrate_block_deadline` records: a
/// database old enough to predate the tasks table reaches this step too.
fn migrate_unverifiable_closures(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let tasks: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    if tasks {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_unverifiable_closures (
                 task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
                 note TEXT NOT NULL,
                 recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
             );",
        )?;
    }
    transaction.pragma_update(None, "user_version", UNVERIFIABLE_CLOSURE_SCHEMA_VERSION)
}

/// When a block's own stated condition arrives, if it named one.
///
/// A plain ADD COLUMN, so none of migration 96's difficulty: no table rebuild
/// and nothing holding a foreign key into it. Guarded on the column's absence
/// because the migration tests rewind `user_version` without rewinding tables.
fn migrate_block_deadline(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    // BOTH GUARDS ARE REQUIRED. The column check alone is not enough: a
    // database old enough to predate the tasks table reaches this step too, and
    // pragma_table_info on a missing table returns no rows -- which reads as
    // "the column is absent" and sends the ALTER at a table that is not there.
    // The migration harness starts from exactly such a database.
    let table: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    let column: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'blocked_until')",
        [],
        |row| row.get(0),
    )?;
    if table && !column {
        transaction.execute_batch("ALTER TABLE tasks ADD COLUMN blocked_until INTEGER;")?;
    }
    transaction.pragma_update(None, "user_version", BLOCK_DEADLINE_SCHEMA_VERSION)
}

/// Puts every amendment that already exists into the activity trail.
///
/// IDEMPOTENT BY NECESSITY, not by taste. The migration harness rewinds
/// `user_version` WITHOUT rewinding tables, so this runs again over rows it has
/// already written. A bare INSERT..SELECT would duplicate every amendment on
/// each re-run, and that does not fail loudly -- it makes every task look
/// freshly touched, which silently drags two attention clocks forward. The NOT
/// EXISTS guard is the whole safety property.
///
/// `occurred_at` is the amendment's OWN `created_at`, never `unixepoch()`. Those
/// clocks read this column, so stamping it with migration time would move every
/// historical amendment to now and mute both flags across the whole board.
///
/// Guards table existence because a database old enough to predate
/// `task_amendments` reaches this step too.
fn migrate_amendment_activity(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = 'task_amendments')",
        [],
        |row| row.get(0),
    )?;
    if present {
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, occurred_at, actor_kind, actor_id)
             SELECT amendment.task_id, 'amended', amendment.body, amendment.created_at,
                    'worker', amendment.author_worker_id
             FROM task_amendments amendment
             WHERE NOT EXISTS (
                 SELECT 1 FROM task_activity existing
                 WHERE existing.task_id = amendment.task_id
                   AND existing.kind = 'amended'
                   AND existing.occurred_at = amendment.created_at
                   AND existing.actor_id = amendment.author_worker_id
             )",
            [],
        )?;
    }
    // Stamped even when the table was absent: the step is still satisfied, and
    // returning without it leaves the database below the ceiling forever.
    transaction.pragma_update(None, "user_version", AMENDMENT_ACTIVITY_SCHEMA_VERSION)
}

/// Every request to set a terminal's size, and what came of it.
///
/// Two devices trading a terminal's size back and forth has now been reported
/// three times and diagnosed three times from screenshots and code reading. Two
/// of those diagnoses were wrong, and the third could not be told apart from
/// the legitimate case it broke. Nothing recorded who asked for which size,
/// whether they claimed it, or whether they were granted it, so every attempt
/// was inference.
///
/// Deliberately records refused requests too: a fight is visible precisely in
/// the requests that were turned down and repeated.
fn migrate_terminal_geometry_ledger(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS terminal_geometry_events (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id TEXT NOT NULL,
             device_id TEXT,
             rows INTEGER NOT NULL,
             columns INTEGER NOT NULL,
             claimed INTEGER NOT NULL,
             granted INTEGER NOT NULL,
             owner_before TEXT,
             at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS terminal_geometry_events_by_session
             ON terminal_geometry_events(session_id, at);",
    )?;
    transaction.pragma_update(
        None,
        "user_version",
        TERMINAL_GEOMETRY_LEDGER_SCHEMA_VERSION,
    )?;
    Ok(())
}

/// Passkeys registered for signing in to this Hive.
///
/// A credential is bound to one relying-party ID — the domain it was registered
/// at — so the domain is stored beside it. This Hive answers on localhost and on
/// a public host, and a passkey for one is not usable at the other; keeping the
/// domain means the wrong ones are never offered rather than failing at the
/// browser.
///
/// The label is the operator's, so a credential can be recognised before it is
/// removed. A passkey that cannot be told apart from another is one that cannot
/// safely be revoked.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_operator_passkeys(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS operator_passkeys (
             credential_id TEXT PRIMARY KEY,
             relying_party TEXT NOT NULL,
             label TEXT NOT NULL,
             credential TEXT NOT NULL,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             last_used_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS operator_passkeys_by_party
             ON operator_passkeys(relying_party, created_at);",
    )?;
    transaction.pragma_update(None, "user_version", OPERATOR_PASSKEY_SCHEMA_VERSION)
}

/// What the coordinator wanted to do and could not.
///
/// It records what it did and nothing else, so a view built on its actions
/// alone is blank exactly when someone opens it. Measured: through the
/// twenty-four hours the operator spent wondering why the Hive was idle, the
/// coordinator took no action at all — the only evidence was one log line
/// repeated 1503 times.
///
/// One row per thing refused, not one per attempt. The count and the first
/// observation are what make it readable: "held since 01:49, 1503 checks" is a
/// fact; 1503 rows is the journal with a stylesheet.
///
/// # Errors
/// Returns an error when the step cannot be applied.
fn migrate_coordinator_refusals(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS coordinator_refusals (
             kind TEXT NOT NULL,
             subject TEXT NOT NULL,
             worker_id TEXT REFERENCES worker_profiles(id) ON DELETE CASCADE,
             session_id TEXT,
             reason TEXT NOT NULL,
             first_observed_at INTEGER NOT NULL,
             last_observed_at INTEGER NOT NULL,
             observations INTEGER NOT NULL DEFAULT 1,
             cleared_at INTEGER,
             PRIMARY KEY (kind, subject)
         );
         CREATE INDEX IF NOT EXISTS coordinator_refusals_live
             ON coordinator_refusals(cleared_at, first_observed_at);",
    )?;
    transaction.pragma_update(None, "user_version", COORDINATOR_REFUSAL_SCHEMA_VERSION)
}

/// Carries one operator line about how a task should be approached.
///
/// A forward step, and guarded on the table, because a database old enough to
/// predate `tasks` passes through here on its way up.
fn migrate_task_operator_instruction(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let tasks_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    let column_exists: bool = tasks_exist
        && transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'operator_instruction')",
            [],
            |row| row.get(0),
        )?;
    if tasks_exist && !column_exists {
        transaction.execute_batch(
            "ALTER TABLE tasks ADD COLUMN operator_instruction TEXT NOT NULL DEFAULT '';",
        )?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        TASK_OPERATOR_INSTRUCTION_SCHEMA_VERSION,
    )
}

fn migrate_task_removal(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let tasks_exist: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |row| row.get(0),
    )?;
    if !tasks_exist {
        return transaction.pragma_update(None, "user_version", TASK_REMOVAL_SCHEMA_VERSION);
    }
    let column_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'removed_at')",
        [],
        |row| row.get(0),
    )?;
    if !column_exists {
        transaction.execute("ALTER TABLE tasks ADD COLUMN removed_at INTEGER", [])?;
    }
    transaction.execute(
        "CREATE INDEX IF NOT EXISTS tasks_visible_queue
         ON tasks(hive_id, state) WHERE removed_at IS NULL",
        [],
    )?;
    transaction.pragma_update(None, "user_version", TASK_REMOVAL_SCHEMA_VERSION)
}

fn migrate_terminal_geometry_ownership(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let sessions_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_sessions')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    let engagements_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_engagements')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    let geometry_owner_exists = sessions_exist
        && transaction
            .prepare("PRAGMA table_info(worker_sessions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "geometry_owner_device_id");
    if sessions_exist && !geometry_owner_exists {
        transaction.execute_batch(
            "ALTER TABLE worker_sessions ADD COLUMN geometry_owner_device_id TEXT;",
        )?;
    }
    if sessions_exist && engagements_exist {
        transaction.execute_batch(
            "UPDATE worker_sessions
             SET geometry_owner_device_id = (
                 SELECT owner_device_id FROM worker_engagements
                 WHERE worker_engagements.session_id = worker_sessions.session_id
             )
             WHERE geometry_owner_device_id IS NULL
               AND EXISTS (
                   SELECT 1 FROM worker_engagements
                   WHERE worker_engagements.session_id = worker_sessions.session_id
               );",
        )?;
    }
    transaction.pragma_update(None, "user_version", TERMINAL_GEOMETRY_SCHEMA_VERSION)
}

fn migrate_managed_worker_roles(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let worker_profiles_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !worker_profiles_exist {
        return transaction.execute_batch("PRAGMA user_version = 51;");
    }
    let has_system_role = {
        let mut statement = transaction.prepare("PRAGMA table_info(worker_profiles)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "system_role")
    };
    if !has_system_role {
        transaction.execute_batch(
            "ALTER TABLE worker_profiles
             ADD COLUMN system_role TEXT CHECK (system_role IS NULL OR system_role = 'scout');",
        )?;
    }
    transaction.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS one_scout_per_hive
             ON worker_profiles(hive_id) WHERE system_role = 'scout' AND archived_at IS NULL;
         PRAGMA user_version = 51;",
    )
}

fn migrate_worker_profile_metadata(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let worker_profiles_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if worker_profiles_exist {
        let columns = {
            let mut statement = transaction.prepare("PRAGMA table_info(worker_profiles)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()?
        };
        if !columns.contains("description") {
            transaction.execute_batch(
                "ALTER TABLE worker_profiles
                 ADD COLUMN description TEXT NOT NULL DEFAULT '';",
            )?;
        }
        if !columns.contains("archived_at") {
            transaction
                .execute_batch("ALTER TABLE worker_profiles ADD COLUMN archived_at INTEGER;")?;
        }
        let has_roster_columns = ["role", "position", "created_at", "id"]
            .iter()
            .all(|column| columns.contains(*column));
        if has_roster_columns {
            transaction.execute_batch(
                "CREATE INDEX IF NOT EXISTS worker_profiles_active_roster
                     ON worker_profiles(role, position, created_at, id)
                     WHERE archived_at IS NULL;",
            )?;
        }
    }
    transaction.pragma_update(None, "user_version", 49)
}

fn migrate_task_activity_actors(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let activity_exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'task_activity')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if activity_exists {
        let actor_kind_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'actor_kind')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !actor_kind_exists {
            transaction.execute_batch(
                "ALTER TABLE task_activity ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'system'
                     CHECK (actor_kind IN ('operator','worker','jira','email','system'));",
            )?;
        }
        let actor_id_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'actor_id')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !actor_id_exists {
            transaction.execute_batch("ALTER TABLE task_activity ADD COLUMN actor_id TEXT;")?;
        }
    }
    transaction.pragma_update(None, "user_version", 44)
}

fn migrate_federation_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < 30 {
        federation::migrate_federation_identity(transaction)?;
    }
    if schema_version < 31 {
        federation::migrate_federation_candidates(transaction)?;
    }
    if schema_version < 32 {
        federation::migrate_federation_invitations(transaction)?;
    }
    if schema_version < 33 {
        federation::migrate_federation_join_invitations(transaction)?;
    }
    if schema_version < 34 {
        federation::migrate_federation_join_invitation_projects(transaction)?;
    }
    if schema_version < 35 {
        federation::migrate_federation_memberships(transaction)?;
    }
    if schema_version < 36 {
        federation::migrate_local_federation_membership(transaction)?;
    }
    if schema_version < 37 {
        federation::migrate_local_federation_catalog(transaction)?;
    }
    if schema_version < 38 {
        federation::migrate_federation_claims(transaction)?;
    }
    if schema_version < 39 {
        federation::migrate_local_federation_sync(transaction)?;
    }
    Ok(())
}

fn migrate_apiary_stewardships(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS stewardships (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             steward_operator_id TEXT NOT NULL REFERENCES operators(id),
             created_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             revoked_at INTEGER
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_stewardship_per_operator
             ON stewardships(apiary_id, steward_operator_id) WHERE revoked_at IS NULL;
         CREATE TABLE IF NOT EXISTS stewardship_hive_grants (
             stewardship_id TEXT NOT NULL REFERENCES stewardships(id) ON DELETE CASCADE,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             PRIMARY KEY (stewardship_id, hive_id)
         );
         CREATE TABLE IF NOT EXISTS stewardship_capability_grants (
             stewardship_id TEXT NOT NULL REFERENCES stewardships(id) ON DELETE CASCADE,
             capability TEXT NOT NULL CHECK (capability IN (
                 'observe','assign','assist','takeover','manage_projects','manage_members'
             )),
             PRIMARY KEY (stewardship_id, capability)
         );
         CREATE TRIGGER IF NOT EXISTS stewardship_creator_is_keeper
             BEFORE INSERT ON stewardships
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.keeper_operator_id = NEW.created_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only the Apiary Keeper can grant Stewardship'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_stewardship_identity
             BEFORE UPDATE OF id, apiary_id, steward_operator_id, created_by_operator_id
             ON stewardships
             BEGIN SELECT RAISE(ABORT, 'Stewardship identity is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS stewardship_hive_scope_insert
             BEFORE INSERT ON stewardship_hive_grants
             WHEN NOT EXISTS (
                 SELECT 1 FROM stewardships s
                 JOIN hives h ON h.id = NEW.hive_id
                 WHERE s.id = NEW.stewardship_id AND h.apiary_id = s.apiary_id
             )
             BEGIN SELECT RAISE(ABORT, 'Steward Hive grant must stay inside its Apiary'); END;
         CREATE TRIGGER IF NOT EXISTS stewardship_hive_scope_update
             BEFORE UPDATE OF stewardship_id, hive_id ON stewardship_hive_grants
             WHEN NOT EXISTS (
                 SELECT 1 FROM stewardships s
                 JOIN hives h ON h.id = NEW.hive_id
                 WHERE s.id = NEW.stewardship_id AND h.apiary_id = s.apiary_id
             )
             BEGIN SELECT RAISE(ABORT, 'Steward Hive grant must stay inside its Apiary'); END;
         PRAGMA user_version = 25;",
    )
}

fn migrate_jira_assigned_sync_preference(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let column_exists: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('jira_project_bindings')
             WHERE name = 'auto_sync_assigned'
         )",
        [],
        |row| row.get(0),
    )?;
    if !column_exists {
        transaction.execute_batch(
            "ALTER TABLE jira_project_bindings
                 ADD COLUMN auto_sync_assigned INTEGER NOT NULL DEFAULT 0
                 CHECK (auto_sync_assigned IN (0,1));",
        )?;
    }
    transaction.pragma_update(None, "user_version", 24)
}

fn migrate_jira_comment_deliveries(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_comment_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             body TEXT NOT NULL,
             state TEXT NOT NULL CHECK (
                 state IN ('queued','dispatching','delivered','conflict','uncertain')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS jira_comment_delivery_queue
             ON jira_comment_deliveries(state, available_at, created_at);
         PRAGMA user_version = 23;",
    )
}

fn migrate_jira_transition_deliveries(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_transition_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             target_task_state TEXT NOT NULL CHECK (
                 target_task_state IN ('draft','ready','active','blocked','review','completed')
             ),
             state TEXT NOT NULL CHECK (
                 state IN ('queued','dispatching','delivered','conflict','uncertain')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE UNIQUE INDEX IF NOT EXISTS jira_transition_one_active_per_task
             ON jira_transition_deliveries(task_id)
             WHERE state IN ('queued','dispatching');
         CREATE INDEX IF NOT EXISTS jira_transition_delivery_queue
             ON jira_transition_deliveries(state, available_at, updated_at);
         PRAGMA user_version = 22;",
    )
}

fn migrate_jira_bindings(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_project_bindings (
             id TEXT PRIMARY KEY,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             project_id TEXT NOT NULL,
             project_key TEXT NOT NULL,
             project_name TEXT NOT NULL,
             scope TEXT NOT NULL CHECK (scope IN ('hive','apiary')),
             apiary_id TEXT REFERENCES apiaries(id),
             default_worker_id TEXT REFERENCES worker_profiles(id),
             access_verified INTEGER NOT NULL DEFAULT 0 CHECK (access_verified IN (0,1)),
             workflow_mapped INTEGER NOT NULL DEFAULT 0 CHECK (workflow_mapped IN (0,1)),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE(hive_id, project_id),
             UNIQUE(hive_id, project_key),
             CHECK ((scope = 'hive' AND apiary_id IS NULL) OR
                    (scope = 'apiary' AND apiary_id IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS jira_project_bindings_by_hive
             ON jira_project_bindings(hive_id, project_name, project_key);
         CREATE TABLE IF NOT EXISTS jira_status_mappings (
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id) ON DELETE CASCADE,
             jira_status_id TEXT NOT NULL,
             jira_status_name TEXT NOT NULL,
             task_state TEXT NOT NULL CHECK (
                 task_state IN ('draft','ready','active','blocked','review','completed')
             ),
             PRIMARY KEY(binding_id, jira_status_id)
         );
         CREATE TABLE IF NOT EXISTS jira_issue_links (
             issue_id TEXT PRIMARY KEY,
             issue_key TEXT NOT NULL UNIQUE,
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             jira_status_id TEXT NOT NULL,
             jira_status_name TEXT NOT NULL,
             jira_assignee_account_id TEXT,
             jira_assignee_name TEXT,
             remote_updated_at TEXT NOT NULL,
             last_synced_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS jira_issue_links_by_binding
             ON jira_issue_links(binding_id, issue_key);
         PRAGMA user_version = 21;",
    )
}

fn migrate_dogfood_reports(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS dogfood_reports (
            id TEXT PRIMARY KEY,
            expectation TEXT NOT NULL,
            observation TEXT NOT NULL,
            diagnostic_bundle TEXT NOT NULL,
            attachment_name TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS dogfood_reports_newest
            ON dogfood_reports(created_at DESC, id DESC);
         PRAGMA user_version = 20;",
    )
}

fn migrate_durable_task_ownership(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let has_owner = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'assigned_worker_id')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_owner {
        transaction.execute(
            "ALTER TABLE tasks ADD COLUMN assigned_worker_id TEXT REFERENCES worker_profiles(id)",
            [],
        )?;
    }
    let has_assignments = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'task_assignments')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if has_assignments {
        transaction.execute_batch(
            "UPDATE tasks
             SET assigned_worker_id = (
                 SELECT session.worker_id
                 FROM task_assignments assignment
                 JOIN worker_sessions session ON session.session_id = assignment.worker_session_id
                 WHERE assignment.task_id = tasks.id AND assignment.released_at IS NULL
                 LIMIT 1
             )
             WHERE assigned_worker_id IS NULL;",
        )?;
    }
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS task_owner_queue
             ON tasks(assigned_worker_id, state)
             WHERE assigned_worker_id IS NOT NULL AND state != 'completed';
         PRAGMA user_version = 19;",
    )
}

fn migrate_queen_autonomy(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_autonomy_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             at_hive TEXT NOT NULL CHECK (at_hive IN ('advisory','coordinate','local_execution')),
             away TEXT NOT NULL CHECK (away IN ('advisory','coordinate','local_execution')),
             night_watch TEXT NOT NULL CHECK (night_watch IN ('advisory','coordinate','local_execution')),
             updated_at INTEGER NOT NULL
         );
         PRAGMA user_version = 17;",
    )
}

fn migrate_presentation_preferences(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS presentation_preferences (
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             color_theme TEXT NOT NULL CHECK (color_theme IN ('light','dark')),
             terminal_keys_visible INTEGER NOT NULL CHECK (terminal_keys_visible IN (0,1)),
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (operator_id, device_class)
         );
         PRAGMA user_version = 18;",
    )
}

fn migrate_worker_roster(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS worker_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE UNIQUE,
            role TEXT NOT NULL CHECK (role IN ('queen','worker')),
            -- No CHECK on provider, deliberately. See migrate_open_provider_set:
            -- the closed list made adding a provider a schema change, and
            -- ProviderKind already refuses an unknown one at every write.
            provider TEXT NOT NULL,
            workspace TEXT NOT NULL,
            autostart INTEGER NOT NULL CHECK (autostart IN (0,1)),
            position INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_queen_profile
            ON worker_profiles(role) WHERE role = 'queen';
        CREATE TABLE IF NOT EXISTS worker_sessions (
            session_id TEXT PRIMARY KEY,
            worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
            started_at INTEGER NOT NULL DEFAULT (unixepoch()),
            ended_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_session_per_worker
            ON worker_sessions(worker_id) WHERE ended_at IS NULL;
        PRAGMA user_version = 2;
        ",
    )
}

fn migrate_task_details(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
         ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal'
             CHECK (priority IN ('low','normal','high','urgent'));
         PRAGMA user_version = 3;",
    )
}

fn migrate_hive_identity(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let operator_id = OperatorId::new();
    let hive_id = HiveId::new();
    transaction.execute_batch(
        "
        CREATE TABLE operators (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE apiaries (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            keeper_operator_id TEXT NOT NULL REFERENCES operators(id),
            shared_work_backend TEXT NOT NULL
                CHECK (shared_work_backend IN ('jira','native')),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE hives (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            operator_id TEXT NOT NULL UNIQUE REFERENCES operators(id),
            apiary_id TEXT REFERENCES apiaries(id),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE local_hive_identity (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            hive_id TEXT NOT NULL UNIQUE REFERENCES hives(id)
        );
        ",
    )?;
    transaction.execute(
        "INSERT INTO operators (id, display_name) VALUES (?1, 'Operator')",
        [operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'My Hive', ?2)",
        params![hive_id.to_string(), operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO local_hive_identity (singleton, hive_id) VALUES (1, ?1)",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        ALTER TABLE tasks ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ALTER TABLE worker_profiles ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ",
    )?;
    transaction.execute(
        "UPDATE tasks SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute(
        "UPDATE worker_profiles SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        CREATE INDEX tasks_by_hive ON tasks(hive_id);
        CREATE INDEX worker_profiles_by_hive ON worker_profiles(hive_id);
        CREATE TRIGGER tasks_require_hive_insert
            BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER tasks_require_hive_update
            BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_insert
            BEFORE INSERT ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_update
            BEFORE UPDATE OF hive_id ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER immutable_apiary_backend
            BEFORE UPDATE OF shared_work_backend ON apiaries
            BEGIN SELECT RAISE(ABORT, 'Apiary shared-work backend is immutable'); END;
        PRAGMA user_version = 4;
        ",
    )
}

fn migrate_control_room_events(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE control_room_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            hive_id TEXT NOT NULL REFERENCES hives(id),
            kind TEXT NOT NULL CHECK (
                kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed')
            ),
            occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE INDEX control_room_events_by_hive_sequence
            ON control_room_events(hive_id, sequence);
        PRAGMA user_version = 5;
        ",
    )
}

fn migrate_task_ordering(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
         WITH ranked AS (
             SELECT id, ROW_NUMBER() OVER (
                 PARTITION BY hive_id ORDER BY created_at, id
             ) - 1 AS new_position
             FROM tasks
         )
         UPDATE tasks SET position = (
             SELECT new_position FROM ranked WHERE ranked.id = tasks.id
         );
         CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
         PRAGMA user_version = 6;",
    )
}

fn migrate_provider_conversations(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE worker_profiles ADD COLUMN provider_conversation_id TEXT;
         PRAGMA user_version = 7;",
    )
}

fn migrate_worker_engagements(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE worker_engagements (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             session_id TEXT NOT NULL UNIQUE REFERENCES worker_sessions(session_id) ON DELETE CASCADE,
             engaged_at INTEGER NOT NULL,
             renewed_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL CHECK (expires_at > renewed_at)
         );
         PRAGMA user_version = 8;",
    )
}

fn migrate_engagement_ownership(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let already_owned = {
        let mut statement = transaction.prepare("PRAGMA table_info(worker_engagements)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "owner_device_id")
    };
    if !already_owned {
        transaction
            .execute_batch("ALTER TABLE worker_engagements ADD COLUMN owner_device_id TEXT;")?;
    }
    transaction.pragma_update(None, "user_version", 16)
}

fn migrate_agent_credentials(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_agent_credentials (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             token_digest BLOB NOT NULL UNIQUE CHECK (length(token_digest) = 32),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             rotated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         PRAGMA user_version = 9;",
    )
}
fn migrate_decision_requests(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v9;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v9;
         DROP TABLE control_room_events_v9;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS decision_requests (
             id TEXT PRIMARY KEY,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             requesting_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT REFERENCES tasks(id),
             kind TEXT NOT NULL CHECK (kind IN ('input','approval','credentials','conflict','help')),
             urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
             title TEXT NOT NULL,
             reason TEXT NOT NULL,
             risk TEXT NOT NULL,
             evidence TEXT NOT NULL,
             suggested_action TEXT NOT NULL,
             allowed_actions TEXT NOT NULL,
             deadline INTEGER,
             state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','resolved')),
             resolution_action TEXT,
             resolution_note TEXT NOT NULL DEFAULT '',
             resolved_by_operator_id TEXT REFERENCES operators(id),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             resolved_at INTEGER,
             CHECK ((state = 'pending' AND resolution_action IS NULL AND resolved_at IS NULL)
                 OR (state = 'resolved' AND resolution_action IS NOT NULL AND resolved_at IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS decision_requests_inbox
             ON decision_requests(hive_id, state, urgency, deadline, created_at DESC);
         CREATE INDEX IF NOT EXISTS decision_requests_by_worker
             ON decision_requests(requesting_worker_id, state, created_at DESC);
         PRAGMA user_version = 10;",
    )
}
fn migrate_decision_deliveries(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS decision_deliveries (
             decision_id TEXT PRIMARY KEY REFERENCES decision_requests(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             session_id TEXT REFERENCES worker_sessions(session_id),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS decision_deliveries_queue
             ON decision_deliveries(state, updated_at, decision_id);
         PRAGMA user_version = 11;",
    )
}
fn migrate_task_dispatches(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_dispatches (
             assignment_id TEXT PRIMARY KEY REFERENCES task_assignments(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS task_dispatches_queue
             ON task_dispatches(state, updated_at, assignment_id);
         PRAGMA user_version = 12;",
    )
}
fn migrate_task_outcomes(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_activity (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             kind TEXT NOT NULL,
             from_state TEXT,
             to_state TEXT,
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );",
    )?;
    let has_note = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'note'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_note {
        transaction.execute(
            "ALTER TABLE task_activity ADD COLUMN note TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_outcome_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             activity_sequence INTEGER NOT NULL UNIQUE REFERENCES task_activity(sequence) ON DELETE CASCADE,
             reporting_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             recipient_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             target_state TEXT NOT NULL CHECK (target_state IN ('blocked','review')),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             session_id TEXT REFERENCES worker_sessions(session_id),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS task_outcome_deliveries_queue
             ON task_outcome_deliveries(state, updated_at, id);
         PRAGMA user_version = 13;",
    )
}
fn migrate_operator_presence(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v13;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed','presence_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v13;
         DROP TABLE control_room_events_v13;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS operator_presence_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             manual_mode TEXT CHECK (manual_mode IS NULL OR manual_mode IN ('at_hive','away','night_watch')),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE TABLE IF NOT EXISTS operator_presence_devices (
             id TEXT PRIMARY KEY,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             state TEXT NOT NULL CHECK (state IN ('active','idle','locked','hidden')),
             expires_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             CHECK (expires_at > updated_at)
         );
         CREATE INDEX IF NOT EXISTS operator_presence_devices_current
             ON operator_presence_devices(operator_id, expires_at, state);
         PRAGMA user_version = 14;",
    )
}
fn migrate_notifications(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v14;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed','presence_changed','notifications_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v14;
         DROP TABLE control_room_events_v14;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS notification_vapid_keys (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             private_key BLOB NOT NULL CHECK (length(private_key) = 32),
             public_key BLOB NOT NULL CHECK (length(public_key) = 65),
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS notification_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             policy TEXT NOT NULL CHECK (policy IN ('important_only','all_decisions','off')),
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS notification_subscriptions (
             device_id TEXT PRIMARY KEY,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             endpoint TEXT NOT NULL UNIQUE CHECK (length(endpoint) BETWEEN 1 AND 4096),
             p256dh BLOB NOT NULL CHECK (length(p256dh) = 65),
             auth BLOB NOT NULL CHECK (length(auth) = 16),
             failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS notification_subscriptions_by_operator
             ON notification_subscriptions(operator_id, updated_at);
         CREATE TABLE IF NOT EXISTS notification_deliveries (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             subscription_id TEXT NOT NULL REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
             decision_id TEXT REFERENCES decision_requests(id) ON DELETE CASCADE,
             urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
             kind TEXT NOT NULL CHECK (kind IN ('decision','test')),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching')),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
             available_at INTEGER NOT NULL,
             created_at INTEGER NOT NULL,
             CHECK ((kind = 'decision' AND decision_id IS NOT NULL) OR (kind = 'test' AND decision_id IS NULL)),
             UNIQUE(decision_id, subscription_id)
         );
         CREATE INDEX IF NOT EXISTS notification_deliveries_ready
             ON notification_deliveries(state, available_at, urgency, id);
         PRAGMA user_version = 15;",
    )
}
/// Keeps a refusal to one line when a task title is a paragraph.
fn truncate_for_refusal(title: &str) -> String {
    const MAX: usize = 60;
    let title = title.trim();
    if title.chars().count() <= MAX {
        return title.to_owned();
    }
    // By CHARACTERS, not bytes: titles carry em dashes and this would otherwise
    // panic on a multi-byte boundary.
    let kept: String = title.chars().take(MAX).collect();
    format!("{}...", kept.trim_end())
}

fn ensure_worker_has_no_other_active_task(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
) -> Result<(), TaskStoreError> {
    let assigned_worker_id = transaction
        .query_row(
            "SELECT assigned_worker_id FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let Some(worker_id) = assigned_worker_id else {
        return Ok(());
    };
    let holder = transaction
        .query_row(
            "SELECT id, title FROM tasks
             WHERE assigned_worker_id = ?1
               AND id != ?2
               AND state = 'active'
               AND removed_at IS NULL
             LIMIT 1",
            params![worker_id, task_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((holding_task, title)) = holder {
        return Err(TaskStoreError::WorkerAlreadyHasActiveTask {
            holding_task,
            // Trimmed on purpose. The point is to identify the ticket, not to
            // reproduce it -- a refusal that grows into a report is a different
            // kind of unreadable.
            holding_title: truncate_for_refusal(&title),
        });
    }
    Ok(())
}

/// Reads the state a caller is allowed to transition from. A worker-reported
/// transition must name the exact live session still holding the assignment, so
/// a concurrent worker exit leaves the task unchanged for a later guarded retry.
fn reportable_task_state(
    transaction: &rusqlite::Transaction<'_>,
    id: TaskId,
    reporting_session_id: Option<WorkerSessionId>,
) -> Result<TaskState, TaskStoreError> {
    let current: Option<String> = if let Some(session_id) = reporting_session_id {
        transaction
            .query_row(
                "SELECT task.state FROM tasks task
                 JOIN task_assignments assignment ON assignment.task_id = task.id
                     AND assignment.released_at IS NULL
                 JOIN worker_sessions session ON session.session_id = assignment.worker_session_id
                     AND session.ended_at IS NULL
                 WHERE task.id = ?1 AND task.removed_at IS NULL AND session.session_id = ?2",
                params![id.to_string(), session_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
    } else {
        transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?
    };
    let current = current.ok_or_else(|| {
        if reporting_session_id.is_some() {
            TaskStoreError::WorkerSessionNotActive
        } else {
            TaskStoreError::NotFound
        }
    })?;
    TaskState::from_str(&current).map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
}

fn validate_text(title: &str, workspace: &str) -> Result<(), TaskStoreError> {
    if title.is_empty() || title.len() > MAX_TASK_TITLE_BYTES {
        return Err(TaskStoreError::InvalidTitle);
    }
    if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(())
}

/// One line, so it stays an instruction rather than becoming a second
/// description competing with the first.
fn validate_operator_instruction(instruction: &str) -> Result<(), TaskStoreError> {
    if instruction.len() > MAX_OPERATOR_INSTRUCTION_BYTES || instruction.contains('\n') {
        return Err(TaskStoreError::InvalidOperatorInstruction);
    }
    Ok(())
}

fn validate_description(description: &str) -> Result<(), TaskStoreError> {
    if description.len() > MAX_TASK_DESCRIPTION_BYTES {
        return Err(TaskStoreError::InvalidDescription);
    }
    Ok(())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get(0)?;
    let hive_id: String = row.get(1)?;
    let priority: String = row.get(4)?;
    let state: String = row.get(6)?;
    let assigned_worker_id: Option<String> = row.get(7)?;
    let assigned_session_id: Option<String> = row.get(8)?;
    let has_assignee = assigned_worker_id.is_some();
    let dispatch_state: Option<String> = row.get(9)?;
    let outcome_delivery_state: Option<String> = row.get(10)?;
    Ok(Task {
        id: TaskId::from_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        hive_id: parse_domain_id::<HiveId>(&hive_id)?,
        title: row.get(2)?,
        description: row.get(3)?,
        priority: TaskPriority::from_str(&priority).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        workspace: row.get(5)?,
        state: TaskState::from_str(&state).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        assigned_worker_id: assigned_worker_id
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        assigned_session_id: assigned_session_id
            .map(|value| WorkerSessionId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        dispatch_state: dispatch_state
            .map(|value| TaskDispatchState::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        operator_instruction: row.get(14)?,
        // STRICT, LIKE EVERY OTHER COLUMN HERE. These were lenient, and the
        // leniency could only ever hide a mistake: a projection that forgets one
        // of them returned `false`, which is a plausible value meaning "this task
        // is not in that state". A caller could not tell a missing column from a
        // genuine negative, which is the failure family this repository keeps
        // paying for — an instrument whose broken state reads as a real answer.
        //
        // Nothing legitimate produces a missing column: these are expressions in
        // the projection, not table columns, so they are present whenever the
        // query is right and absent only when it is wrong. Failing loudly turns a
        // silent wrong answer into an error naming the row.
        deployment_recorded: row.get(15)?,
        closed_on_evidence: row.get(16)?,
        worked_here: row.get(17)?,
        closed_unverifiable: row.get(18)?,
        next_move_owner: swarm_domain::NextMoveOwner::derive(
            TaskState::from_str(&state).unwrap_or(TaskState::Draft),
            has_assignee,
            row.get(19)?,
            row.get(20)?,
        ),
        outcome_delivery_state: outcome_delivery_state
            .map(|value| TaskOutcomeDeliveryState::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        position: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn task_activity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskActivity> {
    let kind = TaskActivityKind::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let from_state = row
        .get::<_, Option<String>>(3)?
        .map(|value| TaskState::from_str(&value))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let to_state = row
        .get::<_, Option<String>>(4)?
        .map(|value| TaskState::from_str(&value))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let actor_kind = TaskActivityActorKind::from_str(&row.get::<_, String>(7)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(TaskActivity {
        sequence: row.get(0)?,
        task_id: TaskId::from_str(&row.get::<_, String>(1)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        kind,
        from_state,
        to_state,
        note: row.get(5)?,
        occurred_at: row.get(6)?,
        actor_kind,
        actor_id: row.get(8)?,
    })
}

pub(crate) fn parse_domain_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_task_lifecycle_and_assignment() {
        let store = TaskStore::in_memory().unwrap();
        let created = store.create_task("Fix reload", "/workspace").unwrap();
        assert_eq!(created.state, TaskState::Draft);

        let ready = store.transition_task(created.id, TaskState::Ready).unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let assigned = store.assign_task(ready.id, session_id).unwrap();
        assert_eq!(assigned.assigned_worker_id, Some(worker.id));
        assert_eq!(assigned.assigned_session_id, Some(session_id));
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        store.transition_task(ready.id, TaskState::Active).unwrap();
        store.transition_task(ready.id, TaskState::Review).unwrap();
        let completed = store
            .transition_task(ready.id, TaskState::Completed)
            .unwrap();
        assert_eq!(completed.state, TaskState::Completed);
        assert!(matches!(
            store.assign_task_to_worker(ready.id, worker.id),
            Err(TaskStoreError::CompletedTask)
        ));

        let activity = store.list_task_activity(created.id, 100).unwrap();
        assert!(!activity.truncated);
        assert_eq!(activity.events.len(), 6);
        assert_eq!(activity.events[0].kind, TaskActivityKind::Created);
        assert_eq!(activity.events[1].from_state, Some(TaskState::Draft));
        assert_eq!(activity.events[1].to_state, Some(TaskState::Ready));
        assert_eq!(activity.events[2].kind, TaskActivityKind::Assigned);
        assert_eq!(activity.events[5].to_state, Some(TaskState::Completed));
    }

    /// THE REFUSAL MUST NAME THE TASK HOLDING THE SLOT.
    ///
    /// It used to say only that the worker "already has work in progress". On
    /// 2026-09-01 a worker read that against its own board, saw a ticket in
    /// Review, and concluded Review gates Active. It does not — the gate is
    /// `state = 'active'` alone. That guess was relayed as a first-hand account
    /// of a refusal, believed, and passed to a second worker as a fact about
    /// its own queue. Half an hour went into a rule that did not exist.
    ///
    /// Naming the holder is what makes that guess unnecessary, and the id was
    /// always one column away in the query that raises this.
    /// THE TWO LISTS MUST PARTITION THE BOARD. Nothing may fall between them.
    ///
    /// The board stopped polling settled work, so anything `list_tasks` hides
    /// and `list_settled_tasks` does not return is closed in the database and
    /// nowhere on the screen. That is the same disappearance the board model
    /// warns about where it admits abandoned work to the completed bucket.
    #[test]
    fn the_board_list_and_the_settled_list_partition_every_task() {
        let store = TaskStore::in_memory().unwrap();
        let mut expected = Vec::new();
        for (title, state) in [
            ("still open", TaskState::Ready),
            ("in flight", TaskState::Active),
            ("waiting on somebody", TaskState::Review),
            ("stuck", TaskState::Blocked),
            ("given up on", TaskState::Abandoned),
            ("finished with nothing recorded", TaskState::Completed),
        ] {
            let task = store.create_task(title, "/workspace").unwrap();
            if state != TaskState::Draft {
                for step in [
                    TaskState::Ready,
                    TaskState::Active,
                    TaskState::Review,
                    state,
                ] {
                    if store.transition_task(task.id, step).is_err() {
                        continue;
                    }
                    if store.get_task(task.id).unwrap().state == state {
                        break;
                    }
                }
            }
            expected.push(task.id.to_string());
        }

        let board = store.list_board_tasks().unwrap();
        let settled = store.list_settled_tasks().unwrap();

        let mut seen: Vec<String> = board
            .iter()
            .chain(settled.iter())
            .map(|task| task.id.to_string())
            .collect();
        seen.sort();
        let mut want = expected.clone();
        want.sort();
        assert_eq!(
            seen, want,
            "every task must appear in exactly one of the two lists"
        );

        for task in &board {
            assert!(
                !settled.iter().any(|other| other.id == task.id),
                "a task in both lists would render twice: {}",
                task.title
            );
        }
        assert!(
            settled
                .iter()
                .any(|task| task.state == TaskState::Abandoned),
            "abandoned work owes nothing and belongs in the settled list, not the board's"
        );
        assert!(
            board
                .iter()
                .any(|task| task.title == "finished with nothing recorded"),
            "completed work with NO evidence still owes some, so it stays on the board"
        );
    }

    #[test]
    fn the_busy_refusal_names_the_task_holding_the_slot() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let holder = store
            .create_task("Reconcile the syslog forwarder", "/workspace")
            .unwrap();
        let next = store.create_task("Queued work", "/workspace").unwrap();
        for task in [holder.id, next.id] {
            store.transition_task(task, TaskState::Ready).unwrap();
            store.assign_task_to_worker(task, worker.id).unwrap();
        }
        store
            .transition_worker_task(holder.id, TaskState::Active, "Starting", session_id)
            .unwrap();

        let refused = store
            .transition_worker_task(next.id, TaskState::Active, "Starting", session_id)
            .expect_err("a second active task is refused");
        let message = refused.to_string();

        assert!(
            message.contains(&holder.id.to_string()),
            "the refusal must identify the holder, or the reader guesses: {message}"
        );
        assert!(
            message.contains("Reconcile the syslog forwarder"),
            "and its title, so the id does not have to be looked up: {message}"
        );
        assert_eq!(
            message.lines().count(),
            1,
            "but it stays one line — a refusal that becomes a report is unread: {message}"
        );
    }

    /// A title long enough to bury the identifier is trimmed rather than
    /// printed whole, and trimmed by CHARACTERS: real titles carry em dashes,
    /// and slicing those by byte panics.
    #[test]
    fn a_long_title_is_trimmed_rather_than_printed_whole() {
        let long = "SOA Phase A5b — remove the two dead Connect Sync OU exclusions \
                    that the replication check has been waiting on since August";
        let trimmed = truncate_for_refusal(long);
        assert!(
            trimmed.chars().count() <= 63,
            "a refusal must not grow into a report: {trimmed}"
        );
        assert!(
            trimmed.ends_with("..."),
            "and it says it was cut: {trimmed}"
        );
        assert_eq!(
            truncate_for_refusal("Short title"),
            "Short title",
            "a title that already fits is left alone rather than decorated"
        );
    }

    #[test]
    fn one_worker_cannot_start_two_assigned_tasks() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let first = store.create_task("Current work", "/workspace").unwrap();
        let next = store.create_task("Queued work", "/workspace").unwrap();
        for task in [first.id, next.id] {
            store.transition_task(task, TaskState::Ready).unwrap();
            store.assign_task_to_worker(task, worker.id).unwrap();
        }

        store
            .transition_worker_task(first.id, TaskState::Active, "Starting", session_id)
            .unwrap();
        assert!(matches!(
            store.transition_worker_task(next.id, TaskState::Active, "Starting", session_id),
            Err(TaskStoreError::WorkerAlreadyHasActiveTask { .. })
        ));
        assert_eq!(store.get_task(next.id).unwrap().state, TaskState::Ready);
    }

    #[test]
    fn recent_task_activity_is_bounded_across_the_local_hive() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First task", "/workspace/first").unwrap();
        let second = store
            .create_task("Second task", "/workspace/second")
            .unwrap();
        store.transition_task(first.id, TaskState::Ready).unwrap();
        store.transition_task(second.id, TaskState::Ready).unwrap();

        let recent = store.list_recent_task_activity(3).unwrap();

        assert!(recent.truncated);
        assert_eq!(recent.events.len(), 3);
        assert!(
            recent
                .events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(recent.events.last().unwrap().task_id, second.id);
        assert_eq!(
            recent.events.last().unwrap().to_state,
            Some(TaskState::Ready)
        );
    }

    #[test]
    fn removing_a_task_hides_it_but_retains_its_audit_history() {
        let store = TaskStore::in_memory().unwrap();
        let removable = store
            .create_task("Duplicate Inbox report", "/workspace")
            .unwrap();
        store
            .remove_task_as(removable.id, &TaskActivityActor::operator(), "")
            .unwrap();

        assert!(store.list_tasks().unwrap().is_empty());
        assert!(matches!(
            store.get_task(removable.id),
            Err(TaskStoreError::NotFound)
        ));
        let activity = store.list_task_activity(removable.id, 10).unwrap();
        assert_eq!(
            activity.events.last().unwrap().kind,
            TaskActivityKind::Removed
        );
        assert_eq!(
            activity.events.last().unwrap().actor_kind,
            TaskActivityActorKind::Operator
        );

        let active = store.create_task("Work in progress", "/workspace").unwrap();
        store.transition_task(active.id, TaskState::Ready).unwrap();
        store.transition_task(active.id, TaskState::Active).unwrap();
        assert!(matches!(
            store.remove_task_as(active.id, &TaskActivityActor::operator(), ""),
            Err(TaskStoreError::ActiveTaskCannotBeRemoved)
        ));
        assert_eq!(store.get_task(active.id).unwrap().state, TaskState::Active);
    }

    #[test]
    fn restoring_removed_local_work_returns_it_at_the_end_of_the_queue() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let restored = store.create_task("Restore me", "/workspace").unwrap();
        store
            .remove_task_as(restored.id, &TaskActivityActor::operator(), "")
            .unwrap();

        assert_eq!(store.list_removed_local_tasks().unwrap()[0].id, restored.id);
        let returned = store
            .restore_task_as(restored.id, &TaskActivityActor::operator())
            .unwrap();

        assert!(store.list_removed_local_tasks().unwrap().is_empty());
        assert_eq!(returned.position, first.position + 1);
        assert_eq!(store.list_tasks().unwrap().len(), 2);
        let activity = store.list_task_activity(restored.id, 10).unwrap();
        assert_eq!(
            activity.events.last().unwrap().kind,
            TaskActivityKind::Restored
        );
        assert!(matches!(
            store.restore_task_as(restored.id, &TaskActivityActor::operator()),
            Err(TaskStoreError::NotFound)
        ));
    }

    #[test]
    fn task_activity_preserves_authenticated_actor_provenance() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task_with_details_as(
                "Trace the work",
                "",
                TaskPriority::Normal,
                "/workspace",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        store
            .transition_task_with_note_as(
                task.id,
                TaskState::Ready,
                "Prepared by Daisy",
                &TaskActivityActor::worker(worker.id),
            )
            .unwrap();

        let activity = store.list_task_activity(task.id, 10).unwrap().events;
        assert_eq!(activity[0].actor_kind, TaskActivityActorKind::Operator);
        assert_eq!(activity[0].actor_id, None);
        assert_eq!(activity[1].actor_kind, TaskActivityActorKind::Worker);
        assert_eq!(activity[1].actor_id, Some(worker.id.to_string()));
    }

    #[test]
    fn unassigning_releases_worker_ownership_and_cancels_a_queued_brief() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let task = store.create_task("Return this work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let assigned = store.assign_task_to_worker(task.id, worker.id).unwrap();
        assert_eq!(assigned.dispatch_state, Some(TaskDispatchState::Queued));

        let unassigned = store.unassign_task(task.id).unwrap();

        assert_eq!(unassigned.assigned_worker_id, None);
        assert_eq!(unassigned.assigned_session_id, None);
        assert_eq!(unassigned.dispatch_state, None);
        assert!(
            store
                .claim_task_dispatches(i64::MAX, &std::collections::HashSet::new())
                .unwrap()
                .is_empty()
        );
        let activity = store.list_task_activity(task.id, 100).unwrap();
        assert_eq!(
            activity.events.last().unwrap().kind,
            TaskActivityKind::Unassigned
        );
    }

    #[test]
    fn sleeping_worker_owns_task_and_rebinds_it_after_restart() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Clover",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task("Resume durable work", "/workspace/clover")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();

        let sleeping = store.assign_task_to_worker(task.id, worker.id).unwrap();
        assert_eq!(sleeping.assigned_worker_id, Some(worker.id));
        assert_eq!(sleeping.assigned_session_id, None);
        assert_eq!(sleeping.dispatch_state, None);

        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        let started = store.get_task(task.id).unwrap();
        assert_eq!(started.assigned_worker_id, Some(worker.id));
        assert_eq!(started.assigned_session_id, Some(first));
        assert_eq!(started.dispatch_state, Some(TaskDispatchState::Queued));

        store.release_worker_session(first).unwrap();
        store.release_session_assignments(first).unwrap();
        let stopped = store.get_task(task.id).unwrap();
        assert_eq!(stopped.assigned_worker_id, Some(worker.id));
        assert_eq!(stopped.assigned_session_id, None);

        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        let resumed = store.get_task(task.id).unwrap();
        assert_eq!(resumed.assigned_worker_id, Some(worker.id));
        assert_eq!(resumed.assigned_session_id, Some(second));
        assert_eq!(resumed.dispatch_state, Some(TaskDispatchState::Queued));

        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_dispatches
                 SET state = 'delivered', delivered_at = unixepoch(), updated_at = unixepoch()
                 WHERE task_id = ?1 AND state = 'queued'",
                [task.id.to_string()],
            )
            .unwrap();
        store.release_worker_session(second).unwrap();
        store.release_session_assignments(second).unwrap();
        let third = WorkerSessionId::new();
        store.bind_worker_session(worker.id, third).unwrap();
        let continued = store.get_task(task.id).unwrap();
        assert_eq!(continued.assigned_session_id, Some(third));
        assert_eq!(continued.dispatch_state, None);
    }

    #[test]
    fn task_activity_is_bounded_and_unknown_tasks_fail_closed() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Bound history", "/workspace").unwrap();
        for _ in 0..(MAX_TASK_ACTIVITY_PAGE + 10) {
            store
                .update_task_details(
                    task.id,
                    &TaskDetailsUpdate {
                        description: Some("same durable detail".into()),
                        ..TaskDetailsUpdate::default()
                    },
                )
                .unwrap();
        }

        let activity = store.list_task_activity(task.id, usize::MAX).unwrap();
        assert!(activity.truncated);
        assert_eq!(activity.events.len(), MAX_TASK_ACTIVITY_PAGE);
        assert!(
            activity
                .events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert!(matches!(
            store.list_task_activity(TaskId::new(), 30),
            Err(TaskStoreError::NotFound)
        ));
    }

    /// Queen's lever on what arrives first, because priority is not one.
    ///
    /// Delivery orders by `position` — `deliverable_briefings` orders by
    /// `t.position` and the head-of-line rule by `earlier.position`. Neither
    /// consults `priority` anywhere. Priority travels with the brief so the
    /// worker knows how urgent the work is; it decides nothing about sequence.
    ///
    /// Queen set a task high, watched it sit eight deep behind five normal ones
    /// because she had filed it last, and found only one way to move it: BLOCK
    /// a lower-value task to shorten the queue ahead of it. That works and it
    /// makes the board lie — Blocked means waiting on something else, and the
    /// something else was her.
    #[test]
    fn promoting_a_task_moves_it_to_the_front_without_disturbing_the_rest() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();
        let third = store.create_task("Third", "/workspace").unwrap();

        let promoted = store.promote_open_task(third.id).unwrap();

        assert_eq!(
            promoted.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![third.id, first.id, second.id],
            "the promoted task leads and everything else keeps its relative order"
        );
        assert_eq!(
            promoted
                .iter()
                .map(|task| task.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2],
            "positions stay dense, so the next promote reasons about the same numbers"
        );
    }

    /// Promoting what is already first is a no-op rather than an error.
    ///
    /// Queen should not have to check the order before acting on urgency; that
    /// is the work this exists to save.
    #[test]
    fn promoting_the_task_that_already_leads_changes_nothing() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();

        let promoted = store.promote_open_task(first.id).unwrap();

        assert_eq!(
            promoted.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![first.id, second.id]
        );
    }

    /// NOTHING STARVES, and that is why this is a promote rather than an
    /// ordering rule.
    ///
    /// A queue sorted by priority starves normal work whenever high-priority
    /// work keeps arriving — the failure the brief warned about before asking
    /// for one. Position is a total order that only changes when somebody
    /// deliberately changes it, so promoting one task moves every other task
    /// exactly one place back and none of them can be moved back twice by the
    /// same act. Work filed and forgotten still reaches the front.
    #[test]
    fn a_promoted_task_pushes_others_back_by_one_and_never_further() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();
        let third = store.create_task("Third", "/workspace").unwrap();

        store.promote_open_task(third.id).unwrap();
        let after = store.promote_open_task(second.id).unwrap();

        // Two promotions, and the task nobody promoted has moved back exactly
        // twice — not repeatedly, and not to the end.
        assert_eq!(
            after.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![second.id, third.id, first.id]
        );
    }

    /// A task that is not open cannot be promoted, and says so rather than
    /// silently reordering nothing.
    #[test]
    fn a_completed_task_cannot_be_promoted() {
        let store = TaskStore::in_memory().unwrap();
        let done = store.create_task("Done", "/workspace").unwrap();
        store.create_task("Open", "/workspace").unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(done.id, state).unwrap();
        }

        assert!(matches!(
            store.promote_open_task(done.id),
            Err(TaskStoreError::NotFound)
        ));
    }

    #[test]
    fn open_task_order_is_complete_atomic_and_durable() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();
        let third = store.create_task("Third", "/workspace").unwrap();
        assert_eq!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .map(|task| task.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        let reordered = store
            .reorder_open_tasks(&[third.id, first.id, second.id])
            .unwrap();
        assert_eq!(
            reordered.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![third.id, first.id, second.id]
        );
        assert_eq!(
            reordered
                .iter()
                .map(|task| task.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        assert!(matches!(
            store.reorder_open_tasks(&[first.id, second.id]),
            Err(TaskStoreError::InvalidTaskOrder)
        ));
        assert_eq!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec![third.id, first.id, second.id]
        );
    }

    #[test]
    fn updates_only_supplied_task_details_and_records_activity() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task_with_details(
                "Polish task cards",
                "Make priority visible",
                TaskPriority::High,
                "/workspace",
            )
            .unwrap();
        let updated = store
            .update_task_details(
                task.id,
                &TaskDetailsUpdate {
                    title: Some("Polish the task board".into()),
                    priority: Some(TaskPriority::Urgent),
                    ..TaskDetailsUpdate::default()
                },
            )
            .unwrap();

        assert_eq!(updated.title, "Polish the task board");
        assert_eq!(updated.description, "Make priority visible");
        assert_eq!(updated.priority, TaskPriority::Urgent);
        assert_eq!(updated.workspace, "/workspace");
        assert!(matches!(
            store.update_task_details(task.id, &TaskDetailsUpdate::default()),
            Err(TaskStoreError::EmptyTaskDetailsUpdate)
        ));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM task_activity WHERE task_id = ?1 AND kind = 'details_updated'",
                    [task.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn stopping_a_session_releases_its_assignments() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Assigned work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let worker = store
            .create_worker(
                "Poppy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        store.assign_task(task.id, session_id).unwrap();

        assert_eq!(store.release_session_assignments(session_id).unwrap(), 1);
        let stopped = store.get_task(task.id).unwrap();
        assert_eq!(stopped.assigned_worker_id, Some(worker.id));
        assert_eq!(stopped.assigned_session_id, None);
        assert_eq!(store.release_session_assignments(session_id).unwrap(), 0);
    }

    #[test]
    fn rejects_skipped_transitions_and_invalid_content() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.create_task("", "/workspace"),
            Err(TaskStoreError::InvalidTitle)
        ));
        let task = store.create_task("A task", "/workspace").unwrap();
        assert!(matches!(
            store.transition_task(task.id, TaskState::Completed),
            Err(TaskStoreError::InvalidTransition { .. })
        ));
    }

    /// A refusal that names the event, the actor and the time.
    ///
    /// "task cannot move from completed to completed" is true and names
    /// nothing about what happened. What happened is that something closed the
    /// task seconds earlier on evidence someone else recorded, and a reader
    /// handed the rule goes looking for a lifecycle bug instead. It did that
    /// twice on 2026-08-25, and it is the sixth instance of one shape: a
    /// message that is accurate and names the wrong precondition.
    #[test]
    fn completing_an_already_completed_task_says_who_closed_it_and_when() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Restore the schema", "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        store
            .transition_task(task.id, TaskState::Completed)
            .unwrap();

        let refusal = store
            .transition_task(task.id, TaskState::Completed)
            .expect_err("a second completion must still be refused");

        let message = refusal.to_string();
        // The three things a reader needs, none of which they used to get.
        assert!(message.contains("already completed"), "{message}");
        assert!(message.contains("s ago by"), "{message}");
        // And what to do with what they were carrying, rather than silence.
        assert!(message.contains("swarm_record_deployment"), "{message}");
        // It must not read as a failure. Nothing is broken here.
        assert!(message.contains("closed rather than blocked"), "{message}");
    }

    /// ORDINARY same-state refusals are left alone.
    ///
    /// ready to ready is somebody making a mistake, not losing a race, and the
    /// plain rule is the right thing to tell them. Dressing every same-state
    /// refusal up as a collision would bury the one case that actually is one.
    #[test]
    fn an_ordinary_same_state_refusal_still_states_the_plain_rule() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("A task", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();

        assert!(matches!(
            store.transition_task(task.id, TaskState::Ready),
            Err(TaskStoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn reopens_file_database_without_losing_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let id = {
            let store = TaskStore::open(&path).unwrap();
            store
                .create_task("Persistent task", "/workspace")
                .unwrap()
                .id
        };
        let reopened = TaskStore::open(path).unwrap();
        assert_eq!(reopened.get_task(id).unwrap().title, "Persistent task");
    }

    #[test]
    fn migrates_the_task_only_schema_to_the_worker_roster() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                PRAGMA user_version = 1;
                ",
            )
            .unwrap();
        let store = TaskStore::from_connection(connection).unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        assert_eq!(queen.role, swarm_domain::WorkerRole::Queen);
        let columns = store
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"description".to_owned()));
        assert!(columns.contains(&"priority".to_owned()));
        let worker_columns = store
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(worker_profiles)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(worker_columns.contains(&"provider_conversation_id".to_owned()));
        assert!(worker_columns.contains(&"description".to_owned()));
        assert!(worker_columns.contains(&"archived_at".to_owned()));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn reopens_current_schema_without_replacing_hive_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn jira_transition_delivery_migration_restores_its_missing_table() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let mut connection = store.connection().unwrap();
            let transaction = connection.transaction().unwrap();
            transaction
                .execute_batch("DROP TABLE jira_transition_deliveries;")
                .unwrap();
            // This is a focused migration-step fixture, not a historical v21
            // database: every other table is current. Do not rerun unrelated
            // migrations against their already-present artifacts.
            migrate_jira_transition_deliveries(&transaction).unwrap();
            transaction
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .unwrap();
            transaction.commit().unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'jira_transition_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v22_to_durable_jira_comment_deliveries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE jira_comment_deliveries;
                     PRAGMA user_version = 22;",
                )
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'jira_comment_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v23_to_opt_in_assigned_jira_sync() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE jira_project_bindings (
                     id TEXT PRIMARY KEY,
                     project_name TEXT NOT NULL
                 );
                 INSERT INTO jira_project_bindings (id, project_name)
                 VALUES ('binding-1', 'Website Services');
                 PRAGMA user_version = 23;",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_schema(&transaction, 23).unwrap();
        transaction.commit().unwrap();

        assert_eq!(
            connection
                .query_row(
                    "SELECT auto_sync_assigned FROM jira_project_bindings WHERE id = 'binding-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrates_v10_decisions_to_the_guarded_delivery_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TABLE decision_deliveries; PRAGMA user_version = 10;")
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decision_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_v3_tasks_and_workers_into_one_durable_hive() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    description TEXT NOT NULL DEFAULT '',
                    priority TEXT NOT NULL DEFAULT 'normal'
                );
                CREATE TABLE task_assignments (
                    id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    worker_session_id TEXT NOT NULL,
                    assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    released_at INTEGER
                );
                CREATE TABLE task_activity (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL,
                    from_state TEXT,
                    to_state TEXT,
                    occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_profiles (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    role TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    autostart INTEGER NOT NULL,
                    position INTEGER NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_sessions (
                    session_id TEXT PRIMARY KEY,
                    worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
                    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    ended_at INTEGER
                );
                INSERT INTO tasks (id, title, workspace, state)
                    VALUES ('018f0000-0000-7000-8000-000000000001', 'Existing task', '/repo', 'ready');
                INSERT INTO worker_profiles
                    (id, name, role, provider, workspace, autostart, position)
                    VALUES ('018f0000-0000-7000-8000-000000000002', 'Existing worker', 'worker', 'claude_code', '/repo', 0, 1);
                PRAGMA user_version = 3;
                ",
            )
            .unwrap();

        let store = TaskStore::from_connection(connection).unwrap();
        let identity = store.local_hive_identity().unwrap();
        assert_eq!(store.list_tasks().unwrap()[0].hive_id, identity.hive.id);
        assert_eq!(
            store.list_worker_profiles().unwrap()[0].hive_id,
            identity.hive.id
        );
        store.verify_integrity().unwrap();
    }

    #[test]
    fn hive_ownership_and_apiary_backend_constraints_fail_closed() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let connection = store.connection().unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO tasks (id, title, workspace, state, description, priority)
                     VALUES (?1, 'Orphan', '/repo', 'draft', '', 'normal')",
                    [TaskId::new().to_string()],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO worker_profiles
                     (id, name, role, provider, workspace, autostart, position)
                     VALUES (?1, 'Orphan', 'worker', 'claude_code', '/repo', 0, 1)",
                    [swarm_domain::WorkerId::new().to_string()],
                )
                .is_err()
        );

        let apiary_id = ApiaryId::new();
        connection
            .execute(
                "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                 VALUES (?1, 'Test Apiary', ?2, 'jira')",
                params![apiary_id.to_string(), identity.operator.id.to_string()],
            )
            .unwrap();
        assert!(
            connection
                .execute(
                    "UPDATE apiaries SET shared_work_backend = 'native' WHERE id = ?1",
                    [apiary_id.to_string()],
                )
                .is_err()
        );
    }

    #[test]
    fn local_apiary_context_is_personal_until_durable_membership_exists() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(
            store.local_apiary_context().unwrap(),
            LocalApiaryContext::Personal
        );

        let identity = store.local_hive_identity().unwrap();
        let apiary_id = ApiaryId::new();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                     VALUES (?1, 'Garden', ?2, 'jira')",
                    params![apiary_id.to_string(), identity.operator.id.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE hives SET apiary_id = ?1 WHERE id = ?2",
                    params![apiary_id.to_string(), identity.hive.id.to_string()],
                )
                .unwrap();
        }

        assert!(matches!(
            store.local_apiary_context().unwrap(),
            LocalApiaryContext::Federated {
                apiary,
                local_role: LocalApiaryRole::Keeper,
            } if apiary.id == apiary_id && apiary.shared_work_backend() == SharedWorkBackend::Jira
        ));
    }

    #[test]
    fn apiary_member_roster_is_role_oriented_and_excludes_personal_hives() {
        let personal = TaskStore::in_memory().unwrap();
        assert!(matches!(
            personal.list_apiary_members(),
            Err(TaskStoreError::InvalidApiary)
        ));

        let store = TaskStore::in_memory().unwrap();
        let keeper = store.local_hive_identity().unwrap();
        let context = store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected federated context");
        };
        let member_operator_id = OperatorId::new();
        let member_hive_id = HiveId::new();
        {
            let connection = store.connection().unwrap();
            insert_test_operator(&connection, member_operator_id, "Cora");
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id, apiary_id)
                     VALUES (?1, 'Clover Hive', ?2, ?3)",
                    params![
                        member_hive_id.to_string(),
                        member_operator_id.to_string(),
                        apiary.id.to_string()
                    ],
                )
                .unwrap();
        }

        let members = store.list_apiary_members().unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].hive_id, keeper.hive.id);
        assert_eq!(members[0].role, LocalApiaryRole::Keeper);
        assert!(members[0].is_local);
        assert_eq!(members[1].hive_id, member_hive_id);
        assert_eq!(members[1].operator_display_name, "Cora");
        assert_eq!(members[1].role, LocalApiaryRole::Member);
        assert!(!members[1].is_local);
    }

    fn insert_test_operator(connection: &Connection, operator_id: OperatorId, name: &str) {
        connection
            .execute(
                "INSERT INTO operators (id, display_name) VALUES (?1, ?2)",
                params![operator_id.to_string(), name],
            )
            .unwrap();
    }

    #[test]
    fn stewardship_scope_is_explicit_durable_and_apiary_bounded() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let apiary_id = ApiaryId::new();
        let steward_operator_id = OperatorId::new();
        let stewardship_id = StewardshipId::new();
        let managed_hive_id = HiveId::new();
        let outside_operator_id = OperatorId::new();
        let outside_hive_id = HiveId::new();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                     VALUES (?1, 'Garden', ?2, 'jira')",
                    params![apiary_id.to_string(), identity.operator.id.to_string()],
                )
                .unwrap();
            insert_test_operator(&connection, steward_operator_id, "Steward");
            insert_test_operator(&connection, outside_operator_id, "Outside");
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id, apiary_id)
                     VALUES (?1, 'Managed Hive', ?2, ?3)",
                    params![
                        managed_hive_id.to_string(),
                        steward_operator_id.to_string(),
                        apiary_id.to_string()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id)
                     VALUES (?1, 'Outside Hive', ?2)",
                    params![outside_hive_id.to_string(), outside_operator_id.to_string()],
                )
                .unwrap();
            assert!(
                connection
                    .execute(
                        "INSERT INTO stewardships
                            (id, apiary_id, steward_operator_id, created_by_operator_id)
                         VALUES (?1, ?2, ?3, ?3)",
                        params![
                            StewardshipId::new().to_string(),
                            apiary_id.to_string(),
                            steward_operator_id.to_string()
                        ],
                    )
                    .is_err()
            );
            connection
                .execute(
                    "INSERT INTO stewardships
                        (id, apiary_id, steward_operator_id, created_by_operator_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        stewardship_id.to_string(),
                        apiary_id.to_string(),
                        steward_operator_id.to_string(),
                        identity.operator.id.to_string()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO stewardship_hive_grants (stewardship_id, hive_id)
                     VALUES (?1, ?2)",
                    params![stewardship_id.to_string(), managed_hive_id.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO stewardship_capability_grants (stewardship_id, capability)
                     VALUES (?1, 'observe'), (?1, 'takeover')",
                    [stewardship_id.to_string()],
                )
                .unwrap();
            assert!(
                connection
                    .execute(
                        "INSERT INTO stewardship_hive_grants (stewardship_id, hive_id)
                         VALUES (?1, ?2)",
                        params![stewardship_id.to_string(), outside_hive_id.to_string()],
                    )
                    .is_err()
            );
        }

        assert_eq!(
            store.stewardships_for_apiary(apiary_id).unwrap(),
            vec![Stewardship {
                id: stewardship_id,
                apiary_id,
                steward_operator_id,
                managed_hive_ids: vec![managed_hive_id],
                capabilities: vec![StewardCapability::Observe, StewardCapability::Takeover],
            }]
        );
    }

    #[test]
    fn migrates_schema_v24_to_explicit_stewardship_grants() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TRIGGER stewardship_hive_scope_update;
                     DROP TRIGGER stewardship_hive_scope_insert;
                     DROP TABLE stewardship_capability_grants;
                     DROP TABLE stewardship_hive_grants;
                     DROP TABLE stewardships;
                     PRAGMA user_version = 24;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in [
            "stewardships",
            "stewardship_hive_grants",
            "stewardship_capability_grants",
        ] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn backup_preflight_rejects_missing_empty_unrelated_and_corrupt_files_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("candidate.sqlite3");
        assert!(verify_existing_hive_backup(&path).is_err());
        assert!(
            !path.exists(),
            "verification must not create its own evidence"
        );
        for bytes in [b"".as_slice(), b"not a SQLite database".as_slice()] {
            std::fs::write(&path, bytes).unwrap();
            assert!(verify_existing_hive_backup(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let unrelated = directory.path().join("unrelated.sqlite3");
        let connection = Connection::open(&unrelated).unwrap();
        connection
            .execute_batch("CREATE TABLE notes(body TEXT); PRAGMA user_version=1;")
            .unwrap();
        drop(connection);
        let before = std::fs::read(&unrelated).unwrap();
        assert!(verify_existing_hive_backup(&unrelated).is_err());
        assert_eq!(std::fs::read(&unrelated).unwrap(), before);
    }

    #[test]
    fn backup_is_consistent_and_reopenable() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.sqlite3");
        let backup = directory.path().join("backups").join("snapshot.sqlite3");
        let store = TaskStore::open(source).unwrap();
        let task = store.create_task("Backed up", "/workspace").unwrap();
        store.backup_to(&backup).unwrap();

        let before = std::fs::read(&backup).unwrap();
        verify_existing_hive_backup(&backup).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), before);

        let truncated = directory.path().join("truncated.sqlite3");
        let damaged = &before[..before.len() / 2];
        std::fs::write(&truncated, damaged).unwrap();
        assert!(verify_existing_hive_backup(&truncated).is_err());
        assert_eq!(std::fs::read(&truncated).unwrap(), damaged);

        let truncated = directory.path().join("truncated.sqlite3");
        let damaged = &before[..before.len() / 2];
        std::fs::write(&truncated, damaged).unwrap();
        assert!(verify_existing_hive_backup(&truncated).is_err());
        assert_eq!(std::fs::read(&truncated).unwrap(), damaged);

        let restored = TaskStore::open(backup).unwrap();
        restored.verify_integrity().unwrap();
        assert_eq!(restored.get_task(task.id).unwrap().title, "Backed up");
    }

    #[test]
    fn task_and_worker_mutations_emit_typed_content_free_events() {
        let store = TaskStore::in_memory().unwrap();
        assert!(store.list_control_room_events(0).unwrap().events.is_empty());

        let task = store.create_task("Secret task text", "/workspace").unwrap();
        let worker = store
            .create_worker(
                "Private worker name",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, swarm_domain::WorkerSessionId::new())
            .unwrap();

        let page = store.list_control_room_events(0).unwrap();
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                ControlRoomEventKind::TasksChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::SessionsChanged,
            ]
        );
        assert!(
            page.events
                .iter()
                .all(|event| event.hive_id == task.hive_id)
        );
        let serialized = serde_json::to_string(&page).unwrap();
        assert!(!serialized.contains("Secret task text"));
        assert!(!serialized.contains("Private worker name"));
    }

    #[test]
    fn control_room_event_log_is_bounded_and_stale_cursors_reset() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
            .unwrap();
        for _ in 0..=MAX_CONTROL_ROOM_EVENTS {
            store
                .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
                .unwrap();
        }

        let connection = store.connection().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM control_room_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            MAX_CONTROL_ROOM_EVENTS
        );
        drop(connection);

        let stale = store.list_control_room_events(first.sequence).unwrap();
        assert!(stale.reset_required);
        assert_eq!(stale.events.len(), MAX_CONTROL_ROOM_EVENT_PAGE);
        let future = store.list_control_room_events(i64::MAX).unwrap();
        assert!(future.reset_required);
    }

    #[test]
    fn migrates_schema_v4_without_losing_existing_hive_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let (task_id, hive_id) = {
            let store = TaskStore::open(&path).unwrap();
            let task = store.create_task("Existing v4 task", "/workspace").unwrap();
            let hive_id = store.local_hive_identity().unwrap().hive.id;
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP INDEX tasks_by_hive_position;
                     ALTER TABLE tasks DROP COLUMN position;
                     DROP TABLE worker_engagements;
                     ALTER TABLE worker_profiles DROP COLUMN provider_conversation_id;
                     DROP TABLE control_room_events;
                     PRAGMA user_version = 4;",
                )
                .unwrap();
            (task.id, hive_id)
        };

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(migrated.get_task(task_id).unwrap().hive_id, hive_id);
        assert_eq!(migrated.get_task(task_id).unwrap().position, 0);
        assert!(
            migrated
                .list_control_room_events(0)
                .unwrap()
                .events
                .is_empty()
        );
        migrated.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v6_without_assigning_ambiguous_existing_conversations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let worker_id = {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker(
                    "Existing worker",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace",
                    false,
                    1,
                )
                .unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            store.release_worker_session(session).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE worker_engagements;
                     ALTER TABLE worker_profiles DROP COLUMN provider_conversation_id;
                     PRAGMA user_version = 6;",
                )
                .unwrap();
            worker.id
        };

        let migrated = TaskStore::open(path).unwrap();
        let worker = migrated.get_worker_profile(worker_id).unwrap();
        assert!(worker.has_session_history);
        assert_eq!(worker.provider_conversation_id, None);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrates_schema_v7_to_bounded_worker_engagements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE worker_engagements; PRAGMA user_version = 7;")
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let tables = migrated
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'worker_engagements'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }

    #[test]
    fn migrates_schema_v11_to_durable_task_dispatches() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE task_dispatches; PRAGMA user_version = 11;")
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let tables = migrated
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'task_dispatches'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v12_to_task_handoff_notes_and_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE task_outcome_deliveries;
                     ALTER TABLE task_activity DROP COLUMN note;
                     PRAGMA user_version = 12;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'task_outcome_deliveries'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('task_activity') WHERE name = 'note'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v13_to_bounded_operator_presence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE operator_presence_devices;
                     DROP TABLE operator_presence_preferences;
                     PRAGMA user_version = 13;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in ["operator_presence_preferences", "operator_presence_devices"] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v14_to_bounded_mobile_attention() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE notification_deliveries;
                     DROP TABLE notification_subscriptions;
                     DROP TABLE notification_preferences;
                     DROP TABLE notification_vapid_keys;
                     PRAGMA user_version = 14;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in [
            "notification_vapid_keys",
            "notification_preferences",
            "notification_subscriptions",
            "notification_deliveries",
        ] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        drop(connection);
        migrated.verify_integrity().unwrap();
    }
    #[test]
    fn migrates_schema_v15_to_device_owned_engagements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "ALTER TABLE worker_engagements DROP COLUMN owner_device_id;
                     PRAGMA user_version = 15;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(worker_engagements)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.iter().any(|column| column == "owner_device_id"));
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v16_to_queen_autonomy_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE queen_autonomy_preferences;
                     PRAGMA user_version = 16;",
                )
                .unwrap();
        }
        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated.queen_autonomy_policy().unwrap(),
            swarm_domain::QueenAutonomyPolicy::default()
        );
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v17_to_device_presentation_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE presentation_preferences;
                     PRAGMA user_version = 17;",
                )
                .unwrap();
        }
        let migrated = TaskStore::open(path).unwrap();
        assert!(
            !migrated
                .presentation_preferences(PresentationDeviceClass::Desktop)
                .unwrap()
                .configured
        );
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn fresh_store_owns_tasks_and_workers_in_one_durable_hive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let (hive_id, operator_id) = {
            let store = TaskStore::open(&path).unwrap();
            let identity = store.local_hive_identity().unwrap();
            assert_eq!(identity.operator.display_name, "Operator");
            assert_eq!(identity.hive.name, "My Hive");
            assert_eq!(identity.hive.operator_id, identity.operator.id);

            let task = store.create_task("Hive-owned task", "/workspace").unwrap();
            let worker = store
                .create_worker(
                    "Violet",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace",
                    false,
                    1,
                )
                .unwrap();
            assert_eq!(task.hive_id, identity.hive.id);
            assert_eq!(worker.hive_id, identity.hive.id);
            (identity.hive.id, identity.operator.id)
        };

        let reopened = TaskStore::open(path).unwrap();
        let identity = reopened.local_hive_identity().unwrap();
        assert_eq!(identity.hive.id, hive_id);
        assert_eq!(identity.operator.id, operator_id);
        assert_eq!(reopened.list_tasks().unwrap()[0].hive_id, hive_id);
        assert_eq!(reopened.list_worker_profiles().unwrap()[0].hive_id, hive_id);
    }

    #[test]
    fn current_schema_requires_hive_ownership_columns() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        for table in ["tasks", "worker_profiles"] {
            let sql =
                format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'hive_id'");
            assert_eq!(
                connection
                    .query_row(&sql, [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn migrates_schema_v43_to_durable_task_activity_actors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let task_id = {
            let store = TaskStore::open(&path).unwrap();
            let task = store
                .create_task("Existing activity", "/workspace")
                .unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "ALTER TABLE task_activity DROP COLUMN actor_id;
                     ALTER TABLE task_activity DROP COLUMN actor_kind;
                     PRAGMA user_version = 43;",
                )
                .unwrap();
            task.id
        };

        let migrated = TaskStore::open(path).unwrap();
        let columns = migrated
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(task_activity)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"actor_kind".to_owned()));
        assert!(columns.contains(&"actor_id".to_owned()));
        let existing = migrated.list_task_activity(task_id, 10).unwrap();
        assert_eq!(existing.events[0].actor_kind, TaskActivityActorKind::System);
        assert_eq!(existing.events[0].actor_id, None);
        migrated.verify_integrity().unwrap();
    }

    /// One named migration step, described well enough for a test to undo it
    /// and check it comes back.
    ///
    /// Adding a migration means adding a line here rather than rewriting the
    /// test below. Editing that test by hand has caught four real mistakes and
    /// cost four edits; the check is worth keeping and the rewriting is not.
    struct SchemaStep {
        table: &'static str,
        /// The column the step adds, or empty when the step adds the table.
        artifact: &'static str,
        /// For a step that changes something which is neither a table nor a
        /// column — a trigger, say — the SQL that models a database one
        /// version short, and the SQL that detects the change. Both empty when
        /// the table and artifact above already describe the step.
        undo_sql: &'static str,
        probe_sql: &'static str,
    }

    impl SchemaStep {
        /// SQL that removes this step's artifact, to model a database that
        /// never ran it.
        fn undo(&self) -> String {
            if !self.undo_sql.is_empty() {
                return self.undo_sql.to_owned();
            }
            if self.artifact.is_empty() {
                format!("DROP TABLE {}", self.table)
            } else {
                format!("ALTER TABLE {} DROP COLUMN {}", self.table, self.artifact)
            }
        }

        /// SQL returning whether this step's artifact is present.
        fn probe(&self) -> String {
            if !self.probe_sql.is_empty() {
                return self.probe_sql.to_owned();
            }
            if self.artifact.is_empty() {
                format!(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master
                     WHERE type = 'table' AND name = '{}')",
                    self.table
                )
            } else {
                format!(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{}')
                     WHERE name = '{}')",
                    self.table, self.artifact
                )
            }
        }
    }

    /// Recent named steps, oldest first. The last is the ceiling.
    const RECENT_SCHEMA_STEPS: &[SchemaStep] = &[
        SchemaStep {
            table: "queen_automation",
            artifact: "delivery_session_id",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "operator_presence_devices",
            artifact: "last_active_at",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "tasks",
            artifact: "operator_instruction",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "worker_revival_intents",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "decision_requests",
            artifact: "resolution_surface",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "decision_requests",
            artifact: "questions",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "decision_requests",
            artifact: "summary",
            undo_sql: "",
            probe_sql: "",
        },
        // A trigger rather than a table or a column: the undo restores the
        // narrower rule so the migration has something real to widen.
        SchemaStep {
            table: "email_reply_deliveries",
            artifact: "the widened evidence rule",
            undo_sql: "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
                 CREATE TRIGGER email_reply_requires_completed_deployment
                     BEFORE INSERT ON email_reply_deliveries
                     WHEN NOT EXISTS (
                         SELECT 1 FROM tasks task
                         JOIN task_deployments deployment ON deployment.task_id = task.id
                         WHERE task.id = NEW.task_id AND task.state = 'completed'
                     )
                     BEGIN SELECT RAISE(ABORT, 'Email replies require completed deployed work'); END",
            // The widened rule now lives on the SEND trigger: 108 renamed this
            // step's artifact when it moved the evidence test off the draft.
            // Probing the old name here asserted the end state of a chain that
            // no longer ends there, and said so as "did not survive".
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'trigger'
                   AND name = 'email_reply_send_requires_evidence'
                   AND sql LIKE '%review%')",
        },
        // 108: the evidence rule guards the send rather than the draft. The undo
        // puts the single INSERT-time trigger back, so the migration has the
        // real old shape to move rather than a stand-in.
        SchemaStep {
            table: "email_reply_deliveries",
            artifact: "the evidence rule moved to the send",
            undo_sql: "DROP TRIGGER IF EXISTS email_reply_draft_requires_finished_work;
                 DROP TRIGGER IF EXISTS email_reply_send_requires_evidence;
                 DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
                 CREATE TRIGGER email_reply_requires_completed_deployment
                     BEFORE INSERT ON email_reply_deliveries
                     WHEN NOT EXISTS (
                         SELECT 1 FROM tasks task
                         WHERE task.id = NEW.task_id
                           AND task.state IN ('completed', 'review')
                           AND EXISTS (SELECT 1 FROM task_deployments deployment
                                        WHERE deployment.task_id = task.id)
                     )
                     BEGIN SELECT RAISE(ABORT, 'Email replies require deployed work in review or completed'); END",
            probe_sql: "SELECT (SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger'
                   AND name IN ('email_reply_draft_requires_finished_work',
                                'email_reply_send_requires_evidence')) = 2
                 AND NOT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'trigger' AND name = 'email_reply_requires_completed_deployment')",
        },
        // A widened CHECK on an existing column. The undo restores the narrower
        // one by rebuilding the table, which is the only way SQLite changes a
        // constraint.
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%worker_filed_draft_attention%')",
        },
        SchemaStep {
            table: "operator_preferences",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "release_check_preferences",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "task_completion_exemptions",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            // The artifact is a value inside a CHECK, not a column, so the
            // default "drop the column" undo cannot express it. Rebuild the
            // table at the shape before this step instead.
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%decision_deadline_passed_attention%')",
        },
        SchemaStep {
            table: "coordinator_refusals",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "operator_passkeys",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "terminal_geometry_events",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            // A value inside a CHECK again, so the undo rebuilds the table at
            // the shape before this step rather than dropping a column.
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%owned_work_never_briefed_attention%')",
        },
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            // Another value inside a CHECK, so the undo rebuilds the table at
            // the v89 shape. Every kind that existed before this step is
            // carried across; dropping one would silently void live rows.
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%reviewed_work_without_evidence_attention%')",
        },
        SchemaStep {
            table: "task_review_holds",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "worker_sessions",
            artifact: "ended_reason",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "email_reply_deliveries",
            artifact: "the approved-exemption clause",
            undo_sql: "DROP TRIGGER IF EXISTS email_reply_requires_completed_deployment;
                 CREATE TRIGGER email_reply_requires_completed_deployment
                     BEFORE INSERT ON email_reply_deliveries
                     WHEN NOT EXISTS (
                         SELECT 1 FROM tasks task
                         JOIN task_deployments deployment ON deployment.task_id = task.id
                         WHERE task.id = NEW.task_id AND task.state IN ('completed', 'review')
                     )
                     BEGIN SELECT RAISE(ABORT, 'Email replies require deployed work in review or completed'); END",
            // Same rename as the step above: 108 moved the evidence test off the
            // draft, so the exemption clause this step added now lives on the
            // send trigger. The old name is gone by the end of the chain.
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'trigger' AND name = 'email_reply_send_requires_evidence'
                   AND sql LIKE '%task_completion_exemptions%')",
        },
        // A table REBUILD rather than an added column, so the undo restores the
        // decision-only shape in full: the narrow kind CHECK, the constraint
        // tying every non-test row to a decision, and the decision-keyed
        // UNIQUE. Dropping the column alone would not model a pre-94 database,
        // and SQLite would refuse anyway while a UNIQUE still names it.
        SchemaStep {
            table: "notification_deliveries",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS notification_deliveries;
                 CREATE TABLE notification_deliveries (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
                     subscription_id TEXT NOT NULL
                         REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
                     decision_id TEXT REFERENCES decision_requests(id) ON DELETE CASCADE,
                     urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
                     kind TEXT NOT NULL CHECK (kind IN ('decision','test')),
                     state TEXT NOT NULL CHECK (state IN ('queued','dispatching')),
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
                     available_at INTEGER NOT NULL,
                     created_at INTEGER NOT NULL,
                     CHECK ((kind = 'decision' AND decision_id IS NOT NULL)
                            OR (kind = 'test' AND decision_id IS NULL)),
                     UNIQUE(decision_id, subscription_id)
                 )",
            probe_sql: "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('notification_deliveries')
                 WHERE name = 'subject_key'
             )",
        },
        SchemaStep {
            table: "operator_attention_watermarks",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "task_completion_exemptions",
            artifact: "superseded_at",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "worker_profiles",
            artifact: "",
            // Puts the closed provider list back, by the same rebuild in
            // reverse. A DROP-and-recreate would be shorter and wrong:
            // seventeen tables carry foreign keys into this one and dropping it
            // would cascade them away, so rewinding the schema would delete the
            // operator's data rather than model an older database.
            // foreign_keys OFF for the same reason migrate_open_provider_set
            // needs it: this rebuild drops a table that seventeen others point
            // at, and DROP TABLE's implicit delete refuses while a
            // non-cascading child row exists. This batch runs outside a
            // transaction, so the pragma takes effect here.
            undo_sql: "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE worker_profiles RENAME TO worker_profiles_undo;
                 CREATE TABLE worker_profiles (
                     id TEXT PRIMARY KEY,
                     name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                     role TEXT NOT NULL CHECK (role IN ('queen','worker')),
                     provider TEXT NOT NULL CHECK (provider IN ('claude_code','codex')),
                     workspace TEXT NOT NULL,
                     autostart INTEGER NOT NULL CHECK (autostart IN (0,1)),
                     position INTEGER NOT NULL,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     hive_id TEXT REFERENCES hives(id),
                     provider_conversation_id TEXT,
                     description TEXT NOT NULL DEFAULT '',
                     archived_at INTEGER,
                     system_role TEXT CHECK (system_role IS NULL OR system_role = 'scout'),
                     provider_conversation_resume INTEGER NOT NULL DEFAULT 0
                         CHECK (provider_conversation_resume IN (0, 1))
                 );
                 INSERT INTO worker_profiles SELECT * FROM worker_profiles_undo;
                 DROP TABLE worker_profiles_undo;
                 CREATE UNIQUE INDEX one_queen_profile
                     ON worker_profiles(role) WHERE role = 'queen';
                 CREATE INDEX worker_profiles_by_hive ON worker_profiles(hive_id);
                 CREATE INDEX worker_profiles_active_roster
                     ON worker_profiles(role, position, created_at, id)
                     WHERE archived_at IS NULL;
                 CREATE UNIQUE INDEX one_scout_per_hive
                     ON worker_profiles(hive_id)
                     WHERE system_role = 'scout' AND archived_at IS NULL;
                 PRAGMA legacy_alter_table = OFF;
                 PRAGMA foreign_keys = ON;",
            probe_sql: "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'worker_profiles'
                   AND sql NOT LIKE '%provider IN (%'
             )",
        },
        SchemaStep {
            table: "worker_profiles",
            artifact: "ephemeral",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "task_amendments",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "decision_command_grants",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            // Rewinds the CHECK to the set before this kind existed, by the same
            // rebuild the migration runs forward. Not a DROP: coordinator_actions
            // carries foreign keys and dropping it would cascade.
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 INSERT INTO coordinator_actions SELECT * FROM coordinator_actions_undo
                     WHERE kind != 'blocked_work_unattended_attention';
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF;
                 PRAGMA foreign_keys = ON;",
            probe_sql: "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%blocked_work_unattended_attention%'
             )",
        },
        // Adds no column and no table: it backfills rows. undo_sql therefore
        // removes what the backfill wrote rather than reshaping anything, and
        // the probe asks whether any amendment is still missing from the trail.
        SchemaStep {
            table: "tasks",
            artifact: "blocked_until",
            undo_sql: "ALTER TABLE tasks DROP COLUMN blocked_until;",
            probe_sql: "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'blocked_until'
             )",
        },
        SchemaStep {
            table: "worker_profiles",
            artifact: "mark",
            undo_sql: "",
            probe_sql: "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('worker_profiles') WHERE name = 'mark'
             )",
        },
        SchemaStep {
            table: "task_activity",
            artifact: "amended",
            undo_sql: "DELETE FROM task_activity WHERE kind = 'amended';",
            probe_sql: "SELECT NOT EXISTS(
                 SELECT 1 FROM task_amendments amendment
                 WHERE NOT EXISTS (
                     SELECT 1 FROM task_activity existing
                     WHERE existing.task_id = amendment.task_id
                       AND existing.kind = 'amended'
                       AND existing.occurred_at = amendment.created_at
                 )
             )",
        },
        // A CHECK CONSTRAINT IS NEITHER A TABLE NOR A COLUMN, so this step
        // carries its own undo: the artifact is a value the column will accept,
        // and the only way to model a database that never ran the step is to
        // rebuild `tasks` with the CHECK it used to have.
        SchemaStep {
            table: "tasks",
            artifact: "",
            undo_sql: "DROP INDEX IF EXISTS tasks_by_hive;
                 DROP INDEX IF EXISTS tasks_by_hive_position;
                 DROP INDEX IF EXISTS task_owner_queue;
                 DROP INDEX IF EXISTS tasks_visible_queue;
                 DROP TRIGGER IF EXISTS tasks_require_hive_insert;
                 DROP TRIGGER IF EXISTS tasks_require_hive_update;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE tasks RENAME TO tasks_undo;
                 CREATE TABLE tasks (
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     workspace TEXT NOT NULL,
                     state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed')),
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     description TEXT NOT NULL DEFAULT '',
                     priority TEXT NOT NULL DEFAULT 'normal'
                         CHECK (priority IN ('low','normal','high','urgent')),
                     hive_id TEXT REFERENCES hives(id),
                     position INTEGER NOT NULL DEFAULT 0,
                     assigned_worker_id TEXT REFERENCES worker_profiles(id),
                     removed_at INTEGER,
                     operator_instruction TEXT NOT NULL DEFAULT '',
                     blocked_until INTEGER
                 );
                 INSERT INTO tasks
                     (id, title, workspace, state, created_at, updated_at, description,
                      priority, hive_id, position, assigned_worker_id, removed_at,
                      operator_instruction, blocked_until)
                 SELECT id, title, workspace, state, created_at, updated_at, description,
                        priority, hive_id, position, assigned_worker_id, removed_at,
                        operator_instruction, blocked_until
                   FROM tasks_undo WHERE state <> 'abandoned';
                 DROP TABLE tasks_undo;
                 CREATE INDEX tasks_by_hive ON tasks(hive_id);
                 CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
                 CREATE INDEX task_owner_queue
                     ON tasks(assigned_worker_id, state)
                     WHERE assigned_worker_id IS NOT NULL AND state != 'completed';
                 CREATE INDEX tasks_visible_queue
                     ON tasks(hive_id, state) WHERE removed_at IS NULL;
                 CREATE TRIGGER tasks_require_hive_insert
                     BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
                     BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
                 CREATE TRIGGER tasks_require_hive_update
                     BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
                     BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
                 PRAGMA legacy_alter_table = OFF;",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'tasks' AND sql LIKE '%abandoned%')",
        },
        // TWO TABLES IN ONE STEP, so the defaults cannot express it: the
        // generated undo drops a single named table and would leave the other
        // standing, which migrates forward into a half-applied step that still
        // probes green.
        SchemaStep {
            table: "task_commit_reports",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS task_commits;
                 DROP TABLE IF EXISTS task_commit_reports;",
            probe_sql: "SELECT (SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('task_commit_reports','task_commits')) = 2",
        },
        // A CHECK again, so again its own undo: the artifact is a value the
        // column accepts, which no generated ALTER or DROP can model.
        SchemaStep {
            table: "task_completion_exemptions",
            artifact: "",
            undo_sql: "PRAGMA legacy_alter_table = ON;
                 ALTER TABLE task_completion_exemptions RENAME TO task_completion_exemptions_undo;
                 CREATE TABLE task_completion_exemptions (
                     task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
                     reason TEXT NOT NULL,
                     claimed_by_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
                     claimed_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     approved_at INTEGER,
                     approved_by TEXT
                         CHECK (approved_by IS NULL OR approved_by IN ('queen','operator')),
                     superseded_at INTEGER
                 );
                 INSERT INTO task_completion_exemptions
                     (task_id, reason, claimed_by_worker_id, claimed_at,
                      approved_at, approved_by, superseded_at)
                 SELECT task_id, reason, claimed_by_worker_id, claimed_at,
                        approved_at, approved_by, superseded_at
                   FROM task_completion_exemptions_undo
                  WHERE approved_by IS NULL OR approved_by <> 'coordinator';
                 DROP TABLE task_completion_exemptions_undo;
                 PRAGMA legacy_alter_table = OFF;",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'task_completion_exemptions'
                   AND sql LIKE '%coordinator%')",
        },
        // 113. A value inside a CHECK again, so the undo rebuilds the table at
        // the shape before this step — every kind through
        // blocked_work_unattended_attention and not the one this adds.
        SchemaStep {
            table: "coordinator_actions",
            artifact: "",
            undo_sql: "DROP INDEX IF EXISTS coordinator_actions_queue;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE coordinator_actions RENAME TO coordinator_actions_undo;
                 CREATE TABLE coordinator_actions (
                     id TEXT PRIMARY KEY,
                     idempotency_key TEXT NOT NULL UNIQUE,
                     kind TEXT NOT NULL CHECK (kind IN ('wake_assigned_worker','stale_owned_work_attention','owned_work_worker_exited_attention','assigned_ready_work_not_started_attention','worker_filed_draft_attention','decision_deadline_passed_attention','owned_work_never_briefed_attention','reviewed_work_without_evidence_attention','blocked_work_unattended_attention')),
                     worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
                     task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                     session_id TEXT,
                     evidence_revision INTEGER,
                     observed_age_seconds INTEGER,
                     state TEXT NOT NULL CHECK (state IN ('queued','running','completed','uncertain','cancelled')),
                     reason TEXT NOT NULL,
                     attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 1),
                     attempted_at INTEGER,
                     finished_at INTEGER,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 DROP TABLE coordinator_actions_undo;
                 CREATE INDEX coordinator_actions_queue
                     ON coordinator_actions(state, created_at, id);
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'coordinator_actions'
                   AND sql LIKE '%evidenced_work_not_closed_attention%')",
        },
        // 114. The tasks CHECK gains a state, so the undo rebuilds tasks at the
        // shape before it — every state through 'abandoned' and not this one.
        SchemaStep {
            table: "tasks",
            artifact: "",
            undo_sql: "DROP INDEX IF EXISTS tasks_by_hive;
                 DROP INDEX IF EXISTS tasks_by_hive_position;
                 DROP INDEX IF EXISTS task_owner_queue;
                 DROP INDEX IF EXISTS tasks_visible_queue;
                 DROP TRIGGER IF EXISTS tasks_require_hive_insert;
                 DROP TRIGGER IF EXISTS tasks_require_hive_update;
                 PRAGMA legacy_alter_table = ON;
                 ALTER TABLE tasks RENAME TO tasks_undo;
                 CREATE TABLE tasks (
                     id TEXT PRIMARY KEY,
                     title TEXT NOT NULL,
                     workspace TEXT NOT NULL,
                     state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed','abandoned')),
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     description TEXT NOT NULL DEFAULT '',
                     priority TEXT NOT NULL DEFAULT 'normal'
                         CHECK (priority IN ('low','normal','high','urgent')),
                     hive_id TEXT REFERENCES hives(id),
                     position INTEGER NOT NULL DEFAULT 0,
                     assigned_worker_id TEXT REFERENCES worker_profiles(id),
                     removed_at INTEGER,
                     operator_instruction TEXT NOT NULL DEFAULT '',
                     blocked_until INTEGER
                 );
                 INSERT INTO tasks
                     (id, title, workspace, state, created_at, updated_at, description,
                      priority, hive_id, position, assigned_worker_id, removed_at,
                      operator_instruction, blocked_until)
                 SELECT id, title, workspace, state, created_at, updated_at, description,
                        priority, hive_id, position, assigned_worker_id, removed_at,
                        operator_instruction, blocked_until
                   FROM tasks_undo;
                 DROP TABLE tasks_undo;
                 CREATE INDEX tasks_by_hive ON tasks(hive_id);
                 CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
                 CREATE INDEX task_owner_queue
                     ON tasks(assigned_worker_id, state)
                     WHERE assigned_worker_id IS NOT NULL
                       AND state NOT IN ('completed','abandoned');
                 CREATE INDEX tasks_visible_queue
                     ON tasks(hive_id, state) WHERE removed_at IS NULL;
                 CREATE TRIGGER tasks_require_hive_insert
                     BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
                     BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
                 CREATE TRIGGER tasks_require_hive_update
                     BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
                     BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
                 PRAGMA legacy_alter_table = OFF",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'tasks'
                   AND sql LIKE '%awaiting_release%')",
        },
        // 115. A whole table this time, so the undo simply removes it.
        SchemaStep {
            table: "task_returned_reviews",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS task_returned_reviews",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'task_returned_reviews')",
        },
        // 116. A whole table again, so the undo simply removes it.
        SchemaStep {
            table: "task_messages",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS task_messages",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'task_messages')",
        },
        // 117. A plain added column, so the default undo drops it.
        SchemaStep {
            table: "task_completion_exemptions",
            artifact: "approved_basis",
            undo_sql: "",
            probe_sql: "",
        },
        // 118. A plain added column, so the default undo drops it.
        SchemaStep {
            table: "task_deployments",
            artifact: "delivers_whole_task",
            undo_sql: "",
            probe_sql: "",
        },
        // 119. New tables, so the undo drops them.
        SchemaStep {
            table: "operator_broadcasts",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS operator_broadcast_deliveries;
                       DROP TABLE IF EXISTS operator_broadcasts",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'operator_broadcasts')",
        },
        // 120. TWO columns, so the undo names both. The default undo drops
        // only the artifact, which left expiry_reason behind and made the
        // re-migration fail on a duplicate column.
        SchemaStep {
            table: "operator_broadcast_deliveries",
            artifact: "expired_at",
            undo_sql: "ALTER TABLE operator_broadcast_deliveries DROP COLUMN expiry_reason;
                       ALTER TABLE operator_broadcast_deliveries DROP COLUMN expired_at",
            probe_sql: "",
        },
        // 121. A plain added column, so the default undo drops it.
        SchemaStep {
            table: "task_messages",
            artifact: "delivered_session_id",
            undo_sql: "",
            probe_sql: "",
        },
        // 122. A plain added column, so the default undo drops it.
        SchemaStep {
            table: "worker_sessions",
            artifact: "last_coordination_delivery_at",
            undo_sql: "",
            probe_sql: "",
        },
        // 123. THE COLUMN CANNOT GO BACK ALONE. `migrate_claim_withdrawal`
        // recreates email_reply_send_requires_evidence to read withdrawn_at, and
        // SQLite checks a trigger body against the table it fires on: dropping
        // the column out from under it makes the next write fail with "error in
        // trigger ... after drop column", which is what two migration tests said
        // when the default undo was left in place here.
        //
        // So the undo puts the schema-108 trigger back first and drops the
        // column second. That is what a database which never ran 123 actually
        // looks like, which is the whole point of these steps.
        SchemaStep {
            table: "task_completion_exemptions",
            artifact: "withdrawn_at",
            undo_sql: "DROP TRIGGER IF EXISTS email_reply_send_requires_evidence;
                 CREATE TRIGGER email_reply_send_requires_evidence
                     BEFORE UPDATE OF state ON email_reply_deliveries
                     WHEN NEW.state = 'queued' AND OLD.state <> 'queued'
                      AND NOT EXISTS (
                         SELECT 1 FROM tasks task
                         WHERE task.id = NEW.task_id
                           AND task.state IN ('completed', 'review')
                           AND (
                               EXISTS (SELECT 1 FROM task_deployments deployment
                                        WHERE deployment.task_id = task.id)
                               OR EXISTS (SELECT 1 FROM task_completion_exemptions exemption
                                           WHERE exemption.task_id = task.id
                                             AND exemption.approved_at IS NOT NULL)
                           )
                     )
                     BEGIN SELECT RAISE(ABORT, 'An email reply cannot be sent without a recorded deployment or an approved no-deployment exemption'); END;
                 ALTER TABLE task_completion_exemptions DROP COLUMN withdrawn_at",
            probe_sql: "",
        },
        // 124. The first maturity migration remains at its published number.
        SchemaStep {
            table: "worker_terminal_control",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        // 125-134. These entries make the migration ledger match every
        // published maturity schema instead of jumping from 124 to 135.
        SchemaStep {
            table: "operator_night_watch",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "browser_evidence",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "worker_startup_context",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "worker_startup_context",
            artifact: "selection_revision",
            undo_sql: "ALTER TABLE worker_startup_context DROP COLUMN selection_suspended;
                       ALTER TABLE worker_startup_context DROP COLUMN selection_revision",
            probe_sql: "SELECT (SELECT COUNT(*) FROM pragma_table_info('worker_startup_context')
                 WHERE name IN ('selection_revision','selection_suspended')) = 2",
        },
        SchemaStep {
            table: "task_dispatches",
            artifact: "generation",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "operator_statements",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "operator_statement_resolutions",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "operator_submissions",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        SchemaStep {
            table: "task_returned_reviews",
            artifact: "request_message_id",
            undo_sql: "DROP TABLE IF EXISTS task_message_deliveries;
                       ALTER TABLE task_returned_reviews DROP COLUMN answer_message_id;
                       ALTER TABLE task_returned_reviews DROP COLUMN request_worker_id;
                       ALTER TABLE task_returned_reviews DROP COLUMN request_message_id",
            probe_sql: "SELECT (SELECT COUNT(*) FROM pragma_table_info('task_returned_reviews')
                 WHERE name IN ('request_message_id','request_worker_id','answer_message_id')) = 3",
        },
        SchemaStep {
            table: "task_message_deliveries",
            artifact: "",
            undo_sql: "",
            probe_sql: "",
        },
        // 135 after integration. Additive provenance preserves existing task
        // and activity rows; 124-134 belong to the maturity branch migrations.
        SchemaStep {
            table: "ops_console_tickets",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS ops_console_tickets",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'ops_console_tickets')",
        },
        // 136 repairs the published schema-124 collision. The same table is
        // intentionally named twice: 124 introduced it for maturity databases;
        // 136 guarantees it for upstream databases that had already claimed 124.
        SchemaStep {
            table: "worker_terminal_control",
            artifact: "",
            undo_sql: "DROP TABLE IF EXISTS worker_terminal_control",
            probe_sql: "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'worker_terminal_control')",
        },
    ];

    /// The step that introduced a named artifact, rather than whichever is newest.
    ///
    /// A test about ONE migration must undo THAT migration. Using `newest_step`
    /// couples it to whatever lands next: adding schema 102 made the amendment
    /// backfill test undo the `blocked_until` column instead, so it migrated a
    /// database that had never lost its amendment rows and asserted against a
    /// backfill that never ran. It failed for a reason with nothing to do with
    /// amendments.
    ///
    /// Third instance of this shape tonight, after a rollback test that used
    /// "gemini" as its example of an unknown provider and a compatibility test
    /// that tied `PROTOCOL_VERSION - 1` to a fixed payload. Pin to the thing
    /// the test is about, never to a position in a list that grows.
    fn step_for(artifact: &str) -> &'static SchemaStep {
        RECENT_SCHEMA_STEPS
            .iter()
            .find(|step| step.artifact == artifact)
            .expect("the named migration step is declared")
    }

    fn newest_step() -> &'static SchemaStep {
        RECENT_SCHEMA_STEPS
            .last()
            .expect("the migration chain has a newest step")
    }

    #[test]
    fn the_newest_recorded_step_is_the_declared_ceiling() {
        // A migration added without a line in RECENT_SCHEMA_STEPS would leave
        // the test below exercising the step before it, silently.
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        let present: bool = connection
            .query_row(&newest_step().probe(), [], |row| row.get(0))
            .unwrap();
        assert!(present, "the newest step listed is not in the schema");
    }

    /// The ceiling migration survives a database that HAS DATA IN IT.
    ///
    /// This exists because the test below it passed while schema 96 was broken.
    /// That one opens an empty store, so the table rebuild had no rows to copy
    /// and, more importantly, no CHILD ROWS pointing at the table it drops. On
    /// the operator's real database the same migration failed immediately:
    /// with foreign keys enforced, DROP TABLE runs an implicit DELETE FROM, and
    /// every child row referencing the parent trips it.
    ///
    /// Seventeen tables carry foreign keys into `worker_profiles`, so a rebuild
    /// with no children present tests almost nothing about a rebuild. One bound
    /// session is enough to make the difference between the two outcomes.
    /// An amendment corrects the record without erasing what it corrects.
    ///
    /// The defect this closes is an asymmetry, not an absence: a task could
    /// already be corrected in a NOTE, which is subordinate to the description
    /// it corrects. So the false claim sat in the authoritative place and its
    /// correction sat three screens below. A correction system whose corrections
    /// carry less standing than the thing they correct reliably loses.
    ///
    /// APPEND ONLY, and the assertion that matters is the second amendment: a
    /// revision is another amendment, never an edit, so what a worker was told
    /// when it picked the task up can still be reconstructed exactly. That is
    /// the property immutability was protecting, kept without immutability.
    #[test]
    fn an_amendment_is_attributed_appended_and_never_replaces_the_original() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Swarm Next",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/swarm-next",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task_with_details(
                "Add a provider",
                "NO schema migration is needed.",
                TaskPriority::Normal,
                "/workspace/swarm-next",
            )
            .unwrap();

        let first = store
            .amend_task_facts(task.id, worker.id, "False: the column carries a CHECK.")
            .unwrap();
        assert_eq!(first.author_worker_id, worker.id);
        assert_eq!(
            first.author_name, "Swarm Next",
            "an amendment names its author"
        );

        // The original is untouched. This is what stops an amendment being an
        // edit: the description a worker was briefed with is still there.
        assert_eq!(
            store.get_task(task.id).unwrap().description,
            "NO schema migration is needed.",
            "the original text survives its own correction"
        );

        // A second thought is another amendment, not a revision of the first.
        store
            .amend_task_facts(task.id, worker.id, "Schema 96 has since removed it.")
            .unwrap();
        let amendments = store.task_amendments(task.id).unwrap();
        assert_eq!(
            amendments.len(),
            2,
            "corrections accumulate rather than replace"
        );
        assert_eq!(amendments[0].body, "False: the column carries a CHECK.");
        assert_eq!(amendments[1].body, "Schema 96 has since removed it.");

        // Unattributed amendment of governing text would be worse than the stale
        // text it corrects, so an unknown author is refused outright.
        assert!(matches!(
            store.amend_task_facts(task.id, WorkerId::new(), "who wrote this"),
            Err(TaskStoreError::WorkerNotFound)
        ));
        assert!(matches!(
            store.amend_task_facts(task.id, worker.id, "   "),
            Err(TaskStoreError::InvalidTaskActivityNote)
        ));
    }

    #[test]
    fn migrates_the_previous_schema_when_the_database_carries_related_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let worker_id;
        {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker(
                    "Petal",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace/petal",
                    false,
                    1,
                )
                .unwrap();
            worker_id = worker.id;
            // Child rows in the tables that reference this one. A CASCADING
            // child is not enough: DROP TABLE's implicit delete cascades it away
            // harmlessly. tasks.assigned_worker_id has NO on-delete action, so
            // it is the shape that actually refuses the drop -- and the shape
            // the operator's database was full of.
            let session = swarm_domain::WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            let task = store
                .create_task("Carry something", "/workspace/petal")
                .unwrap();
            store.assign_task(task.id, session).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(&format!(
                    "{};
                     PRAGMA user_version = {};",
                    newest_step().undo(),
                    CURRENT_SCHEMA_VERSION - 1
                ))
                .unwrap();
        }

        let migrated = TaskStore::open(&path).expect("a populated database migrates");
        migrated
            .verify_integrity()
            .expect("the migration leaves the database verifiable");
        assert_eq!(
            migrated.get_worker_profile(worker_id).unwrap().name,
            "Petal",
            "the worker survives the rebuild"
        );
        let dangling: Option<String> = migrated
            .connection()
            .unwrap()
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(dangling, None, "no child is left pointing at nothing");
    }

    /// The backfill is exercised against a database that HAS amendments.
    ///
    /// This is the shape migration 96 got wrong: forty schema tests passed
    /// against empty databases while it was broken on the operator's real data.
    /// The generic step probe here is vacuously true when no amendment exists,
    /// so it cannot be the thing that establishes the backfill works.
    ///
    /// Two properties, and the second is the one that fails silently. The rows
    /// must arrive carrying the amendment's OWN `created_at`, because two
    /// attention flags read that column as a clock and migration-time stamps
    /// would drag every historical amendment to now. And the step must be safe
    /// to run twice, because the harness rewinds `user_version` WITHOUT
    /// rewinding tables -- a duplicate does not raise, it just makes every task
    /// look freshly touched.
    /// The block deadline is read from a MARKED line, and only a marked line.
    ///
    /// The stakes are asymmetric and the parser is built for that: a deadline
    /// read where none was meant SUPPRESSES an escalation silently, while
    /// failing to read one merely escalates something the operator could have
    /// been spared. So anything ambiguous yields None.
    #[test]
    fn a_block_deadline_is_read_only_from_its_own_marked_line() {
        // The epoch itself, then a date this Hive will actually see.
        assert_eq!(
            parse_block_deadline("Blocked until: 1970-01-01T00:00:00Z"),
            Some(0)
        );
        assert_eq!(
            parse_block_deadline("Waiting on the window.\nBlocked until: 2026-08-27T17:35:33Z"),
            Some(1_787_852_133)
        );

        // A note that MENTIONS a time is not a note that names its deadline.
        // This is the case that decides whether the operator hears about a
        // stalled task, so it is not inferred from a stray timestamp.
        assert_eq!(
            parse_block_deadline("The window opened at 2026-08-27T17:35:33Z and we missed it"),
            None
        );
        assert_eq!(parse_block_deadline("Blocked on Queen deciding"), None);
        assert_eq!(parse_block_deadline(""), None);

        // Refused rather than half-understood. An offset silently dropped would
        // move a deadline by hours in whichever direction nobody checked.
        assert_eq!(
            parse_block_deadline("Blocked until: 2026-08-27T17:35:33+02:00"),
            None
        );
        assert_eq!(parse_block_deadline("Blocked until: 2026-08-27"), None);
        assert_eq!(parse_block_deadline("Blocked until: tomorrow"), None);
        assert_eq!(
            parse_block_deadline("Blocked until: 2026-13-01T00:00:00Z"),
            None
        );
        assert_eq!(
            parse_block_deadline("Blocked until: 2026-08-27T24:00:00Z"),
            None
        );
    }

    /// A deadline belongs to the block that named it and dies with it.
    ///
    /// A stale deadline left behind by an earlier block would suppress the NEXT
    /// escalation silently, which is the worst way for this to fail: the task
    /// goes quiet and nothing says why.
    #[test]
    fn a_block_deadline_is_cleared_when_the_task_leaves_blocked() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Wait for the window", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .transition_task_with_note(
                task.id,
                TaskState::Blocked,
                "Zero-traffic window.\nBlocked until: 2026-08-27T17:35:33Z",
            )
            .unwrap();

        let deadline = |store: &TaskStore| -> Option<i64> {
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT blocked_until FROM tasks WHERE id = ?1",
                    [task.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(deadline(&store), Some(1_787_852_133));

        store.transition_task(task.id, TaskState::Active).unwrap();
        assert_eq!(
            deadline(&store),
            None,
            "a deadline must not outlive the block that named it"
        );

        // Blocked again with no deadline: the previous one must not come back.
        store
            .transition_task_with_note(task.id, TaskState::Blocked, "Blocked on Queen deciding")
            .unwrap();
        assert_eq!(deadline(&store), None);
    }

    #[test]
    fn the_amendment_backfill_carries_original_timestamps_and_survives_a_rerun() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let (task_id, worker_id) = {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker(
                    "Yarrow",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace/yarrow",
                    false,
                    1,
                )
                .unwrap();
            let task = store
                .create_task("Carry the facts", "/workspace/yarrow")
                .unwrap();
            for body in ["First finding.", "Second finding."] {
                store.amend_task_facts(task.id, worker.id, body).unwrap();
            }
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "UPDATE task_amendments SET created_at = 4242
                     WHERE body = 'First finding.';
                     UPDATE task_amendments SET created_at = 5353
                     WHERE body = 'Second finding.';",
                )
                .unwrap();
            // Model the pre-101 database: the amendments exist and the trail
            // does not know about them.
            connection
                .execute_batch(&format!(
                    "{};
                     PRAGMA user_version = {};",
                    step_for("amended").undo(),
                    AMENDMENT_ACTIVITY_SCHEMA_VERSION - 1
                ))
                .unwrap();
            (task.id, worker.id)
        };

        let migrated = TaskStore::open(&path).expect("a database with amendments migrates");
        let stamps: Vec<i64> = migrated
            .connection()
            .unwrap()
            .prepare(
                "SELECT occurred_at FROM task_activity
                 WHERE task_id = ?1 AND kind = 'amended' ORDER BY occurred_at",
            )
            .unwrap()
            .query_map([task_id.to_string()], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            stamps,
            vec![4242, 5353],
            "backfilled rows carry the amendment's own created_at, not migration time"
        );

        // The trail is what a reader reads, so assert through that rather than
        // through the table the migration wrote.
        let trail = migrated.list_task_activity(task_id, 100).unwrap();
        let amended: Vec<_> = trail
            .events
            .iter()
            .filter(|event| event.kind == TaskActivityKind::Amended)
            .collect();
        assert_eq!(
            amended.len(),
            2,
            "both amendments reach the rendered history"
        );
        assert!(
            amended
                .iter()
                .all(|event| event.actor_id.as_deref() == Some(worker_id.to_string().as_str())),
            "an amendment stays attributable to the worker that wrote it"
        );

        // Rewind the version WITHOUT rewinding the rows, which is exactly what
        // the harness does, and migrate again.
        migrated
            .connection()
            .unwrap()
            .execute_batch(&format!(
                "PRAGMA user_version = {};",
                AMENDMENT_ACTIVITY_SCHEMA_VERSION - 1
            ))
            .unwrap();
        drop(migrated);
        let rerun = TaskStore::open(&path).expect("the step is safe to run twice");
        let after: i64 = rerun
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_activity WHERE task_id = ?1 AND kind = 'amended'",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, 2, "a re-run must not duplicate the backfilled rows");
    }

    #[test]
    fn migrates_the_immediately_previous_schema_to_the_declared_ceiling() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                // Models a database that genuinely stopped one version short:
                // it carries everything the steps below the ceiling created and
                // lacks only what the newest one adds. Dropping older columns
                // here instead would describe a database that never existed,
                // and would pass or fail for reasons unrelated to the ceiling.
                .execute_batch(&format!(
                    "{};
                     PRAGMA user_version = {};",
                    newest_step().undo(),
                    CURRENT_SCHEMA_VERSION - 1
                ))
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        let removed_at_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks')
                 WHERE name = 'removed_at')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(removed_at_exists);
        let provider_resume_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('worker_profiles')
                 WHERE name = 'provider_conversation_resume')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(provider_resume_exists);
        let replacement_provenance_exists: bool = connection
            .query_row(
                "SELECT COUNT(*) = 6 FROM pragma_table_info('migration_worker_links')
                 WHERE name IN (
                    'adopted_existing', 'previous_provider_conversation_id',
                    'previous_provider_conversation_resume', 'previous_updated_at',
                    'imported_provider_conversation_id', 'previous_session_count'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(replacement_provenance_exists);
        // Carried by the step below the ceiling, so a database one version
        // short already has it.
        let delivery_session_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('queen_automation')
                 WHERE name = 'delivery_session_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(delivery_session_exists);
        // Every recent step's artifact, the newest of which is the one this
        // test dropped and expects the migration to put back.
        for step in RECENT_SCHEMA_STEPS {
            let restored: bool = connection
                .query_row(&step.probe(), [], |row| row.get(0))
                .unwrap();
            assert!(
                restored,
                "{} on {} did not survive the migration",
                step.artifact, step.table
            );
        }
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        drop(connection);
        migrated.verify_integrity().unwrap();
    }

    #[test]
    fn repairs_terminal_control_when_upstream_schema_124_was_already_recorded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.sqlite3");
        let worker_id;
        {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker(
                    "Collision Canary",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace/collision-canary",
                    false,
                    1,
                )
                .unwrap();
            worker_id = worker.id;
            let connection = store.connection().unwrap();
            connection
                .execute_batch(&format!(
                    "DROP TABLE worker_terminal_control;
                     PRAGMA user_version = {OPS_TICKETS_SCHEMA_VERSION};"
                ))
                .unwrap();
        }

        let migrated = TaskStore::open(&path).expect("the collided schema is repaired");
        let connection = migrated.connection().unwrap();
        let terminal_control_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'worker_terminal_control')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(terminal_control_exists);
        let ops_tickets_survived: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'ops_console_tickets')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ops_tickets_survived);
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            TERMINAL_CONTROL_PROJECTION_REPAIR_SCHEMA_VERSION
        );
        drop(connection);
        assert_eq!(
            migrated.get_worker_profile(worker_id).unwrap().name,
            "Collision Canary"
        );
        migrated.verify_integrity().unwrap();
    }

    /// The operator asked for one choice used everywhere, because a phone
    /// landing somewhere a desktop would not is the problem. Storing it per
    /// device class would have built that problem into the schema.
    /// "A Hive never contacts an origin its owner did not choose." An
    /// unanswered question is not a choice, so it is stored as neither.
    #[test]
    fn a_hive_does_not_check_for_releases_until_it_is_told_to() {
        let store = TaskStore::in_memory().unwrap();

        assert_eq!(
            store.release_check_state().unwrap(),
            ReleaseCheckState::default()
        );
        assert_eq!(store.release_check_state().unwrap().mode, "unset");

        assert_eq!(store.set_release_check_mode("daily").unwrap().mode, "daily");
        assert_eq!(store.set_release_check_mode("off").unwrap().mode, "off");
        assert!(store.set_release_check_mode("hourly").is_err());
        assert_eq!(store.release_check_state().unwrap().mode, "off");
    }

    /// An origin that is unreachable today does not make yesterday's answer
    /// untrue. Blanking the offer would read as "nothing available".
    #[test]
    fn a_failed_check_records_the_failure_without_erasing_what_was_offered() {
        let store = TaskStore::in_memory().unwrap();
        store.set_release_check_mode("daily").unwrap();

        let offered = store
            .record_release_check("offered", Some(r#"{"version":"0.2.0"}"#), 1_000)
            .unwrap();
        assert_eq!(offered.last_outcome.as_deref(), Some("offered"));
        assert_eq!(
            offered.last_offer.as_deref(),
            Some(r#"{"version":"0.2.0"}"#)
        );

        let failed = store
            .record_release_check("unreachable", None, 2_000)
            .unwrap();
        assert_eq!(failed.last_outcome.as_deref(), Some("unreachable"));
        assert_eq!(failed.last_checked_at, Some(2_000));
        assert_eq!(failed.last_offer.as_deref(), Some(r#"{"version":"0.2.0"}"#));
        assert_eq!(failed.mode, "daily");

        assert!(store.record_release_check("shrugged", None, 3_000).is_err());
    }

    #[test]
    fn the_screen_swarm_opens_on_is_one_choice_for_every_device() {
        let store = TaskStore::in_memory().unwrap();

        // A default that is a real answer rather than an empty one.
        assert_eq!(store.start_surface().unwrap(), "tasks");

        assert_eq!(store.set_start_surface("workers").unwrap(), "workers");
        assert_eq!(store.start_surface().unwrap(), "workers");

        // Choosing again replaces the choice rather than accumulating one.
        assert_eq!(store.set_start_surface("decisions").unwrap(), "decisions");
        assert_eq!(store.start_surface().unwrap(), "decisions");

        // Only screens this product actually opens on.
        assert!(store.set_start_surface("elsewhere").is_err());
        assert_eq!(store.start_surface().unwrap(), "decisions");
    }
}

/// ONE PROJECTION, AND THIS IS WHAT KEEPS IT ONE.
///
/// There were three byte-identical copies feeding `task_from_row`, and every
/// column added had to be added to all three. The failure mode was not the
/// duplication itself but what a MISS cost: `row.get` was `unwrap_or(false)`, so
/// a copy that forgot a column returned `false` — a plausible value meaning "this
/// task is not in that state" — and no caller could tell that from a genuine
/// negative.
///
/// Both halves are fixed: the copies are gone, and the reads are strict so a
/// fourth that forgets one errors instead of lying. This test guards the first
/// half, because a future reader in a hurry writes a new SELECT rather than
/// reusing a constant, and nothing else would notice.
#[cfg(test)]
mod the_task_projection_stays_singular {
    const SOURCE: &str = include_str!("lib.rs");

    /// Split so the scanner does not match its own needle — this module is
    /// inside the file it reads, and written as one literal it counted itself.
    /// That mistake was made twice in one day before it was written down.
    const PROJECTION_HEAD: &str = concat!("SELECT t.id,", " t.hive_id");

    #[test]
    fn there_is_exactly_one_task_projection() {
        let copies = SOURCE.matches(PROJECTION_HEAD).count();
        assert_eq!(
            copies, 1,
            "the task projection appears {copies} times. Every column has to be \
             added to each one, and a copy that forgets one is not an error — it \
             reads as a genuine negative. Use TaskStore::TASK_PROJECTION and add \
             only a WHERE."
        );
    }

    /// The lenient read is what made a missed column silent rather than loud.
    #[test]
    fn the_projection_columns_are_read_strictly() {
        let lenient = SOURCE.matches(concat!("row.get(", "15).unwrap_or")).count()
            + SOURCE.matches(concat!("row.get(", "19).unwrap_or")).count()
            + SOURCE.matches(concat!("row.get(", "20).unwrap_or")).count();
        assert_eq!(
            lenient, 0,
            "a task projection column is read with unwrap_or again. A missing \
             column then reads as `false`, which is indistinguishable from the \
             task genuinely not being in that state."
        );
    }
}
