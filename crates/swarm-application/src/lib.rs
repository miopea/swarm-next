/// The question this check raises, and the action that answers it.
///
/// Shared constants rather than three string literals, because the offer, the
/// execution and the loop guard must agree exactly. They did not: the action was
/// offered in two places and executed in none.
const PARKED_WORK_QUESTION_TITLE: &str = "Work left parked by an answer";
const RELEASE_PARKED_WORK_ACTION: &str = "Release them to the queue";

use swarm_domain::{
    Apiary, ApiaryCollapseReadiness, ApiaryHiveCandidate, ApiaryInvitation, ApiaryInvitationBundle,
    ApiaryInvitationId, ApiaryJiraProject, ApiaryJoinCheckState, ApiaryJoinChecks, ApiaryJoinLink,
    ApiaryJoinLinkBundle, ApiaryJoinLinkId, ApiaryJoinLinkPoll, ApiaryJoinReadiness,
    ApiaryKeeperLink, ApiaryMemberSummary, ApiaryTask, DecisionQuestion, DecisionRequest,
    DecisionRequestId, DecisionRequestKind, DecisionUrgency, FederationCatalogAcknowledgement,
    FederationCatalogReadiness, FederationCatalogSnapshot, FederationClaimHandoff,
    FederationClaimHandoffId, FederationClaimId, FederationDepartureOverview,
    FederationDepartureReadiness, FederationDepartureReceipt, FederationJoinAcceptance,
    FederationJoinInvitation, FederationJoinReadiness, FederationJoinSubmission,
    FederationMemberConnection, FederationNodeId, FederationSharedClaim,
    FederationStewardAssistCommand, FederationStewardAssistCommandId, FederationStewardAssistInbox,
    FederationStewardAssistLocalState, FederationStewardAssistOutboxEntry,
    FederationStewardAssistReceipt, FederationStewardAssistRequestId, FederationStewardAssistState,
    FederationStewardTakeoverCommand, FederationStewardTakeoverCommandId,
    FederationStewardTakeoverInbox, FederationStewardTakeoverLeaseId,
    FederationStewardTakeoverLocalState, FederationStewardTakeoverOutboxEntry,
    FederationStewardTakeoverReceipt, FederationStewardTakeoverRelayAuthorization,
    FederationStewardTaskAuditEntry, FederationStewardTaskCommand, FederationStewardTaskCommandId,
    FederationStewardTaskOutboxEntry, FederationStewardTaskReceipt, FederationStewardshipSnapshot,
    FederationSyncCondition, FederationSyncHealth, FederationTaskCommand, FederationTaskCommandId,
    FederationTaskCommandReceipt, FederationTaskOutboxEntry, FederationTaskOutboxStatus,
    FederationTaskPage, FederationTaskSyncStatus, HiveConnectionCard, HiveId, JiraConnectionState,
    JiraProjectBindingId, LocalApiaryContext, LocalApiaryRole, LocalApiaryTaskExecution,
    OperatorId, OperatorPresence, PresenceDeviceClass, PresenceDeviceId, PresenceMode,
    PresenceObservationState, SharedWorkBackend, StewardCapability, Stewardship, StewardshipId,
    Task, TaskActivityActor, TaskId, TaskPriority, TaskState, WorkerId, WorkerProfile, WorkerRole,
    WorkerSessionId,
};
use swarm_persistence::{NewDecisionRequest, TaskStore, TaskStoreError};
use thiserror::Error;

mod decision_clarification;
mod workspace_settings;
pub use workspace_settings::WorkspaceSettingsService;
#[cfg(test)]
mod enrollment_tests;
mod native_operator_sources;
pub use native_operator_sources::{NativeAnswerLink, NativeSourceAdmission, NativeSourceReceipt};
mod ops_tickets;
mod queen_review;
mod queue_snapshot;
mod released_tasks;
pub use released_tasks::{release_carrying_every_commit, release_deployment_reference};
mod support;
mod support_submission;
mod task_block;
pub use ops_tickets::{OpsTicketError, OpsTicketProgress, OpsTicketService};
pub use queue_snapshot::{
    PendingDecisionOverlap, PendingDecisionOverlapSnapshot, QueenQueueSnapshot,
};
pub use support::{SupportService, SupportServiceError};
pub use support_submission::{HiveSupportService, HiveSupportServiceError, SupportDestination};

/// The durable agent identity resolved before an application command is invoked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentPrincipal {
    pub worker_id: WorkerId,
    pub role: WorkerRole,
    pub active_session_id: Option<WorkerSessionId>,
}

impl From<&WorkerProfile> for AgentPrincipal {
    fn from(profile: &WorkerProfile) -> Self {
        Self {
            worker_id: profile.id,
            role: profile.role,
            active_session_id: profile.active_session_id,
        }
    }
}

#[derive(Clone)]
pub struct TaskService {
    store: TaskStore,
}

/// Coordinates the local side of Apiary membership. Adapter evidence is typed
/// input; sealed readiness and durable membership remain domain/store owned.
#[derive(Clone)]
pub struct ApiaryService {
    store: TaskStore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiaryInvitationOverview {
    pub invitation: ApiaryInvitation,
    pub apiary: Apiary,
    pub readiness: ApiaryJoinReadiness,
    pub jira_connection: JiraConnectionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiaryHiveCandidateOverview {
    pub candidate: ApiaryHiveCandidate,
    pub invitation_pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FederationJoinInvitationOverview {
    pub invitation: FederationJoinInvitation,
    pub readiness: FederationJoinReadiness,
}

/// The title every fleet-version raise carries.
///
/// ⚠️ ONE TITLE FOR ALL OF THEM, ON PURPOSE. A title per Hive would mean a
/// separate card per member the moment a release cuts, which is a pile rather
/// than a signal — and the pile is what gets dismissed wholesale. The card
/// names the Hives in its summary instead, and it is the dedup key.
const FLEET_BEHIND_QUESTION_TITLE: &str = "Hives are behind the current release";

impl ApiaryService {
    /// Raises the Hives that have fallen behind, and withdraws the raise once
    /// they catch up.
    ///
    /// Returns the report it judged, so a caller can surface the same numbers
    /// it acted on rather than reading them again and possibly differing.
    ///
    /// ⚠️ EVENT-DRIVEN, NOT TIMED. This runs when the answer can actually have
    /// changed — a member reported, or a new release appeared — rather than on
    /// a repeating timer, because a periodic sweep would be a background task to
    /// own and bound for no gain over the moments that already exist.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn raise_hives_left_behind(
        &self,
        now: i64,
    ) -> Result<swarm_persistence::FleetVersionReport, ApplicationError> {
        let report = self.store.fleet_version_report(now)?;
        let raised = report.raised();
        // ⚠️ THE CARD MUST GO AWAY BY ITSELF. The operator's standing complaint
        // about this machinery is a card that keeps coming back after being
        // dealt with. A Hive that upgraded has answered the question, and
        // leaving the card up would make catching up look like nothing
        // happened.
        let open = self
            .store
            .list_decision_requests()?
            .into_iter()
            .filter(|request| {
                request.title == FLEET_BEHIND_QUESTION_TITLE
                    && request.state == swarm_domain::DecisionRequestState::Pending
            })
            .collect::<Vec<_>>();
        if raised.is_empty() {
            for request in open {
                if let Err(error) = self.store.withdraw_decision_request(
                    request.id,
                    request.requesting_worker_id,
                    "Every Hive has caught up.",
                ) {
                    tracing::warn!(
                        message = %error,
                        decision = %request.id,
                        "the fleet caught up and its raise could not be withdrawn"
                    );
                }
            }
            return Ok(report);
        }
        // Already asked, and still true. Asking again would be the loop.
        if !open.is_empty() {
            return Ok(report);
        }
        let Ok(Some(queen)) = self.store.queen_worker_id() else {
            tracing::warn!(
                behind = raised.len(),
                "Hives are behind and there is no Queen to raise it to"
            );
            return Ok(report);
        };
        let expected = report
            .expected_release
            .as_ref()
            .map_or("an unknown release", |(version, _)| version.as_str());
        let listed = raised
            .iter()
            .map(|hive| {
                format!(
                    "- {} runs {} (schema {}), last reported {}: {:?}",
                    hive.identity.hive_id,
                    hive.swarm_version,
                    hive.database_schema_version,
                    hive.observed_at,
                    hive.standing
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let summary = format!(
            "{} Hive(s) in the Apiary are not on {expected}, and the grace window \
             has passed.\n{listed}",
            raised.len()
        );
        if let Err(error) = self.store.create_decision_request(&NewDecisionRequest {
            requesting_worker_id: queen,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: FLEET_BEHIND_QUESTION_TITLE,
            summary: &summary,
            reason: "A Hive left on an old build stops being evidence about what \
                     gets released, which is the point of running it.",
            risk: "This Hive sat wedged on a stale build for roughly a day in \
                   September and every screen that could have said so showed a \
                   version and nothing else.",
            evidence: &summary,
            suggested_action: "Update them",
            allowed_actions: &[
                "Update them".to_owned(),
                "Leave them behind for now".to_owned(),
            ],
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: None,
        }) {
            tracing::warn!(
                message = %error,
                behind = raised.len(),
                "Hives are behind and the raise could not be recorded"
            );
        }
        Ok(report)
    }

    /// Exchanges an authenticated member's signed public profile for a signed
    /// full directory. A retry after response loss does not duplicate changes.
    ///
    /// # Errors
    /// Rejects invalid member credentials, signatures, revisions or storage.
    pub fn exchange_federation_directory(
        &self,
        credential: &str,
        update: &swarm_domain::FederationProfileUpdate,
        now: i64,
    ) -> Result<swarm_domain::FederationDirectorySnapshot, ApplicationError> {
        self.store
            .accept_federation_public_profile(credential, update, now)?;
        self.store
            .signed_federation_directory(credential, now)
            .map_err(Into::into)
    }

    /// Keeper signing the current policy body for one authenticated member.
    ///
    /// # Errors
    /// Rejects invalid credentials and unavailable storage.
    pub fn signed_apiary_policy(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<swarm_domain::ApiaryPolicySnapshot, ApplicationError> {
        self.store
            .signed_apiary_policy(credential, now)
            .map_err(Into::into)
    }

    /// A member verifying and storing the Apiary's current defaults.
    ///
    /// # Errors
    /// Rejects non-members, bad signatures, wrong scope and expired snapshots.
    pub fn apply_apiary_policy(
        &self,
        snapshot: &swarm_domain::ApiaryPolicySnapshot,
        now: i64,
    ) -> Result<bool, ApplicationError> {
        self.store
            .apply_apiary_policy(snapshot, now)
            .map_err(Into::into)
    }

    /// Where this member stands against the Apiary's defaults.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a false convergence.
    pub fn local_policy_convergence(
        &self,
    ) -> Result<swarm_persistence::PolicyConvergence, ApplicationError> {
        self.store.local_policy_convergence().map_err(Into::into)
    }

    /// The Apiary this Hive belongs to, if any.
    ///
    /// # Errors
    /// Returns storage failures; a personal Hive is `None` rather than an error.
    pub fn local_apiary_id(&self) -> Result<Option<swarm_domain::ApiaryId>, ApplicationError> {
        Ok(self.store.local_hive_identity()?.hive.apiary_id)
    }

    /// The Apiary one authenticated member node belongs to.
    ///
    /// # Errors
    /// Rejects malformed, unknown and expired credentials.
    pub fn authenticated_member_apiary(
        &self,
        credential: &str,
        now: i64,
    ) -> Result<swarm_domain::ApiaryId, ApplicationError> {
        self.store
            .authenticated_member_apiary(credential, now)
            .map_err(Into::into)
    }

    /// Keeper accepting one authenticated member's capability report.
    ///
    /// # Errors
    /// Rejects invalid member credentials, signatures, revisions or storage.
    pub fn accept_hive_capability(
        &self,
        credential: &str,
        update: &swarm_domain::HiveCapabilityUpdate,
        now: i64,
    ) -> Result<bool, ApplicationError> {
        self.store
            .accept_hive_capability(credential, update, now)
            .map_err(Into::into)
    }

    /// Seals this Hive's derived capability into one signed report.
    ///
    /// The workers are derived by the caller, because resolving a repository
    /// remote means reading git and that is not database state.
    ///
    /// # Errors
    /// Rejects non-members, invalid input and unavailable storage.
    pub fn seal_local_hive_capability(
        &self,
        workers: &[swarm_domain::HiveCapabilityWorker],
        workers_truncated: bool,
        swarm_version: &str,
        database_schema_version: i64,
        now: i64,
    ) -> Result<swarm_domain::HiveCapabilityUpdate, ApplicationError> {
        self.store
            .seal_local_hive_capability(
                workers,
                workers_truncated,
                swarm_version,
                database_schema_version,
                now,
            )
            .map_err(Into::into)
    }

    /// The fleet's capability as this Hive holds it.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a partial fleet.
    pub fn federation_hive_capabilities(
        &self,
    ) -> Result<Vec<swarm_persistence::StoredHiveCapability>, ApplicationError> {
        self.store
            .federation_hive_capabilities()
            .map_err(Into::into)
    }

    /// Prepares the latest local profile for its current membership.
    ///
    /// # Errors
    /// Rejects missing membership, invalid profile or unavailable storage.
    pub fn signed_local_profile(
        &self,
        now: i64,
    ) -> Result<swarm_domain::FederationProfileUpdate, ApplicationError> {
        self.store
            .signed_local_profile_update(now)
            .map_err(Into::into)
    }

    /// Verifies and saves the directory without granting local authority.
    ///
    /// # Errors
    /// Rejects invalid signatures, scope, revisions or unavailable storage.
    pub fn apply_directory(
        &self,
        snapshot: &swarm_domain::FederationDirectorySnapshot,
        now: i64,
    ) -> Result<bool, ApplicationError> {
        self.store
            .apply_federation_directory(snapshot, now)
            .map_err(Into::into)
    }

    /// Reads the last verified member directory with its freshness metadata.
    ///
    /// # Errors
    /// Rejects missing membership or corrupt/unavailable storage.
    pub fn local_directory(
        &self,
    ) -> Result<Option<swarm_domain::FederationDirectoryPayload>, ApplicationError> {
        self.store.local_federation_directory().map_err(Into::into)
    }

    #[must_use]
    pub const fn new(store: TaskStore) -> Self {
        Self { store }
    }

    /// Authenticates a joined member node and returns its Keeper-signed public
    /// promoted-project catalog. No Jira or membership state is mutated.
    ///
    /// # Errors
    /// Rejects invalid or expired member credentials, non-Keepers, identity
    /// drift, and unavailable persistence.
    pub fn federation_catalog(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationCatalogSnapshot, ApplicationError> {
        self.store
            .signed_federation_catalog(node_credential, now)
            .map_err(Into::into)
    }

    /// Returns only the authenticated Member operator's current Steward scope.
    ///
    /// # Errors
    /// Rejects invalid credentials, non-Keepers, malformed grants, and unavailable persistence.
    pub fn federation_stewardship(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardshipSnapshot, ApplicationError> {
        self.store
            .federation_stewardship_snapshot(node_credential, now)
            .map_err(Into::into)
    }

    /// Replaces the Member's local Keeper-confirmed Steward projection.
    ///
    /// # Errors
    /// Rejects foreign, malformed, or incompatible snapshots and unavailable persistence.
    pub fn apply_federation_stewardship(
        &self,
        snapshot: &FederationStewardshipSnapshot,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .apply_federation_stewardship_snapshot(snapshot, now)
            .map_err(Into::into)
    }

    /// Returns the last Keeper-confirmed local Steward projection.
    ///
    /// # Errors
    /// Returns an error when the projection is corrupt or unavailable.
    pub fn local_federation_stewardship(
        &self,
    ) -> Result<Option<FederationStewardshipSnapshot>, ApplicationError> {
        self.store
            .local_federation_stewardship_snapshot()
            .map_err(Into::into)
    }

    /// Applies one authenticated, retry-stable Steward task command on Keeper.
    ///
    /// # Errors
    /// Returns authorization, validation, conflict, bound, or persistence errors.
    pub fn apply_federation_steward_task_command(
        &self,
        node_credential: &str,
        command: &FederationStewardTaskCommand,
        now: i64,
    ) -> Result<FederationStewardTaskReceipt, ApplicationError> {
        self.store
            .apply_federation_steward_task_command(node_credential, command, now)
            .map_err(Into::into)
    }

    /// Queues one offline-safe Steward task for an explicitly managed Hive.
    ///
    /// # Errors
    /// Returns scope, validation, outbox-bound, role, or persistence errors.
    pub fn queue_federation_steward_task(
        &self,
        target_hive_id: HiveId,
        title: &str,
        description: &str,
        priority: TaskPriority,
        now: i64,
    ) -> Result<FederationStewardTaskOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_task(target_hive_id, title, description, priority, now)
            .map_err(Into::into)
    }

    /// Returns one bounded batch of queued Steward commands.
    ///
    /// # Errors
    /// Returns invalid-bound or persistence errors.
    pub fn pending_federation_steward_tasks(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardTaskOutboxEntry>, ApplicationError> {
        self.store
            .pending_federation_steward_tasks(limit)
            .map_err(Into::into)
    }

    /// Records one outbound Steward command attempt.
    ///
    /// # Errors
    /// Returns role, validation, missing-command, or persistence errors.
    pub fn record_federation_steward_task_attempt(
        &self,
        command_id: FederationStewardTaskCommandId,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .record_federation_steward_task_attempt(command_id, now)
            .map_err(Into::into)
    }

    /// Applies one exact Keeper receipt to the local outbox.
    ///
    /// # Errors
    /// Returns receipt conflict, validation, role, or persistence errors.
    pub fn apply_federation_steward_task_receipt(
        &self,
        receipt: &FederationStewardTaskReceipt,
        now: i64,
    ) -> Result<FederationStewardTaskOutboxEntry, ApplicationError> {
        self.store
            .apply_federation_steward_task_receipt(receipt, now)
            .map_err(Into::into)
    }

    /// Returns recent local Steward command delivery evidence.
    ///
    /// # Errors
    /// Returns corrupt-record or persistence errors.
    pub fn federation_steward_task_outbox(
        &self,
    ) -> Result<Vec<FederationStewardTaskOutboxEntry>, ApplicationError> {
        self.store
            .list_federation_steward_task_outbox()
            .map_err(Into::into)
    }

    /// Returns recent Keeper-side audit evidence for guarded Steward task
    /// routing.
    ///
    /// # Errors
    /// Returns role, bound, corrupt-record, or persistence errors.
    pub fn federation_steward_task_audit(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardTaskAuditEntry>, ApplicationError> {
        self.store
            .list_federation_steward_task_audit(limit)
            .map_err(Into::into)
    }

    /// Applies one authenticated, retry-stable Steward assistance command on Keeper.
    ///
    /// # Errors
    /// Returns authentication, authorization, validation, or persistence errors.
    pub fn apply_federation_steward_assist_command(
        &self,
        node_credential: &str,
        command: &FederationStewardAssistCommand,
        now: i64,
    ) -> Result<FederationStewardAssistReceipt, ApplicationError> {
        self.store
            .apply_federation_steward_assist_command(node_credential, command, now)
            .map_err(Into::into)
    }

    /// Reads assistance addressed only to the authenticated Member Hive.
    ///
    /// # Errors
    /// Returns authentication or persistence errors.
    pub fn federation_steward_assist_inbox(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardAssistInbox, ApplicationError> {
        self.store
            .federation_steward_assist_inbox(node_credential, now)
            .map_err(Into::into)
    }

    /// Queues a structured Steward request without network I/O or terminal injection.
    ///
    /// # Errors
    /// Returns role, scope, validation, queue-bound, or persistence errors.
    pub fn queue_federation_steward_assist(
        &self,
        target_hive_id: HiveId,
        message: &str,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_assist(target_hive_id, message, now)
            .map_err(Into::into)
    }

    /// Queues the target operator's explicit accept or decline response.
    ///
    /// # Errors
    /// Returns validation, queue-bound, or persistence errors.
    pub fn queue_federation_steward_assist_response(
        &self,
        request_id: FederationStewardAssistRequestId,
        decision: FederationStewardAssistState,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_assist_response(request_id, decision, now)
            .map_err(Into::into)
    }

    /// Returns queued Assist commands awaiting Keeper delivery.
    ///
    /// # Errors
    /// Returns bound, corrupt-record, or persistence errors.
    pub fn pending_federation_steward_assists(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardAssistOutboxEntry>, ApplicationError> {
        self.store
            .pending_federation_steward_assists(limit)
            .map_err(Into::into)
    }

    /// Records a delivery attempt for a queued Assist command.
    ///
    /// # Errors
    /// Returns state or persistence errors.
    pub fn record_federation_steward_assist_attempt(
        &self,
        command_id: FederationStewardAssistCommandId,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .record_federation_steward_assist_attempt(command_id, now)
            .map_err(Into::into)
    }

    /// Applies Keeper's receipt to the matching local Assist command.
    ///
    /// # Errors
    /// Returns validation, state, or persistence errors.
    pub fn apply_federation_steward_assist_receipt(
        &self,
        receipt: &FederationStewardAssistReceipt,
        now: i64,
    ) -> Result<FederationStewardAssistOutboxEntry, ApplicationError> {
        self.store
            .apply_federation_steward_assist_receipt(receipt, now)
            .map_err(Into::into)
    }

    /// Replaces the local Assist projection with Keeper's bounded inbox.
    ///
    /// # Errors
    /// Returns role, validation, or persistence errors.
    pub fn apply_federation_steward_assist_inbox(
        &self,
        inbox: &FederationStewardAssistInbox,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .apply_federation_steward_assist_inbox(inbox, now)
            .map_err(Into::into)
    }

    /// Returns operator-facing local Assist state.
    ///
    /// # Errors
    /// Returns invalid-identity, corrupt-record, or persistence errors.
    pub fn federation_steward_assist_local_state(
        &self,
    ) -> Result<FederationStewardAssistLocalState, ApplicationError> {
        self.store
            .federation_steward_assist_local_state()
            .map_err(Into::into)
    }

    /// Applies one authenticated, retry-stable takeover transition on Keeper.
    ///
    /// # Errors
    /// Returns authentication, authorization, validation, conflict, or persistence errors.
    pub fn apply_federation_steward_takeover_command(
        &self,
        node_credential: &str,
        command: &FederationStewardTakeoverCommand,
        now: i64,
    ) -> Result<FederationStewardTakeoverReceipt, ApplicationError> {
        self.store
            .apply_federation_steward_takeover_command(node_credential, command, now)
            .map_err(Into::into)
    }

    /// Reads only takeover leases involving the authenticated Member Hive.
    ///
    /// # Errors
    /// Returns authentication or persistence errors.
    pub fn federation_steward_takeover_inbox(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverInbox, ApplicationError> {
        self.store
            .federation_steward_takeover_inbox(node_credential, now)
            .map_err(Into::into)
    }

    /// Revalidates one exact active participant immediately before relay I/O.
    ///
    /// # Errors
    /// Returns authentication, scope, revision, expiry, or persistence errors.
    pub fn authorize_federation_steward_takeover_relay(
        &self,
        node_credential: &str,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverRelayAuthorization, ApplicationError> {
        self.store
            .authorize_federation_steward_takeover_relay(node_credential, lease_id, revision, now)
            .map_err(Into::into)
    }

    /// Journals a reasoned takeover request before network I/O.
    ///
    /// # Errors
    /// Returns scope, protocol, role, queue-bound, or persistence errors.
    pub fn queue_federation_steward_takeover(
        &self,
        target_hive_id: HiveId,
        reason: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_takeover(target_hive_id, reason, now)
            .map_err(Into::into)
    }

    /// Journals target acknowledgement after exact local host installation.
    ///
    /// # Errors
    /// Returns projection, revision, queue-bound, or persistence errors.
    pub fn queue_federation_steward_takeover_acknowledgement(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_takeover_acknowledgement(lease_id, expected_revision, now)
            .map_err(Into::into)
    }

    /// Journals immediate local reclaim before contacting Keeper.
    ///
    /// # Errors
    /// Returns projection, revision, queue-bound, or persistence errors.
    pub fn queue_federation_steward_takeover_reclaim(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        reason: &str,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_takeover_reclaim(lease_id, expected_revision, reason, now)
            .map_err(Into::into)
    }

    /// Journals an active lease renewal after authenticated Steward input.
    ///
    /// # Errors
    /// Returns projection, revision, queue-bound, or persistence errors.
    pub fn queue_federation_steward_takeover_renewal(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_takeover_renewal(lease_id, expected_revision, now)
            .map_err(Into::into)
    }

    /// Journals source release of an active lease.
    ///
    /// # Errors
    /// Returns projection, revision, queue-bound, or persistence errors.
    pub fn queue_federation_steward_takeover_release(
        &self,
        lease_id: FederationStewardTakeoverLeaseId,
        expected_revision: u64,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_steward_takeover_release(lease_id, expected_revision, now)
            .map_err(Into::into)
    }

    /// Returns the bounded takeover command delivery batch.
    ///
    /// # Errors
    /// Returns invalid-bound or persistence errors.
    pub fn pending_federation_steward_takeovers(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationStewardTakeoverOutboxEntry>, ApplicationError> {
        self.store
            .pending_federation_steward_takeovers(limit)
            .map_err(Into::into)
    }

    /// Records one takeover delivery attempt.
    ///
    /// # Errors
    /// Returns state or persistence errors.
    pub fn record_federation_steward_takeover_attempt(
        &self,
        command_id: FederationStewardTakeoverCommandId,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .record_federation_steward_takeover_attempt(command_id, now)
            .map_err(Into::into)
    }

    /// Applies one exact Keeper receipt to the local outbox.
    ///
    /// # Errors
    /// Returns state, validation, or persistence errors.
    pub fn apply_federation_steward_takeover_receipt(
        &self,
        receipt: &FederationStewardTakeoverReceipt,
        now: i64,
    ) -> Result<FederationStewardTakeoverOutboxEntry, ApplicationError> {
        self.store
            .apply_federation_steward_takeover_receipt(receipt, now)
            .map_err(Into::into)
    }

    /// Replaces the local public takeover projection.
    ///
    /// # Errors
    /// Returns role, validation, or persistence errors.
    pub fn apply_federation_steward_takeover_inbox(
        &self,
        inbox: &FederationStewardTakeoverInbox,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .apply_federation_steward_takeover_inbox(inbox, now)
            .map_err(Into::into)
    }

    /// Returns local takeover lease and outbox evidence.
    ///
    /// # Errors
    /// Returns corrupt-record or persistence errors.
    pub fn federation_steward_takeover_local_state(
        &self,
    ) -> Result<FederationStewardTakeoverLocalState, ApplicationError> {
        self.store
            .federation_steward_takeover_local_state()
            .map_err(Into::into)
    }

    /// Returns one authenticated, bounded Keeper-canonical Swarm task page.
    /// Jira issue content never enters this path.
    ///
    /// # Errors
    /// Rejects invalid credentials/cursors, non-Keepers, and persistence failures.
    pub fn federation_task_page(
        &self,
        node_credential: &str,
        after: i64,
        now: i64,
    ) -> Result<FederationTaskPage, ApplicationError> {
        self.store
            .federation_task_page(node_credential, after, now)
            .map_err(Into::into)
    }

    /// Applies one ordered Keeper page to the Member's durable projection.
    ///
    /// # Errors
    /// Rejects non-Members, foreign or gapped pages, and persistence failures.
    pub fn apply_federation_task_page(
        &self,
        page: &FederationTaskPage,
        now: i64,
    ) -> Result<FederationTaskSyncStatus, ApplicationError> {
        self.store
            .apply_federation_task_page(page, now)
            .map_err(Into::into)
    }

    /// Returns content-free evidence for the local Apiary task projection.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable projection state.
    pub fn federation_task_sync_status(
        &self,
    ) -> Result<FederationTaskSyncStatus, ApplicationError> {
        self.store.federation_task_sync_status().map_err(Into::into)
    }

    /// Lists the member-local Keeper task projection without contacting Keeper.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable projection state.
    pub fn local_apiary_tasks(&self) -> Result<Vec<ApiaryTask>, ApplicationError> {
        self.store.list_local_apiary_tasks().map_err(Into::into)
    }

    /// Lists canonical Keeper tasks or the Member's durable local projection.
    ///
    /// # Errors
    /// Returns an error for invalid membership or unavailable state.
    pub fn visible_apiary_tasks(&self) -> Result<Vec<ApiaryTask>, ApplicationError> {
        self.store.list_visible_apiary_tasks().map_err(Into::into)
    }

    /// Creates or returns the private local execution task for one owned
    /// Keeper-canonical work item.
    ///
    /// # Errors
    /// Rejects non-Members, foreign/completed work, invalid private workers,
    /// or unavailable persistence.
    pub fn materialize_local_apiary_task_execution(
        &self,
        apiary_task_id: swarm_domain::ApiaryTaskId,
        worker_id: WorkerId,
        now: i64,
    ) -> Result<LocalApiaryTaskExecution, ApplicationError> {
        self.store
            .materialize_local_apiary_task_execution(apiary_task_id, worker_id, now)
            .map_err(Into::into)
    }

    /// Lists this Hive's private Apiary-to-local task bridges.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable local state.
    pub fn local_apiary_task_executions(
        &self,
    ) -> Result<Vec<LocalApiaryTaskExecution>, ApplicationError> {
        self.store
            .list_local_apiary_task_executions()
            .map_err(Into::into)
    }

    /// Creates one Swarm-generated Apiary task on the Keeper.
    ///
    /// # Errors
    /// Rejects non-Keepers, invalid content, capacity exhaustion, and persistence failures.
    pub fn create_apiary_task(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        now: i64,
    ) -> Result<ApiaryTask, ApplicationError> {
        self.store
            .create_apiary_task(title, description, priority, now)
            .map_err(Into::into)
    }

    /// Creates one Keeper-canonical Swarm task and optionally routes it to an
    /// active Member Hive without selecting any of that Hive's private workers.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown target Hives, invalid content, capacity
    /// exhaustion, and persistence failures.
    pub fn create_apiary_task_for_hive(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        home_hive_id: Option<swarm_domain::HiveId>,
        now: i64,
    ) -> Result<ApiaryTask, ApplicationError> {
        self.store
            .create_apiary_task_for_hive(title, description, priority, home_hive_id, now)
            .map_err(Into::into)
    }

    /// Relocates a set of existing Hive tasks to the Apiary level in one
    /// transaction, carrying their prerequisite ordering with them.
    ///
    /// The local tasks are NOT retired: each keeps its history, evidence and
    /// decision links, and the origin link is how a reader gets back to them.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown or removed tasks, and any set that would
    /// leave a prerequisite chain straddling the Hive and the Apiary.
    pub fn relocate_local_tasks_to_apiary(
        &self,
        local_task_ids: &[swarm_domain::TaskId],
        now: i64,
    ) -> Result<Vec<ApiaryTask>, ApplicationError> {
        self.store
            .relocate_local_tasks_to_apiary(local_task_ids, now)
            .map_err(Into::into)
    }

    /// The Apiary task one local task became, if it has been relocated.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable local state.
    pub fn relocated_apiary_task_for_local_task(
        &self,
        local_task_id: swarm_domain::TaskId,
    ) -> Result<Option<ApiaryTask>, ApplicationError> {
        self.store
            .relocated_apiary_task_for_local_task(local_task_id)
            .map_err(Into::into)
    }

    /// Applies one authenticated idempotent Member command on Keeper.
    ///
    /// # Errors
    /// Rejects invalid credentials, command identity, revision, or persistence.
    pub fn apply_federation_task_command(
        &self,
        node_credential: &str,
        command: &FederationTaskCommand,
        now: i64,
    ) -> Result<FederationTaskCommandReceipt, ApplicationError> {
        self.store
            .apply_federation_task_command(node_credential, command, now)
            .map_err(Into::into)
    }

    /// Queues one Member claim for delivery to Keeper.
    ///
    /// # Errors
    /// Rejects invalid membership, task state, queue capacity, or persistence.
    pub fn queue_federation_task_claim(
        &self,
        task_id: swarm_domain::ApiaryTaskId,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_task_claim(task_id, now)
            .map_err(Into::into)
    }

    /// Queues one Member-owned task transition for delivery to Keeper.
    ///
    /// # Errors
    /// Rejects invalid membership, ownership, transition, capacity, or persistence.
    pub fn queue_federation_task_transition(
        &self,
        task_id: swarm_domain::ApiaryTaskId,
        target_state: TaskState,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, ApplicationError> {
        self.store
            .queue_federation_task_transition(task_id, target_state, now)
            .map_err(Into::into)
    }

    /// Returns the oldest bounded commands waiting for Keeper.
    ///
    /// # Errors
    /// Rejects invalid bounds or corrupt/unavailable persistence.
    pub fn pending_federation_task_commands(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationTaskOutboxEntry>, ApplicationError> {
        self.store
            .pending_federation_task_commands(limit)
            .map_err(Into::into)
    }

    /// Stages the next legal Keeper transition for each locally linked task.
    ///
    /// # Errors
    /// Rejects invalid Member state, corrupt projections, capacity, or storage.
    pub fn prepare_local_apiary_task_lifecycle_commands(
        &self,
        now: i64,
    ) -> Result<usize, ApplicationError> {
        self.store
            .prepare_local_apiary_task_lifecycle_commands(now)
            .map_err(Into::into)
    }

    /// Durably records one transport attempt before network I/O.
    ///
    /// # Errors
    /// Rejects unknown commands, invalid time, or unavailable persistence.
    pub fn record_federation_task_command_attempt(
        &self,
        command_id: FederationTaskCommandId,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .record_federation_task_command_attempt(command_id, now)
            .map_err(Into::into)
    }

    /// Stores one exact Keeper receipt against its queued command.
    ///
    /// # Errors
    /// Rejects unknown or altered receipts and unavailable persistence.
    pub fn apply_federation_task_command_receipt(
        &self,
        receipt: &FederationTaskCommandReceipt,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, ApplicationError> {
        self.store
            .apply_federation_task_command_receipt(receipt, now)
            .map_err(Into::into)
    }

    /// Lists bounded operator-visible outbound command evidence.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable persistence.
    pub fn federation_task_outbox(
        &self,
    ) -> Result<Vec<FederationTaskOutboxEntry>, ApplicationError> {
        self.store.list_federation_task_outbox().map_err(Into::into)
    }

    /// Returns content-free pending and attention counts.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn federation_task_outbox_status(
        &self,
    ) -> Result<FederationTaskOutboxStatus, ApplicationError> {
        self.store
            .federation_task_outbox_status()
            .map_err(Into::into)
    }

    /// Atomically reserves one promoted Jira issue for an authenticated member
    /// Hive before that Hive attempts the canonical Jira assignee write.
    ///
    /// # Errors
    /// Rejects invalid or expired node credentials, unknown project/issue
    /// identities, and issues already owned or reserved by another Hive.
    pub fn reserve_federation_claim(
        &self,
        node_credential: &str,
        project_id: &str,
        issue_id: &str,
        issue_key: &str,
        now: i64,
    ) -> Result<FederationSharedClaim, ApplicationError> {
        self.store
            .reserve_federation_claim(node_credential, project_id, issue_id, issue_key, now)
            .map_err(Into::into)
    }

    /// Confirms a reservation only after the member Hive has received Jira's
    /// acknowledgement of the human-assignee change.
    ///
    /// # Errors
    /// Rejects invalid credentials, foreign, expired, or released claims, and
    /// unavailable persistence.
    pub fn confirm_federation_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        now: i64,
    ) -> Result<FederationSharedClaim, ApplicationError> {
        self.store
            .confirm_federation_claim(node_credential, claim_id, now)
            .map_err(Into::into)
    }

    /// Releases one still-unconfirmed reservation after the member's Jira
    /// assignment fails. Confirmed claims use the later governed handoff path.
    ///
    /// # Errors
    /// Rejects invalid credentials, foreign, confirmed, or expired claims, and
    /// unavailable persistence.
    pub fn release_federation_claim(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        now: i64,
    ) -> Result<FederationSharedClaim, ApplicationError> {
        self.store
            .release_federation_claim(node_credential, claim_id, now)
            .map_err(Into::into)
    }

    /// Lists only active reservations and confirmed home-Hive ownership for
    /// the Keeper's low-noise shared-work rollup. No remote system is read or
    /// mutated by this command.
    ///
    /// # Errors
    /// Rejects personal and Member Hives, invalid time, corrupt state, and
    /// unavailable persistence.
    pub fn active_federation_claims(
        &self,
        now: i64,
    ) -> Result<Vec<FederationSharedClaim>, ApplicationError> {
        self.store
            .list_active_federation_claims(now)
            .map_err(Into::into)
    }

    /// Reads only claims owned by the authenticated member from its Keeper.
    ///
    /// # Errors
    /// Rejects invalid credentials, invalid time, or unavailable persistence.
    pub fn member_federation_claims(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<Vec<FederationSharedClaim>, ApplicationError> {
        self.store
            .list_member_federation_claims(node_credential, now)
            .map_err(Into::into)
    }

    /// Returns the local membership role used to select the federation transport.
    ///
    /// # Errors
    /// Returns an error when durable membership state is unavailable.
    pub fn local_context(&self) -> Result<LocalApiaryContext, ApplicationError> {
        self.store.local_apiary_context().map_err(Into::into)
    }

    /// Lists public destination identities for a member-initiated handoff.
    ///
    /// # Errors
    /// Rejects invalid credentials or unavailable persistence.
    pub fn federation_handoff_targets(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<Vec<swarm_domain::FederationHandoffTarget>, ApplicationError> {
        self.store
            .list_federation_handoff_targets(node_credential, now)
            .map_err(Into::into)
    }

    /// Offers a confirmed shared claim to another active Hive.
    ///
    /// # Errors
    /// Rejects invalid actors, claims, targets, content, or conflicts.
    pub fn offer_federation_claim_handoff(
        &self,
        node_credential: &str,
        claim_id: FederationClaimId,
        target_node_id: FederationNodeId,
        reason: Option<&str>,
        now: i64,
    ) -> Result<FederationClaimHandoff, ApplicationError> {
        self.store
            .offer_federation_claim_handoff(node_credential, claim_id, target_node_id, reason, now)
            .map_err(Into::into)
    }

    /// Lists the authenticated member's bounded handoff feed.
    ///
    /// # Errors
    /// Rejects invalid credentials or unavailable persistence.
    pub fn federation_claim_handoffs(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<Vec<FederationClaimHandoff>, ApplicationError> {
        self.store
            .list_federation_claim_handoffs(node_credential, now)
            .map_err(Into::into)
    }

    /// Lists the Keeper's bounded Apiary-wide handoff rollup.
    ///
    /// # Errors
    /// Rejects personal or Member Hives and unavailable persistence.
    pub fn all_federation_claim_handoffs(
        &self,
        now: i64,
    ) -> Result<Vec<FederationClaimHandoff>, ApplicationError> {
        self.store
            .list_all_federation_claim_handoffs(now)
            .map_err(Into::into)
    }

    /// Accepts an offer as its target Hive.
    ///
    /// # Errors
    /// Rejects invalid actors or lifecycle transitions.
    pub fn accept_federation_claim_handoff(
        &self,
        credential: &str,
        id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, ApplicationError> {
        self.store
            .accept_federation_claim_handoff(credential, id, now)
            .map_err(Into::into)
    }

    /// Declines an offer as its target Hive.
    ///
    /// # Errors
    /// Rejects invalid actors or lifecycle transitions.
    pub fn decline_federation_claim_handoff(
        &self,
        credential: &str,
        id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, ApplicationError> {
        self.store
            .decline_federation_claim_handoff(credential, id, now)
            .map_err(Into::into)
    }

    /// Cancels an unaccepted offer as its source Hive.
    ///
    /// # Errors
    /// Rejects invalid actors or lifecycle transitions.
    pub fn cancel_federation_claim_handoff(
        &self,
        credential: &str,
        id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, ApplicationError> {
        self.store
            .cancel_federation_claim_handoff(credential, id, now)
            .map_err(Into::into)
    }

    /// Confirms successful target-side Jira assignment and transfers ownership.
    ///
    /// # Errors
    /// Rejects invalid actors, claim drift, or lifecycle transitions.
    pub fn confirm_federation_claim_handoff(
        &self,
        credential: &str,
        id: FederationClaimHandoffId,
        now: i64,
    ) -> Result<FederationClaimHandoff, ApplicationError> {
        self.store
            .confirm_federation_claim_handoff(credential, id, now)
            .map_err(Into::into)
    }

    /// Returns the Member Hive's content-free durable reconciliation health.
    /// No transport or remote system is contacted.
    ///
    /// # Errors
    /// Rejects personal and Keeper Hives and unavailable or corrupt storage.
    pub fn federation_sync_health(&self) -> Result<FederationSyncHealth, ApplicationError> {
        self.store.federation_sync_health().map_err(Into::into)
    }

    /// Returns host-private transport material for the local joined Member.
    /// Adapters must never serialize this value into browser or agent output.
    ///
    /// # Errors
    /// Rejects personal and Keeper Hives and missing or corrupt membership.
    pub fn federation_member_connection(
        &self,
    ) -> Result<FederationMemberConnection, ApplicationError> {
        self.store
            .federation_member_connection()
            .map_err(Into::into)
    }

    /// Returns only local durable blockers before any Keeper request is made.
    ///
    /// # Errors
    /// Rejects non-Members and corrupt local membership state.
    pub fn local_departure_readiness(
        &self,
    ) -> Result<FederationDepartureReadiness, ApplicationError> {
        self.store
            .local_federation_departure_readiness()
            .map_err(Into::into)
    }

    /// Returns local progress and blockers even when a prior departure request
    /// is frozen for an exact retry after an uncertain transport outcome.
    ///
    /// # Errors
    /// Rejects non-Members and corrupt local membership state.
    pub fn local_departure_overview(
        &self,
    ) -> Result<FederationDepartureOverview, ApplicationError> {
        self.store
            .local_federation_departure_overview()
            .map_err(Into::into)
    }

    /// Returns host-private Keeper transport material for a departure retry.
    ///
    /// # Errors
    /// Rejects missing or corrupt membership material and invalid endpoints.
    pub fn departure_connection(&self) -> Result<FederationMemberConnection, ApplicationError> {
        self.store
            .federation_departure_connection()
            .map_err(Into::into)
    }

    /// Freezes new local shared-work mutations and returns the existing private
    /// Keeper connection for the explicit departure request.
    ///
    /// # Errors
    /// Rejects outstanding local work, non-Members, and invalid time/state.
    pub fn begin_departure(
        &self,
        now: i64,
    ) -> Result<FederationMemberConnection, ApplicationError> {
        self.store
            .begin_federation_departure(now)
            .map_err(Into::into)
    }

    /// Unfreezes a departure only after an authoritative Keeper readiness
    /// conflict. Transport ambiguity is deliberately not a reason to call it.
    ///
    /// # Errors
    /// Rejects missing or corrupt departure state.
    pub fn cancel_departure(&self) -> Result<(), ApplicationError> {
        self.store.cancel_federation_departure().map_err(Into::into)
    }

    /// Returns Keeper-owned blockers for one exact authenticated Member.
    ///
    /// # Errors
    /// Rejects invalid credentials, non-Keepers, and corrupt shared state.
    pub fn remote_departure_readiness(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationDepartureReadiness, ApplicationError> {
        self.store
            .federation_departure_readiness(node_credential, now)
            .map_err(Into::into)
    }

    /// Atomically ends one Keeper-side membership and returns its signed,
    /// retry-stable receipt.
    ///
    /// # Errors
    /// Rejects outstanding shared work, invalid credentials, and corrupt state.
    pub fn depart_remote_member(
        &self,
        node_credential: &str,
        now: i64,
    ) -> Result<FederationDepartureReceipt, ApplicationError> {
        self.store
            .depart_federation_member(node_credential, now)
            .map_err(Into::into)
    }

    /// Applies the Keeper-signed receipt and returns this installation to a
    /// personal Hive without deleting private work or integrations.
    ///
    /// # Errors
    /// Rejects invalid receipts, outstanding local work, and corrupt state.
    pub fn apply_departure(
        &self,
        receipt: &FederationDepartureReceipt,
        now: i64,
    ) -> Result<LocalApiaryContext, ApplicationError> {
        self.store
            .apply_federation_departure(receipt, now)
            .map_err(Into::into)
    }

    /// Requests an explicit retry by the existing owned synchronization runner.
    ///
    /// # Errors
    /// Rejects non-Members, invalid time, and persistence failures.
    pub fn request_federation_sync_retry(
        &self,
        now: i64,
    ) -> Result<FederationSyncHealth, ApplicationError> {
        self.store
            .request_federation_sync_retry(now)
            .map_err(Into::into)
    }

    /// Records a successful reconciliation outcome for the bounded Member runner.
    ///
    /// # Errors
    /// Rejects non-Members, invalid time, and persistence failures.
    pub fn record_federation_sync_success(
        &self,
        now: i64,
    ) -> Result<FederationSyncHealth, ApplicationError> {
        self.store
            .record_federation_sync_success(now)
            .map_err(Into::into)
    }

    /// Records a classified reconciliation failure for the bounded Member runner.
    ///
    /// # Errors
    /// Rejects non-Members, invalid classifications/time, and persistence failures.
    pub fn record_federation_sync_failure(
        &self,
        condition: FederationSyncCondition,
        now: i64,
    ) -> Result<FederationSyncHealth, ApplicationError> {
        self.store
            .record_federation_sync_failure(condition, now)
            .map_err(Into::into)
    }

    /// Verifies and durably acknowledges one signed Keeper catalog locally.
    /// This does not contact Jira or claim project readiness.
    ///
    /// # Errors
    /// Rejects non-Members, invalid or stale snapshots, expired membership,
    /// identity mismatch, and unavailable persistence.
    pub fn acknowledge_federation_catalog(
        &self,
        snapshot: &FederationCatalogSnapshot,
        now: i64,
    ) -> Result<FederationCatalogAcknowledgement, ApplicationError> {
        self.store
            .acknowledge_federation_catalog(snapshot, now)
            .map_err(Into::into)
    }

    /// Returns the latest locally verified catalog evidence, if any.
    ///
    /// # Errors
    /// Returns an error when durable state is unavailable or corrupt.
    pub fn federation_catalog_acknowledgement(
        &self,
    ) -> Result<Option<FederationCatalogAcknowledgement>, ApplicationError> {
        self.store
            .federation_catalog_acknowledgement()
            .map_err(Into::into)
    }

    /// Derives current Member-local convergence for the latest verified
    /// Keeper catalog from private Jira and local policy evidence.
    ///
    /// # Errors
    /// Rejects personal Hives and corrupt or unavailable durable state.
    pub fn federation_catalog_readiness(
        &self,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<FederationCatalogReadiness, ApplicationError> {
        let context = self.store.local_apiary_context()?;
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            return Err(ApplicationError::Store(
                TaskStoreError::InvalidFederationCatalog,
            ));
        };
        let acknowledgement = self.store.federation_catalog_acknowledgement()?;
        let projects = self.store.acknowledged_federation_project_readiness()?;
        Ok(FederationCatalogReadiness::evaluate(
            acknowledgement,
            apiary.policy_revision(),
            jira_connection,
            projects,
            now,
        ))
    }

    /// Lists the public Hive/operator identities registered in this Apiary.
    ///
    /// # Errors
    /// Rejects personal Hives and unavailable persistence.
    pub fn members(&self) -> Result<Vec<ApiaryMemberSummary>, ApplicationError> {
        if matches!(
            self.store.local_apiary_context()?,
            LocalApiaryContext::Federated {
                local_role: LocalApiaryRole::Member,
                ..
            }
        ) && let Some(directory) = self.store.local_federation_directory()?
        {
            let local = self.store.local_hive_identity()?;
            let local_profile = self.store.local_public_hive_profile()?;
            return Ok(directory
                .entries
                .into_iter()
                .map(|entry| {
                    let is_local = entry.identity.hive_id == local.hive.id;
                    let profile = if is_local {
                        local_profile.profile.clone()
                    } else {
                        entry.profile
                    };
                    ApiaryMemberSummary {
                        hive_id: entry.identity.hive_id,
                        hive_name: profile.hive_name,
                        operator_id: entry.identity.operator_id,
                        operator_display_name: profile.operator_display_name,
                        operator_email: profile.contact_email,
                        role: entry.role,
                        is_local,
                    }
                })
                .collect());
        }
        self.store.list_apiary_members().map_err(Into::into)
    }

    /// Lists active Keeper-owned Steward delegations for this Apiary.
    ///
    /// # Errors
    /// Rejects personal and Member Hives and unavailable persistence.
    pub fn stewardships(&self) -> Result<Vec<Stewardship>, ApplicationError> {
        let LocalApiaryContext::Federated { apiary, local_role } =
            self.store.local_apiary_context()?
        else {
            return Err(TaskStoreError::ApiaryKeeperRequired.into());
        };
        if local_role != LocalApiaryRole::Keeper {
            return Err(TaskStoreError::ApiaryKeeperRequired.into());
        }
        self.store
            .stewardships_for_apiary(apiary.id)
            .map_err(Into::into)
    }

    /// Atomically creates or replaces one explicit Steward delegation.
    ///
    /// # Errors
    /// Rejects non-Keepers, foreign/empty scope, unsafe capabilities, invalid
    /// time, and persistence failures.
    pub fn set_stewardship(
        &self,
        steward_operator_id: OperatorId,
        managed_hive_ids: &[HiveId],
        capabilities: &[StewardCapability],
        now: i64,
    ) -> Result<Stewardship, ApplicationError> {
        self.store
            .set_stewardship(steward_operator_id, managed_hive_ids, capabilities, now)
            .map_err(Into::into)
    }

    /// Revokes one active delegation while preserving its audit identity.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown delegations, invalid time, and persistence failures.
    pub fn revoke_stewardship(
        &self,
        stewardship_id: StewardshipId,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .revoke_stewardship(stewardship_id, now)
            .map_err(Into::into)
    }

    /// Issues a one-day signed public connection card for deliberate sharing
    /// with a Keeper. Generating a card grants no membership or authority.
    ///
    /// # Errors
    /// Returns a persistence error when the durable local node identity cannot
    /// be created or reconstituted.
    pub fn connection_card(&self, now: i64) -> Result<HiveConnectionCard, ApplicationError> {
        self.store
            .issue_hive_connection_card(now, 24 * 60 * 60)
            .map_err(Into::into)
    }

    /// Creates one short-lived Keeper invitation URL capability. The secret is
    /// returned only in this result and is never exposed by later list calls.
    ///
    /// # Errors
    /// Rejects non-Keepers, invalid public endpoints, capability exhaustion,
    /// and persistence failures.
    pub fn create_join_link(
        &self,
        keeper_endpoint: &str,
        now: i64,
    ) -> Result<ApiaryJoinLinkBundle, ApplicationError> {
        self.store
            .issue_apiary_join_link(keeper_endpoint, now, 24 * 60 * 60)
            .map_err(Into::into)
    }

    /// Lists Keeper-side bootstrap state without returning any bearer secret.
    ///
    /// # Errors
    /// Rejects non-Keepers and unavailable persistence.
    pub fn join_links(&self, now: i64) -> Result<Vec<ApiaryJoinLink>, ApplicationError> {
        self.store.apiary_join_links(now).map_err(Into::into)
    }

    /// Cancels one invitation link until its signed invitation has been
    /// delivered to the receiving Hive.
    ///
    /// # Errors
    /// Rejects non-Keepers and links that are expired, revoked, or already
    /// delivered.
    pub fn revoke_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        now: i64,
    ) -> Result<ApiaryJoinLink, ApplicationError> {
        self.store
            .revoke_apiary_join_link(link_id, now)
            .map_err(Into::into)
    }

    /// Verifies the signed member identity presented through one join link and
    /// binds the capability to that exact Hive pending Keeper approval.
    ///
    /// # Errors
    /// Rejects invalid capabilities, identity substitution, invalid cards,
    /// and unavailable persistence.
    pub fn present_join_link_identity(
        &self,
        link_id: ApiaryJoinLinkId,
        secret: &str,
        card: &HiveConnectionCard,
        now: i64,
    ) -> Result<ApiaryJoinLink, ApplicationError> {
        self.store
            .present_apiary_join_link_identity(link_id, secret, card, now)
            .map_err(Into::into)
    }

    /// Records explicit Keeper approval for one exact pending Hive identity.
    ///
    /// # Errors
    /// Rejects non-Keepers, unbound/resolved links, and persistence failures.
    pub fn approve_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        now: i64,
    ) -> Result<ApiaryJoinLink, ApplicationError> {
        self.store
            .approve_apiary_join_link(link_id, now)
            .map_err(Into::into)
    }

    /// Polls one Keeper capability from the member side. Invitation material
    /// remains absent until explicit approval and is retry-stable afterward.
    ///
    /// # Errors
    /// Rejects invalid or expired bearer material and corrupt durable state.
    pub fn poll_join_link(
        &self,
        link_id: ApiaryJoinLinkId,
        secret: &str,
        now: i64,
    ) -> Result<ApiaryJoinLinkPoll, ApplicationError> {
        self.store
            .poll_apiary_join_link(link_id, secret, now)
            .map_err(Into::into)
    }

    /// Saves a Keeper URL capability privately on this personal Hive so local
    /// server-side polling survives browser reloads and device changes.
    ///
    /// # Errors
    /// Rejects malformed, duplicate, or non-personal-Hive capabilities.
    pub fn save_keeper_link(
        &self,
        link_id: ApiaryJoinLinkId,
        keeper_endpoint: &str,
        secret: &str,
        now: i64,
    ) -> Result<ApiaryKeeperLink, ApplicationError> {
        self.store
            .save_local_apiary_keeper_link(link_id, keeper_endpoint, secret, now)
            .map_err(Into::into)
    }

    /// Records the operator's submission of the verified Keeper disclosure.
    /// Nothing is sent remotely until both capability and consent are durable.
    ///
    /// # Errors
    /// Rejects altered offers, conflicting saved capabilities, expired terms,
    /// identity changes, or unavailable storage.
    pub fn begin_consented_enrollment(
        &self,
        offer: &swarm_domain::ApiaryEnrollmentOffer,
        secret: &str,
        now: i64,
    ) -> Result<swarm_domain::ApiaryEnrollment, ApplicationError> {
        swarm_persistence::verify_apiary_enrollment_offer(offer, now)?;
        let payload = &offer.payload;
        let local = self.connection_card(now)?;
        let existing = self
            .store
            .apiary_enrollments()?
            .into_iter()
            .find(|record| record.consent.link_id == payload.link_id);
        let consent = swarm_domain::ApiaryEnrollmentConsent {
            link_id: payload.link_id,
            apiary_id: payload.apiary_id,
            keeper_node_id: payload.keeper.payload.node_id,
            member_node_id: local.payload.node_id,
            member_hive_id: local.payload.hive_id,
            member_operator_id: local.payload.operator_id,
            policy_revision: payload.policy_revision,
            accepted_at: existing
                .as_ref()
                .map_or(now, |record| record.consent.accepted_at),
            expires_at: payload.expires_at,
        };
        match self
            .store
            .local_apiary_keeper_link_credential(payload.link_id)
        {
            Ok((endpoint, saved_secret)) => {
                if endpoint != payload.keeper_endpoint || saved_secret != secret {
                    return Err(TaskStoreError::InvalidApiaryJoinLink.into());
                }
            }
            Err(TaskStoreError::ApiaryJoinLinkNotFound) => {
                self.save_keeper_link(payload.link_id, &payload.keeper_endpoint, secret, now)?;
            }
            Err(error) => return Err(error.into()),
        }
        self.store
            .save_apiary_enrollment(&consent, now)
            .map_err(Into::into)
    }

    /// Advances the imported approval using only the consent already recorded.
    ///
    /// # Errors
    /// Rejects cancelled, changed, expired, or unrelated enrollment material.
    pub fn prepare_consented_join(
        &self,
        link_id: ApiaryJoinLinkId,
        invitation_id: ApiaryInvitationId,
        now: i64,
    ) -> Result<swarm_domain::ApiaryEnrollment, ApplicationError> {
        self.store
            .prepare_consented_apiary_join(link_id, invitation_id, now)
            .map_err(Into::into)
    }

    /// Lists the bounded member enrollment journal.
    /// # Errors
    /// Returns storage or integrity errors.
    pub fn enrollments(&self) -> Result<Vec<swarm_domain::ApiaryEnrollment>, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        Ok(self
            .store
            .apiary_enrollments()?
            .into_iter()
            .filter(|record| {
                record.phase != swarm_domain::ApiaryEnrollmentPhase::Cancelled
                    && (record.phase != swarm_domain::ApiaryEnrollmentPhase::Complete
                        || identity.hive.apiary_id == Some(record.consent.apiary_id))
            })
            .collect())
    }

    /// Records a classified transport outcome without exposing remote text.
    /// # Errors
    /// Returns storage or integrity errors.
    pub fn record_enrollment_attempt(
        &self,
        link_id: ApiaryJoinLinkId,
        problem: Option<swarm_domain::ApiaryEnrollmentProblem>,
        problem_code: Option<String>,
        now: i64,
    ) -> Result<(), ApplicationError> {
        self.store
            .record_apiary_enrollment_attempt(link_id, problem, problem_code, now)
            .map_err(Into::into)
    }

    /// Returns at most four unfinished enrollments for one transport pass.
    /// Receipt recovery precedes expiry so a saved membership is never lost.
    /// # Errors
    /// Returns storage or integrity errors without inferring membership.
    pub fn pending_enrollments(
        &self,
        now: i64,
    ) -> Result<Vec<swarm_domain::ApiaryEnrollment>, ApplicationError> {
        use swarm_domain::ApiaryEnrollmentPhase::{AwaitingApproval, Joining};
        let mut pending = Vec::new();
        for record in self.enrollments()? {
            if !matches!(record.phase, AwaitingApproval | Joining)
                || self.finish_consented_join(record.consent.link_id)?
            {
                continue;
            }
            if record.consent.expires_at <= now {
                self.record_enrollment_attempt(
                    record.consent.link_id,
                    Some(swarm_domain::ApiaryEnrollmentProblem::InvitationUnavailable),
                    // Classified, so there is nothing unexplained to record.
                    None,
                    now,
                )?;
            } else if pending.len() < 4 && record.next_attempt_at.is_none_or(|next| next <= now) {
                pending.push(record);
            }
        }
        Ok(pending)
    }

    /// Advances a phase without inferring any new authority.
    /// # Errors
    /// Rejects stale or invalid phase changes.
    pub fn advance_enrollment(
        &self,
        link_id: ApiaryJoinLinkId,
        expected: swarm_domain::ApiaryEnrollmentPhase,
        next: swarm_domain::ApiaryEnrollmentPhase,
    ) -> Result<swarm_domain::ApiaryEnrollment, ApplicationError> {
        self.store
            .advance_apiary_enrollment(link_id, expected, next)
            .map_err(Into::into)
    }

    /// Finishes only after the verified membership receipt has been applied.
    /// # Errors
    /// Returns storage or integrity errors.
    pub fn finish_consented_join(
        &self,
        link_id: ApiaryJoinLinkId,
    ) -> Result<bool, ApplicationError> {
        self.store
            .finish_consented_apiary_join(link_id)
            .map_err(Into::into)
    }

    /// Lists pending outbound Keeper connections without exposing secrets.
    ///
    /// # Errors
    /// Returns an error when local persistence is unavailable.
    pub fn keeper_links(&self) -> Result<Vec<ApiaryKeeperLink>, ApplicationError> {
        self.store.local_apiary_keeper_links().map_err(Into::into)
    }

    /// Loads one private endpoint and bearer secret for server-side transport.
    /// This method must never feed a browser response.
    ///
    /// # Errors
    /// Rejects unknown links and corrupt local state.
    pub fn keeper_link_credential(
        &self,
        link_id: ApiaryJoinLinkId,
    ) -> Result<(String, String), ApplicationError> {
        self.store
            .local_apiary_keeper_link_credential(link_id)
            .map_err(Into::into)
    }

    /// Saves the latest signed Keeper response metadata without changing the
    /// locally pinned endpoint or bearer capability.
    ///
    /// # Errors
    /// Rejects endpoint substitution and persistence failures.
    pub fn record_keeper_link_poll(
        &self,
        remote: &ApiaryJoinLink,
        now: i64,
    ) -> Result<ApiaryKeeperLink, ApplicationError> {
        self.store
            .update_local_apiary_keeper_link(remote, now)
            .map_err(Into::into)
    }

    /// Removes one completed local bootstrap after its invitation is durable.
    ///
    /// # Errors
    /// Rejects unknown links and unavailable persistence.
    pub fn remove_keeper_link(&self, link_id: ApiaryJoinLinkId) -> Result<(), ApplicationError> {
        self.store
            .remove_local_apiary_keeper_link(link_id)
            .map_err(Into::into)
    }

    /// Verifies and pins one deliberately imported connection card for the
    /// local Keeper. This records identity only; membership and authority stay
    /// unchanged until the later invitation handshake succeeds.
    ///
    /// # Errors
    /// Rejects invalid cards, non-Keepers, identity conflicts, and persistence failures.
    pub fn pin_hive_candidate(
        &self,
        card: &HiveConnectionCard,
        now: i64,
    ) -> Result<ApiaryHiveCandidate, ApplicationError> {
        self.store.pin_hive_candidate(card, now).map_err(Into::into)
    }

    /// Lists the current Keeper's pinned Hive identities without treating them
    /// as members or invitation recipients.
    ///
    /// # Errors
    /// Rejects personal/member Hives and unavailable persistence.
    pub fn hive_candidates(&self) -> Result<Vec<ApiaryHiveCandidate>, ApplicationError> {
        self.store.list_hive_candidates().map_err(Into::into)
    }

    /// Lists pinned identities together with the durable invitation state that
    /// determines whether another one-time bundle may be issued.
    ///
    /// # Errors
    /// Rejects non-Keepers and unavailable persistence.
    pub fn hive_candidate_overviews(
        &self,
        now: i64,
    ) -> Result<Vec<ApiaryHiveCandidateOverview>, ApplicationError> {
        self.store
            .list_hive_candidates()?
            .into_iter()
            .map(|candidate| {
                let invitation_pending = self
                    .store
                    .pending_federation_invitation_count(candidate.hive_id, now)?
                    > 0;
                Ok(ApiaryHiveCandidateOverview {
                    candidate,
                    invitation_pending,
                })
            })
            .collect()
    }

    /// Issues one signed, one-time invitation for a Keeper-pinned Hive. The
    /// bearer secret is returned only once and only its digest remains durable.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown candidates, invalid endpoint configuration,
    /// duplicate pending invitations, and persistence failures.
    pub fn invite_hive_candidate(
        &self,
        invited_hive_id: HiveId,
        keeper_endpoint: &str,
        now: i64,
    ) -> Result<ApiaryInvitationBundle, ApplicationError> {
        self.store
            .issue_apiary_invitation_bundle(invited_hive_id, keeper_endpoint, now, 24 * 60 * 60)
            .map_err(Into::into)
    }

    /// Verifies and durably imports a signed invitation for this exact personal
    /// Hive. This pins Keeper identity only; policy and membership remain
    /// separate explicit steps.
    ///
    /// # Errors
    /// Rejects invalid, expired, misaddressed, duplicate, or unsupported
    /// invitations and non-personal Hives.
    pub fn import_invitation(
        &self,
        bundle: &ApiaryInvitationBundle,
        now: i64,
    ) -> Result<FederationJoinInvitation, ApplicationError> {
        self.store
            .import_apiary_invitation_bundle(bundle, now)
            .map_err(Into::into)
    }

    /// Lists current imported invitations without exposing the one-time secret,
    /// pinned public key, or complete signed envelope.
    ///
    /// # Errors
    /// Returns a persistence error when private invitation state is unavailable.
    pub fn imported_invitations(
        &self,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<Vec<FederationJoinInvitationOverview>, ApplicationError> {
        let hive = self.store.local_hive_identity()?.hive;
        self.store
            .federation_join_invitations(now)?
            .into_iter()
            .map(|invitation| {
                let projects = self
                    .store
                    .federation_project_readiness(invitation.invitation_id)?;
                let readiness = FederationJoinReadiness::evaluate(
                    &hive,
                    &invitation,
                    jira_connection,
                    projects,
                    now,
                );
                Ok(FederationJoinInvitationOverview {
                    invitation,
                    readiness,
                })
            })
            .collect()
    }

    /// Acknowledges the exact policy revision from a current imported
    /// invitation, then returns freshly derived local readiness. No Keeper is
    /// contacted and no membership is granted.
    ///
    /// # Errors
    /// Rejects a stale revision, expired/resolved invitation, identity or
    /// membership mismatch, or unavailable local evidence.
    pub fn accept_imported_policy(
        &self,
        invitation_id: ApiaryInvitationId,
        policy_revision: u64,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<FederationJoinInvitationOverview, ApplicationError> {
        self.store
            .accept_federation_join_policy(invitation_id, policy_revision, now)?;
        self.imported_invitations(jira_connection, now)?
            .into_iter()
            .find(|overview| overview.invitation.invitation_id == invitation_id)
            .ok_or(ApplicationError::Store(
                TaskStoreError::ApiaryInvitationNotFound,
            ))
    }

    /// Re-derives private local readiness and creates one durable signed
    /// submission for transport to the pinned Keeper. Exact retries return the
    /// same submission; no remote state is changed by this command.
    ///
    /// # Errors
    /// Rejects incomplete Jira/project readiness, stale policy or invitation
    /// state, expiry, membership drift, and persistence failures.
    pub fn prepare_imported_join_submission(
        &self,
        invitation_id: ApiaryInvitationId,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<FederationJoinSubmission, ApplicationError> {
        let overview = self
            .imported_invitations(jira_connection, now)?
            .into_iter()
            .find(|overview| overview.invitation.invitation_id == invitation_id)
            .ok_or(ApplicationError::Store(
                TaskStoreError::ApiaryInvitationNotFound,
            ))?;
        self.store
            .prepare_federation_join_submission(invitation_id, &overview.readiness, now)
            .map_err(Into::into)
    }

    /// Consumes one independently signed Hive submission on the Keeper and
    /// returns the durable signed membership receipt plus bounded credential.
    /// The one-time secret and node credential remain adapter-private.
    ///
    /// # Errors
    /// Rejects invalid signatures/secrets, stale policy/catalog identity,
    /// expiry, membership conflicts, and altered replays.
    pub fn consume_remote_join_submission(
        &self,
        submission: &FederationJoinSubmission,
        now: i64,
    ) -> Result<FederationJoinAcceptance, ApplicationError> {
        self.store
            .consume_federation_join_submission(submission, now)
            .map_err(Into::into)
    }

    /// Applies one Keeper-signed acceptance to the invited Hive only after the
    /// receipt and credential pass persistence-owned identity checks.
    ///
    /// # Errors
    /// Rejects invalid or unsolicited receipts, expired credentials,
    /// invitation drift, existing membership, and persistence failures.
    pub fn apply_remote_join_acceptance(
        &self,
        invitation_id: ApiaryInvitationId,
        acceptance: &FederationJoinAcceptance,
        now: i64,
    ) -> Result<LocalApiaryContext, ApplicationError> {
        self.store
            .apply_federation_join_acceptance(invitation_id, acceptance, now)
            .map_err(Into::into)
    }

    /// Creates one Apiary around the current personal Hive. The local operator
    /// becomes Keeper and backend selection is permanent.
    ///
    /// # Errors
    /// Rejects invalid input or a Hive that already belongs to an Apiary.
    pub fn create_from_personal_hive(
        &self,
        name: &str,
        backend: SharedWorkBackend,
        now: i64,
    ) -> Result<LocalApiaryContext, ApplicationError> {
        if backend != SharedWorkBackend::Jira {
            return Err(ApplicationError::SharedWorkBackendUnavailable);
        }
        self.store
            .create_apiary_for_local_hive(name, backend, now)
            .map_err(Into::into)
    }

    /// Renames the Hive owned by this installation without changing any
    /// membership, worker, task, repository, or federation identity.
    ///
    /// # Errors
    /// Rejects invalid public naming/time or unavailable persistence.
    pub fn rename_local_hive(
        &self,
        name: &str,
        now: i64,
    ) -> Result<swarm_domain::HiveIdentity, ApplicationError> {
        self.store.rename_local_hive(name, now).map_err(Into::into)
    }

    /// Reads the local owner's shareable labels, not membership authority.
    ///
    /// # Errors
    /// Returns unavailable or invalid local storage errors.
    pub fn local_public_profile(
        &self,
    ) -> Result<swarm_persistence::LocalPublicHiveProfile, ApplicationError> {
        self.store.local_public_hive_profile().map_err(Into::into)
    }

    /// Saves operator-provided public labels without inferring identity from
    /// private integrations or changing access to the Hive.
    ///
    /// # Errors
    /// Rejects invalid fields, time, or persistence failures.
    pub fn save_local_public_profile(
        &self,
        profile: &swarm_domain::PublicHiveProfile,
        now: i64,
    ) -> Result<swarm_persistence::LocalPublicHiveProfile, ApplicationError> {
        self.store
            .save_local_public_hive_profile(profile, now)
            .map_err(Into::into)
    }

    /// Saves the explicitly reviewed joining profile, replacing only My Hive.
    ///
    /// # Errors
    /// Rejects invalid profile fields or unavailable persistence.
    pub fn save_join_public_profile(
        &self,
        profile: swarm_domain::PublicHiveProfile,
        now: i64,
    ) -> Result<swarm_persistence::LocalPublicHiveProfile, ApplicationError> {
        self.save_local_public_profile(&profile.with_default_join_name(), now)
    }

    /// Renames the current Apiary public label. Only its Keeper can do this;
    /// backend, policy, membership, projects, and signed identity remain fixed.
    ///
    /// # Errors
    /// Rejects invalid input, a personal or Member Hive, or unavailable persistence.
    pub fn rename_local_apiary(
        &self,
        name: &str,
        now: i64,
    ) -> Result<LocalApiaryContext, ApplicationError> {
        self.store
            .rename_local_apiary(name, now)
            .map_err(Into::into)
    }

    /// Returns the persisted blockers that must be cleared before the current
    /// sole Keeper Hive may become personal again.
    ///
    /// # Errors
    /// Rejects a personal Hive, missing Apiary, or unavailable persistence.
    pub fn collapse_readiness(&self) -> Result<ApiaryCollapseReadiness, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryNotFound)?;
        self.store
            .apiary_collapse_readiness(apiary_id)
            .map_err(Into::into)
    }

    /// Re-derives collapse readiness inside the store transaction and converts
    /// the sole Keeper Apiary back into a personal Hive.
    ///
    /// # Errors
    /// Rejects non-Keepers, federation blockers, invalid time, or stale state.
    pub fn collapse(&self, now: i64) -> Result<LocalApiaryContext, ApplicationError> {
        self.store.collapse_local_apiary(now).map_err(Into::into)
    }

    /// Lists the current Apiary's authoritative promoted Jira catalog for this
    /// member Hive. Personal Hives do not have a shared catalog.
    ///
    /// # Errors
    /// Rejects a personal Hive or unavailable persistence.
    pub fn promoted_jira_projects(&self) -> Result<Vec<ApiaryJiraProject>, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        let apiary_id = identity
            .hive
            .apiary_id
            .ok_or(TaskStoreError::ApiaryNotFound)?;
        self.store
            .list_apiary_jira_projects(apiary_id)
            .map_err(Into::into)
    }

    /// Promotes one ready local Jira binding through a single Keeper command.
    /// Store-owned validation keeps catalog insertion and local scope conversion
    /// atomic so a partial promotion cannot be observed.
    ///
    /// # Errors
    /// Rejects non-Keepers, Native or personal Hives, incomplete Jira readiness,
    /// foreign bindings, invalid time, and unavailable persistence.
    pub fn promote_jira_binding(
        &self,
        binding_id: JiraProjectBindingId,
        now: i64,
    ) -> Result<ApiaryJiraProject, ApplicationError> {
        self.store
            .promote_local_jira_binding_to_apiary(binding_id, now)
            .map_err(Into::into)
    }

    /// Lists current invitations and derives readiness from durable state plus
    /// the integration adapter's current connection evidence.
    ///
    /// # Errors
    /// Returns a persistence error when any invitation evidence is unavailable.
    pub fn pending_invitations(
        &self,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<Vec<ApiaryInvitationOverview>, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        self.store
            .pending_apiary_invitations_for_hive(identity.hive.id, now)?
            .into_iter()
            .map(|invitation| self.overview(invitation, jira_connection, now))
            .collect()
    }

    /// Accepts the exact policy revision required by one current invitation as
    /// the local Hive operator and returns freshly derived readiness.
    ///
    /// # Errors
    /// Rejects stale revisions, foreign invitations, or unavailable evidence.
    pub fn accept_policy(
        &self,
        invitation_id: ApiaryInvitationId,
        policy_revision: u64,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<ApiaryInvitationOverview, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        let invitation = self.store.accept_apiary_policy(
            invitation_id,
            identity.operator.id,
            policy_revision,
            now,
        )?;
        self.overview(invitation, jira_connection, now)
    }

    /// Re-derives all readiness evidence at command time, then atomically joins
    /// the invited Apiary. A stale browser snapshot can never authorize joining.
    ///
    /// # Errors
    /// Rejects incomplete or stale readiness and persistence conflicts.
    pub fn join(
        &self,
        invitation_id: ApiaryInvitationId,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<LocalApiaryContext, ApplicationError> {
        let invitation = self.store.get_apiary_invitation(invitation_id)?;
        let overview = self.overview(invitation, jira_connection, now)?;
        self.store
            .accept_apiary_invitation(invitation_id, &overview.readiness, now)?;
        self.store.local_apiary_context().map_err(Into::into)
    }

    fn overview(
        &self,
        invitation: ApiaryInvitation,
        jira_connection: JiraConnectionState,
        now: i64,
    ) -> Result<ApiaryInvitationOverview, ApplicationError> {
        let identity = self.store.local_hive_identity()?;
        let apiary = self.store.get_apiary(invitation.apiary_id)?;
        let project_access = self.store.apiary_jira_project_access_ready(apiary.id)?;
        let readiness = ApiaryJoinReadiness::evaluate(
            &identity.hive,
            &apiary,
            Some(&invitation),
            apiary_join_checks(&apiary, jira_connection, project_access),
            now,
        );
        Ok(ApiaryInvitationOverview {
            invitation,
            apiary,
            readiness,
            jira_connection,
        })
    }
}

fn apiary_join_checks(
    apiary: &Apiary,
    jira_connection: JiraConnectionState,
    project_access: bool,
) -> ApiaryJoinChecks {
    let integration = if apiary.shared_work_backend() == SharedWorkBackend::Jira
        && jira_connection == JiraConnectionState::Ready
    {
        ApiaryJoinCheckState::Ready
    } else {
        ApiaryJoinCheckState::Blocked
    };
    ApiaryJoinChecks {
        identity: ApiaryJoinCheckState::Ready,
        integration,
        project_access: if project_access {
            ApiaryJoinCheckState::Ready
        } else {
            ApiaryJoinCheckState::Blocked
        },
        // The local store currently owns both ends of this protocol evidence.
        // Distributed transport will replace this with negotiated compatibility.
        protocol: ApiaryJoinCheckState::Ready,
    }
}

impl TaskService {
    #[must_use]
    pub const fn new(store: TaskStore) -> Self {
        Self { store }
    }

    #[must_use]
    pub const fn store(&self) -> &TaskStore {
        &self.store
    }

    /// Returns the effective local operator presence policy.
    ///
    /// # Errors
    /// Returns a persistence error when presence cannot be read.
    pub fn operator_presence(&self, now: i64) -> Result<OperatorPresence, ApplicationError> {
        self.store.operator_presence(now).map_err(Into::into)
    }

    /// Reads the optional operator-configured Night Watch schedule.
    ///
    /// # Errors
    /// Returns a persistence or stored-configuration error.
    pub fn night_watch_configuration(
        &self,
    ) -> Result<Option<swarm_persistence::NightWatchConfiguration>, ApplicationError> {
        self.store.night_watch_configuration().map_err(Into::into)
    }

    /// Saves a validated schedule without changing provider capabilities.
    ///
    /// # Errors
    /// Returns a persistence error; settings and event writes are atomic.
    pub fn set_night_watch_configuration(
        &self,
        config: &swarm_persistence::NightWatchConfiguration,
    ) -> Result<bool, ApplicationError> {
        self.store
            .set_night_watch_configuration(config)
            .map_err(Into::into)
    }

    /// Sets or clears the operator's explicit presence override.
    ///
    /// # Errors
    /// Returns a persistence error when presence cannot be updated atomically.
    pub fn set_operator_presence(
        &self,
        mode: Option<PresenceMode>,
        now: i64,
    ) -> Result<(OperatorPresence, bool), ApplicationError> {
        let mutation = self.store.set_manual_presence(mode, now)?;
        Ok((mutation.presence, mutation.changed))
    }

    /// Records one authenticated client observation for derived presence.
    ///
    /// # Errors
    /// Returns a capacity or persistence error.
    pub fn observe_operator_device(
        &self,
        device_id: PresenceDeviceId,
        device_class: PresenceDeviceClass,
        state: PresenceObservationState,
        desktop_return: bool,
        now: i64,
    ) -> Result<(OperatorPresence, bool), ApplicationError> {
        let mutation = self.store.record_presence_observation_with_return(
            device_id,
            device_class,
            state,
            desktop_return,
            now,
        )?;
        Ok((mutation.presence, mutation.changed))
    }
    /// Lists the complete local Hive queue for an operator or Queen coordinator.
    ///
    /// # Errors
    /// Returns a persistence error when the task snapshot cannot be read.
    pub fn list_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_tasks().map_err(Into::into)
    }

    /// Queen owns cross-worker prerequisites; workers must ask her to route them.
    ///
    /// # Errors
    /// Denies ordinary workers and propagates atomic graph validation failures.
    pub fn change_task_prerequisite(
        &self,
        principal: AgentPrincipal,
        change: &swarm_domain::TaskPrerequisiteChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        self.apply_prerequisite_change(&TaskActivityActor::worker(principal.worker_id), change, now)
    }

    /// Apply an authenticated operator's explicit prerequisite change.
    ///
    /// # Errors
    /// Propagates atomic graph validation and persistence failures.
    pub fn change_operator_task_prerequisite(
        &self,
        change: &swarm_domain::TaskPrerequisiteChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        self.apply_prerequisite_change(&TaskActivityActor::operator(), change, now)
    }

    fn apply_prerequisite_change(
        &self,
        actor: &TaskActivityActor,
        change: &swarm_domain::TaskPrerequisiteChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        match change.operation {
            swarm_domain::PrerequisiteOperation::Add => self.store.add_task_prerequisite(
                change.task_id,
                change.prerequisite_id,
                &change.reason,
                actor,
                now,
            )?,
            swarm_domain::PrerequisiteOperation::Remove => self.store.remove_task_prerequisite(
                change.task_id,
                change.prerequisite_id,
                &change.reason,
                actor,
                now,
            )?,
        }
        self.store.get_task(change.task_id).map_err(Into::into)
    }

    /// Queen records explicit shared operator gates without sharing permissions.
    ///
    /// # Errors
    /// Refuses ordinary workers, stale evidence and invalid blocker links.
    pub fn change_task_decision_link(
        &self,
        principal: AgentPrincipal,
        change: &swarm_domain::TaskDecisionLinkChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        self.apply_decision_link_change(
            &TaskActivityActor::worker(principal.worker_id),
            change,
            now,
        )
    }

    /// Apply an authenticated operator's explicit shared decision blocker change.
    ///
    /// # Errors
    /// Refuses stale evidence and invalid blocker links atomically.
    pub fn change_operator_task_decision_link(
        &self,
        change: &swarm_domain::TaskDecisionLinkChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        self.apply_decision_link_change(&TaskActivityActor::operator(), change, now)
    }

    fn apply_decision_link_change(
        &self,
        actor: &TaskActivityActor,
        change: &swarm_domain::TaskDecisionLinkChange,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        match change.operation {
            swarm_domain::TaskDecisionLinkOperation::Add => self.store.add_task_decision_link(
                change.task_id,
                change.decision_id,
                &change.reason,
                &change.expected_evidence_revision,
                actor,
                now,
            )?,
            swarm_domain::TaskDecisionLinkOperation::Remove => {
                self.store.remove_task_decision_link(
                    change.task_id,
                    change.decision_id,
                    &change.reason,
                    &change.expected_evidence_revision,
                    actor,
                    now,
                )?;
            }
        }
        self.store.get_task(change.task_id).map_err(Into::into)
    }

    /// Settled work, which the board fetches once rather than on every poll.
    ///
    /// # Errors
    /// Returns a persistence error when the board cannot be read.
    pub fn list_settled_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_settled_tasks().map_err(Into::into)
    }

    /// The browser board's working set: everything except settled work.
    ///
    /// # Errors
    /// Returns a persistence error when the board cannot be read.
    pub fn list_board_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_board_tasks().map_err(Into::into)
    }

    /// Lists the bounded recovery shelf for local, non-Jira work.
    ///
    /// # Errors
    /// Returns a persistence error when the recovery snapshot cannot be read.
    pub fn list_removed_local_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_removed_local_tasks().map_err(Into::into)
    }

    /// Creates a validated local draft through the shared application boundary.
    ///
    /// # Errors
    /// Propagates validation or persistence failures.
    pub fn create_operator_task(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, ApplicationError> {
        self.store
            .create_task_with_details_as(
                title,
                description,
                priority,
                workspace,
                &TaskActivityActor::operator(),
            )
            .map_err(Into::into)
    }

    /// Updates supplied task details through the shared application boundary.
    ///
    /// # Errors
    /// Propagates validation and persistence failures.
    pub fn update_operator_task(
        &self,
        task_id: TaskId,
        update: &swarm_domain::TaskDetailsUpdate,
    ) -> Result<Task, ApplicationError> {
        self.store
            .update_task_details_as(task_id, update, &TaskActivityActor::operator())
            .map_err(Into::into)
    }
    /// Removes one task from the active Hive while retaining its source and audit history.
    ///
    /// # Errors
    /// Propagates lifecycle and persistence failures.
    pub fn remove_operator_task(&self, task_id: TaskId) -> Result<(), ApplicationError> {
        self.store
            .remove_task_as(task_id, &TaskActivityActor::operator(), "")
            .map_err(Into::into)
    }

    /// Moves one open task to the front of the delivery order.
    ///
    /// Queen's lever for urgency, and she had none. Delivery orders by
    /// `position`, never by `priority` — she set HIGH on a task, watched it sit
    /// eight deep because she filed it last, and the only way to move it
    /// forward was to Block something else, which makes the board lie about why
    /// that work is waiting.
    ///
    /// Deliberately a promote rather than a reorder. A full reorder needs the
    /// complete open set in order and can corrupt it if the caller gets the
    /// list wrong; "put this first" cannot.
    ///
    /// # Errors
    /// Denies a non-Queen caller, and propagates the reordering rules.
    pub fn promote_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<(), ApplicationError> {
        require_queen(principal)?;
        self.store.promote_open_task(task_id)?;
        Ok(())
    }

    /// Records why a reviewer does not consider shipped work finished.
    ///
    /// Queen's only lever for "shipped but not done". Before this there was
    /// none: a task already in Review cannot be transitioned to Review, and
    /// nothing else annotates a task, so a hold lived only in prose between
    /// sessions and vanished when the shipped-work sweep closed the task.
    ///
    /// The hold does not stop the sweep, on the operator's ruling — closing
    /// shipped work without a human round trip is what makes unattended running
    /// possible. It makes the reason survive the close.
    ///
    /// # Errors
    /// Denies a non-Queen caller and propagates persistence failures.
    pub fn hold_reviewed_work(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        reason: &str,
        now: i64,
    ) -> Result<(), ApplicationError> {
        require_queen(principal)?;
        self.store
            .hold_reviewed_work(
                task_id,
                &TaskActivityActor::worker(principal.worker_id),
                reason,
                now,
            )
            .map_err(Into::into)
    }

    /// Withdraws a hold, so the work closes with nothing to explain.
    ///
    /// # Errors
    /// Denies a non-Queen caller and propagates persistence failures.
    pub fn release_reviewed_work_hold(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<bool, ApplicationError> {
        require_queen(principal)?;
        self.store
            .release_reviewed_work_hold(task_id)
            .map_err(Into::into)
    }

    /// Retires a task Queen has been told should not exist any more.
    ///
    /// Soft-delete, per the operator's ruling: the row keeps its history and
    /// leaves the live board, rather than becoming a sixth lifecycle state that
    /// every query, filter and summary has to learn.
    ///
    /// This existed for the operator and not for Queen, so an operator ruling
    /// of "retire these four" could only be carried out by moving them to
    /// Blocked with a note saying they were retired — a lie on the board, since
    /// Blocked means work waiting on something and the next reader will try to
    /// unblock it. Worse, drafts are now part of Queen's review set, so a
    /// retired-but-undeletable task would be re-triaged on every future review,
    /// forever.
    ///
    /// # Errors
    /// Denies a worker caller, and propagates lifecycle and persistence
    /// failures — including the refusal to retire Active or Review work, which
    /// must be moved out of flight first.
    pub fn retire_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        if principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        self.store
            .remove_task_as(
                task_id,
                &TaskActivityActor::worker(principal.worker_id),
                reason,
            )
            .map_err(Into::into)
    }

    /// Returns one removed local task to the active Hive board.
    ///
    /// # Errors
    /// Propagates missing-task, Jira-authority, and persistence failures.
    pub fn restore_operator_task(&self, task_id: TaskId) -> Result<Task, ApplicationError> {
        self.store
            .restore_task_as(task_id, &TaskActivityActor::operator())
            .map_err(Into::into)
    }
    /// The operator releasing work they had parked on their own action.
    ///
    /// # Errors
    /// Refuses a task that is not parked; propagates persistence failures.
    pub fn lift_operator_park(&self, task_id: TaskId) -> Result<Task, ApplicationError> {
        self.store.lift_operator_park(task_id)?;
        self.store.get_task(task_id).map_err(Into::into)
    }

    /// Assigns a local task to a stable worker profile.
    ///
    /// # Errors
    /// Propagates task lifecycle and persistence failures.
    pub fn assign_operator_task(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
    ) -> Result<Task, ApplicationError> {
        self.store
            .assign_task_to_worker_as(task_id, worker_id, &TaskActivityActor::operator())
            .map_err(Into::into)
    }

    /// Returns an operator task to the unassigned Hive queue.
    ///
    /// # Errors
    ///
    /// Propagates task validation and persistence failures.
    pub fn unassign_operator_task(&self, task_id: TaskId) -> Result<Task, ApplicationError> {
        self.store
            .unassign_task_as(task_id, &TaskActivityActor::operator())
            .map_err(Into::into)
    }

    /// Applies one domain-valid task transition for the operator or Queen.
    ///
    /// # Errors
    /// Propagates lifecycle and persistence failures.
    pub fn transition_operator_task(
        &self,
        task_id: TaskId,
        target: TaskState,
    ) -> Result<Task, ApplicationError> {
        self.transition_operator_task_with_note(task_id, target, "")
    }
    /// Applies one domain-valid task transition with an optional audit note.
    ///
    /// # Errors
    /// Propagates lifecycle, note validation, and persistence failures.
    pub fn transition_operator_task_with_note(
        &self,
        task_id: TaskId,
        target: TaskState,
        note: &str,
    ) -> Result<Task, ApplicationError> {
        require_completion_evidence(&self.store, target, task_id, note)?;
        self.store
            .transition_task_with_note_as(task_id, target, note, &TaskActivityActor::operator())
            .map_err(Into::into)
    }
    /// Lists the work visible to an agent. Queen sees the Hive queue; workers see only their
    /// current session assignment.
    ///
    /// # Errors
    /// Returns a persistence error when the task snapshot cannot be read.
    pub fn list_visible_tasks(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<Task>, ApplicationError> {
        let tasks = self.list_tasks()?;
        if principal.role == WorkerRole::Queen {
            return Ok(tasks);
        }
        Ok(principal
            .active_session_id
            .map_or_else(Vec::new, |session_id| {
                tasks
                    .into_iter()
                    .filter(|task| {
                        task.assigned_worker_id == Some(principal.worker_id)
                            && task.assigned_session_id == Some(session_id)
                            && task.state != TaskState::Completed
                    })
                    .collect()
            }))
    }

    /// Every task a principal may READ, which is not the same set as the ones
    /// it may act on.
    ///
    /// ⚠️ READ AND ACT WERE ONE GATE, AND THAT IS THE WHOLE DEFECT THIS EXISTS
    /// TO AVOID. `list_visible_tasks` answers "what may this agent see", and
    /// `visible_task_id` — the check every transition, note and evidence record
    /// runs through — answers "what may this agent touch" by calling it. Widening
    /// that one function to give a worker board-wide READING would have handed it
    /// board-wide WRITING in the same line, silently, and nothing in the type
    /// system would have said so. So reading gets its own function and every
    /// mutating path keeps calling the old one.
    ///
    /// Queen sees everything because she may move everything. A `board_read`
    /// worker sees everything and may still move only its own assignment.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn list_tasks_readable_by(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<Task>, ApplicationError> {
        // Read from the store rather than from the principal, so revoking the
        // capability takes effect on the next call instead of when a long-lived
        // session happens to end.
        let reads_the_board = principal.role == WorkerRole::Queen
            || self
                .store
                .get_worker_profile(principal.worker_id)
                .is_ok_and(|profile| profile.board_read);
        if reads_the_board {
            return self.list_tasks();
        }
        self.list_visible_tasks(principal)
    }

    /// Whether this principal may READ one task, by either route.
    ///
    /// ⚠️ ONE TOOL, FOUR GATES. `swarm_read_task_history` calls four separate
    /// application methods — the activity log, the evidence record, the message
    /// exchange and the returned-review request — and each carried its own copy
    /// of "is this yours". Widening the first and stopping there produced a
    /// board reader that got an authorisation error from a tool it had just
    /// been granted, which is worse than a plain refusal: the capability looks
    /// broken rather than absent. This is the single answer all four now ask.
    ///
    /// It is a READ check. Nothing here decides what may be changed.
    ///
    /// # Errors
    /// Returns `NotAuthorized` when the task is neither readable nor this
    /// worker's own.
    fn may_read_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<(), ApplicationError> {
        if self
            .list_tasks_readable_by(principal)?
            .iter()
            .any(|task| task.id == task_id)
        {
            return Ok(());
        }
        // A task the board has NAMED as this worker's own blocker is readable.
        // Seeing that you are blocked, being told which task you wait on, and
        // then being refused that task is the prerequisite primitive working
        // against the worker it exists to inform.
        if self
            .store
            .worker_waits_on_task(principal.worker_id, task_id)
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.task_this_worker_finished(principal, task_id)?;
        Ok(())
    }

    /// The task this worker owns, INCLUDING one it has just finished.
    ///
    /// `list_visible_tasks` hides completed work from a worker on purpose: its
    /// assignment is what it may act on, and finished work is not that. But
    /// `swarm_draft_email_reply` requires the task to be completed with its
    /// deployment recorded — and recording a deployment completes the task. So
    /// the precondition the tool demands was the exact condition that removed
    /// the task from the caller's reach, and no ordering existed that worked:
    /// draft first and it is not yet completed, complete first and it is gone.
    /// A worker following its own dispatch instruction got "not authorized",
    /// which reads as a deliberate permission rather than a trap.
    ///
    /// Deliberately narrow. Same ownership rule as visibility — this worker,
    /// this session — with only the completed clause lifted, so it reaches the
    /// task the caller just finished and nothing else. Widening visibility
    /// generally would have been the wrong fix: workers seeing only their
    /// current assignment is load-bearing, and this is one tool needing one
    /// exception.
    ///
    /// # Errors
    /// Returns `NotAuthorized` when the task is not this worker's own.
    /// Work this worker finished, newest first.
    ///
    /// Exists so an empty task list can explain itself. A worker whose only
    /// assignment closes sees the same empty list as one that never had work,
    /// and the difference between "you are done" and "you were cut off" is not
    /// visible from the list alone.
    ///
    /// Keyed on the WORKER rather than the session, for the same reason
    /// `task_this_worker_finished` is: the question is "did I do this", and
    /// that answer survives a restart.
    ///
    /// # Errors
    /// Returns a persistence error when the tasks cannot be read.
    pub fn tasks_this_worker_finished(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<Task>, ApplicationError> {
        let mut finished = self
            .list_tasks()?
            .into_iter()
            .filter(|task| {
                task.assigned_worker_id == Some(principal.worker_id)
                    && task.state == TaskState::Completed
            })
            .collect::<Vec<_>>();
        finished.sort_by_key(|task| std::cmp::Reverse(task.updated_at));
        Ok(finished)
    }

    /// # Errors
    /// Returns `NotAuthorized` when this worker did not do the work, or a
    /// persistence error when the task cannot be read.
    pub fn task_this_worker_finished(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<Task, ApplicationError> {
        let task = self.store.get_task(task_id)?;
        if principal.role == WorkerRole::Queen {
            return Ok(task);
        }
        // The WORKER, not the worker-and-session. Visibility is session-scoped
        // because it answers "what may I act on now", and a new session is not
        // holding the old one's assignment. This answers a different question —
        // "did I do this work" — and the answer does not change when the fleet
        // restarts. Keying on the session here meant a worker could never write
        // the reply for anything it finished before a restart, which is exactly
        // when a person has been waiting longest. Found by a reload on
        // 2026-08-25: session 01a03637 ended, 01a0389d began, and the reply to
        // an email from 22 August became unwritable.
        let owned = task.assigned_worker_id == Some(principal.worker_id);
        owned.then_some(task).ok_or(ApplicationError::NotAuthorized)
    }

    /// A draft this worker filed and nobody has routed yet.
    ///
    /// `create_task` lets a worker record work it noticed, as an unassigned
    /// draft for Queen to route. The filer could not then correct it: a draft is
    /// nobody's assignment, so it fails the visibility check, and it was never
    /// finished, so it fails `task_this_worker_finished` too.
    ///
    /// So the one party who knows the ticket is wrong — the one who wrote it —
    /// was the only party who could not say so.
    ///
    /// ⚠️ DRAFT ONLY. The moment Queen routes it this stops applying and the
    /// ordinary assignment rules govern, which is what keeps this from being a
    /// way to reach into live work.
    ///
    /// # Errors
    /// Denies a task that is not an unrouted draft this worker filed, and
    /// propagates persistence failures.
    pub fn unrouted_draft_this_worker_filed(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<Task, ApplicationError> {
        if self
            .store
            .worker_owns_unrouted_draft(task_id, principal.worker_id)?
        {
            return Ok(self.store.get_task(task_id)?);
        }
        Err(ApplicationError::NotAuthorized)
    }

    /// Reads one task's history, including the notes workers wrote on it.
    ///
    /// Queen's job is to accept or reject finished work, and the outcome
    /// notification only carries an excerpt of the handoff — it has to, because
    /// pasting whole handoffs put 128 KB through her terminal in a day. The
    /// excerpt points at task history, and until now nothing let her read it,
    /// so she was asked to judge work on evidence she could not see.
    ///
    /// History is evidence, not authority to act. Queen sees any task; a worker
    /// retains read access through its durable task ownership after completion
    /// or session replacement, just as for reading its finished-work evidence.
    ///
    /// # Errors
    /// Denies a task the caller cannot see, and propagates persistence failures.
    pub fn read_task_history(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        limit: usize,
    ) -> Result<swarm_domain::TaskActivityPage, ApplicationError> {
        // A reader granted the whole board reads the whole board's history.
        // Anyone else is held to their own assignment exactly as before —
        // `task_this_worker_finished` is the same check it has always been, and
        // this is an additional way in rather than a relaxation of it.
        self.may_read_task(principal, task_id)?;
        Ok(self.store.list_task_activity(task_id, limit)?)
    }

    /// Sends a worker's own task message, optionally answering one exact review.
    ///
    /// # Errors
    /// Refuses non-worker callers and persistence/correlation failures.
    pub fn message_queen_from_worker(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        body: &str,
        reply_to: Option<&str>,
        now: i64,
    ) -> Result<swarm_persistence::TaskMessage, ApplicationError> {
        if principal.role != WorkerRole::Worker {
            return Err(ApplicationError::NotAuthorized);
        }
        Ok(self.store.message_queen_from_worker(
            task_id,
            principal.worker_id,
            body,
            reply_to,
            now,
        )?)
    }

    /// Queen reconciles a specific uncertain delivery after inspecting its content.
    ///
    /// # Errors
    /// Denies workers and propagates persistence or reason validation errors.
    pub fn reconcile_task_message(
        &self,
        principal: AgentPrincipal,
        message_id: &str,
        claim_id: &str,
        retry_may_duplicate: bool,
        reason: &str,
        now: i64,
    ) -> Result<bool, ApplicationError> {
        if principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        Ok(self.store.reconcile_task_message(
            message_id,
            claim_id,
            retry_may_duplicate,
            reason,
            now,
        )?)
    }

    /// Reads the current review request for Queen or its assigned worker.
    ///
    /// # Errors
    /// Denies another worker's task and returns persistence failures.
    pub fn read_returned_review_request(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<Option<swarm_persistence::ReturnedReviewRequest>, ApplicationError> {
        let task = self.store.get_task(task_id)?;
        if principal.role != WorkerRole::Queen
            && task.assigned_worker_id != Some(principal.worker_id)
            && self.may_read_task(principal, task_id).is_err()
        {
            return Err(ApplicationError::NotAuthorized);
        }
        Ok(self.store.returned_review_request(task_id)?)
    }

    /// Reads the Queen-worker exchange recorded on a task.
    ///
    /// Same visibility rule as the history it accompanies. Separate from
    /// `events` for the same reason evidence is: a message is not a state
    /// change, and folding it into the activity log would make an exchange
    /// read as something that moved the work.
    ///
    /// # Errors
    /// Denies a task the caller cannot see, and returns persistence failures.
    pub fn read_task_messages(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<Vec<swarm_persistence::TaskMessage>, ApplicationError> {
        self.may_read_task(principal, task_id)?;
        Ok(self.store.task_messages(task_id)?)
    }

    /// Reads the completion evidence that sits BESIDE a task's activity log.
    ///
    /// Same visibility rule as the history it accompanies. Separate because it
    /// is not activity: claiming an exemption, approving one and recording a
    /// deployment write their own tables and no event, so a caller reading only
    /// the log sees an accurate list of transitions and concludes there is no
    /// evidence. That inference cost a real misfiling.
    ///
    /// # Errors
    /// Denies a task the caller cannot see, and returns persistence failures.
    pub fn read_task_evidence(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<swarm_persistence::TaskEvidenceRecord, ApplicationError> {
        self.may_read_task(principal, task_id)?;
        Ok(self.store.task_evidence_record(task_id)?)
    }

    /// Approves a worker's claim that a task had nothing to deploy.
    ///
    /// The completion gate exists so nothing is recorded as done without
    /// evidence, and for shipping work the deployment is that evidence. Work
    /// that ships nothing — documentation, an investigation, a duplicate,
    /// a "verified, not applicable" finding — has none, so a worker claims an
    /// exemption and somebody else agrees to it. The store has enforced that
    /// from the start; nothing above it could ever say yes, so non-shipping
    /// work could be finished and never completed.
    ///
    /// Queen approves. A worker cannot approve its own claim, which is the
    /// whole point of a second pair of eyes; the operator can, so a wedged
    /// Queen cannot strand finished work.
    ///
    /// # Errors
    /// Denies a worker caller, denies a task the caller cannot see, and
    /// propagates persistence failures.
    pub fn approve_completion_exemption(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        basis: &str,
    ) -> Result<swarm_persistence::CompletionEvidence, ApplicationError> {
        if principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        let visible = self
            .list_visible_tasks(principal)?
            .into_iter()
            .any(|task| task.id == task_id);
        if !visible {
            return Err(ApplicationError::NotAuthorized);
        }
        Ok(self.store.approve_completion_exemption(
            task_id,
            "queen",
            basis,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0)),
        )?)
    }

    /// Lists the local roster for Queen coordination.
    ///
    /// # Errors
    /// Denies worker callers and propagates persistence failures.
    pub fn list_workers(
        &self,
        principal: AgentPrincipal,
    ) -> Result<Vec<WorkerProfile>, ApplicationError> {
        require_queen(principal)?;
        self.store.list_worker_profiles().map_err(Into::into)
    }

    /// Queen-only durable Scout routing evidence; not permission to dispatch.
    ///
    /// # Errors
    /// Denies ordinary workers and propagates persistence failures.
    pub fn scout_routing_facts(
        &self,
        principal: AgentPrincipal,
        now: i64,
    ) -> Result<Option<swarm_persistence::ScoutRoutingFacts>, ApplicationError> {
        require_queen(principal)?;
        self.store.scout_routing_facts(now).map_err(Into::into)
    }

    /// Creates a draft in the local Hive.
    ///
    /// Open to any worker, not only Queen. A worker that discovers follow-up
    /// work — in its own repository or another — had no way to record it and
    /// simply lost it. The operator hit this directly: a worker asked to file
    /// architecture tasks found no tool for it and stalled.
    ///
    /// A draft is inert. It is not assigned, not queued, and cannot be worked
    /// until someone readies it, so recording work is not routing it. Routing
    /// stays Queen's, which is the authority boundary that matters here.
    ///
    /// # Errors
    /// Propagates validation or persistence failures.
    pub fn create_task(
        &self,
        principal: AgentPrincipal,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, ApplicationError> {
        let task = self.store.create_task_with_details_as(
            title,
            description,
            priority,
            workspace,
            &TaskActivityActor::worker(principal.worker_id),
        )?;
        // Filing is the whole of what a worker can do here, so it has to reach
        // the one who routes. Queen already sees drafts; nothing told her one
        // was waiting, and a worker has no other channel to her. Queen filing
        // her own draft needs no such notice.
        if principal.role != WorkerRole::Queen
            && let Some(session_id) = principal.active_session_id
        {
            self.store.record_worker_filed_draft_attention(
                task.id,
                principal.worker_id,
                session_id,
            )?;
        }
        Ok(task)
    }

    /// Assigns a task to a stable worker, whether running or sleeping.
    ///
    /// # Errors
    /// Denies worker callers and invalid persistence changes.
    pub fn assign_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        worker_id: WorkerId,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        self.store.get_worker_profile(worker_id)?;
        self.store
            .assign_task_to_worker_as(
                task_id,
                worker_id,
                &TaskActivityActor::worker(principal.worker_id),
            )
            .map_err(Into::into)
    }
}

/// What a blocking task waits on, named on the transition that blocks it.
///
/// Both kinds are here because the gap was hit twice in one day and the two
/// cases needed different ones: a worker waiting on an operator ruling, and a
/// worker waiting on another task. Supporting only one would have left half the
/// hole open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockerLink {
    Prerequisite(TaskId),
    Decision(swarm_domain::DecisionRequestId),
}

impl TaskService {
    /// Records the blocker a worker names while blocking its OWN task.
    ///
    /// ⚠️ WHY THIS EXISTS AT ALL. The guard below demands a structured blocker,
    /// and named two tools to record one — both Queen-only. A worker with a real
    /// blocker was told to do something it had no way to do, and the refusal
    /// read like a malformed call rather than a permission boundary, so the
    /// honest response was to retry and eventually leave the task Active. That
    /// is the silent stall the guard exists to prevent, reached from the other
    /// side. Recording it here keeps the requirement and supplies the missing
    /// affordance, without putting a freestanding linking tool on a worker's
    /// surface.
    ///
    /// ⚠️ AUTHORITY IS THE CALLER'S TO ESTABLISH FIRST. This writes a link and
    /// does not check who is asking; `transition_task` proves the task is the
    /// caller's own before calling it. Moving this call above that check would
    /// let a worker annotate somebody else's work.
    fn record_named_blocker(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        blocker: BlockerLink,
        now: i64,
    ) -> Result<(), ApplicationError> {
        let actor = TaskActivityActor::worker(principal.worker_id);
        let reason = "Named by the assigned worker while blocking its own task.";
        match blocker {
            BlockerLink::Prerequisite(prerequisite_id) => {
                self.apply_prerequisite_change(
                    &actor,
                    &swarm_domain::TaskPrerequisiteChange {
                        task_id,
                        prerequisite_id,
                        operation: swarm_domain::PrerequisiteOperation::Add,
                        reason: reason.to_owned(),
                    },
                    now,
                )?;
            }
            BlockerLink::Decision(decision_id) => {
                // The stale-evidence guard compares against the revision the
                // CALLER last read. Queen links a decision after reading review
                // evidence and forming a judgement, so a revision that moved
                // underneath her means she is acting on a stale picture. A worker
                // naming what its own task waits on has no prior read to go
                // stale, so the current revision is read here rather than
                // demanded from a caller that never held one. This satisfies the
                // guard for this path; it does not disable it for Queen's.
                let expected_evidence_revision = self
                    .store
                    .queen_task_review_evidence(task_id)?
                    .evidence_revision;
                self.apply_decision_link_change(
                    &actor,
                    &swarm_domain::TaskDecisionLinkChange {
                        task_id,
                        decision_id,
                        operation: swarm_domain::TaskDecisionLinkOperation::Add,
                        reason: reason.to_owned(),
                        expected_evidence_revision,
                    },
                    now,
                )?;
            }
        }
        Ok(())
    }

    /// Refuses a Blocked that names neither an owner nor what it waits for.
    ///
    /// ⚠️ THE STATE THIS MAKES UNREACHABLE HAD 25 OCCUPANTS IN ONE EVENING.
    /// Queen converted 22 drafts into blocked tasks in about 25 minutes with the
    /// verdict `insufficient_evidence` and assigned none of them; 24 of the 25
    /// had never been assigned to anyone at any point. Nothing objected, and
    /// nothing could see them afterwards either: `unattended_block_candidates`
    /// joins on the assignee, so an ownerless block can never become an
    /// attention record. The board read 32 blocked while Needs You read 0.
    ///
    /// ⚠️ AND WIDENING THAT DETECTOR WOULD HAVE BEEN THE WRONG FIX. It would
    /// have pushed 25 items at the operator — an operator button in place of
    /// working automation, which this Hive has been burned by before. The bad
    /// state is refused instead.
    ///
    /// Blocked now means one of two things and says which: somebody owes work
    /// and is waiting on a named thing, or it is not blocked, it is unstarted.
    /// The blocker must be a LINK — a decision or a prerequisite — because the
    /// board can compute what unblocks when a link resolves and cannot act on a
    /// sentence. Only 6 of 32 blocked tasks carried either when this was
    /// written, which is the measure of how little the old state meant.
    ///
    /// # Errors
    /// Refuses with the missing half named, and how to supply it.
    fn refuse_a_block_nobody_owns(&self, task_id: TaskId) -> Result<(), ApplicationError> {
        let task = self.store.get_task(task_id)?;
        if task.assigned_worker_id.is_none() {
            return Err(ApplicationError::TransitionNotPermitted(
                "Blocked work needs somebody who owes it. Assign a worker first with \
                 swarm_assign_task — an unowned block is invisible to the sweep that \
                 chases stale work, because that sweep asks whose it is. If the work is \
                 under-specified rather than waiting on something, assign the scoping \
                 pass as work instead of parking it."
                    .into(),
            ));
        }
        if !self.store.task_has_structured_blocker(task_id)? {
            return Err(ApplicationError::TransitionNotPermitted(
                "Blocked work has to name what it waits for, as a link rather than a \
                 sentence. Name it on this same call: pass prerequisite_id for the task \
                 it waits on, or decision_id for the operator decision it needs. A link \
                 lets the board notice when the blocker resolves; prose does not, and \
                 prose is how work comes to rest here forever. If you have no id to \
                 name, the work is under-specified rather than waiting on something — \
                 say so in Review instead of parking it."
                    .into(),
            ));
        }
        Ok(())
    }

    /// Applies a domain-valid state transition within the caller's authority.
    ///
    /// Workers may report progress only for their own assignment and cannot approve completion.
    /// Queen may apply any transition accepted by the task lifecycle.
    ///
    /// # Errors
    /// Denies foreign assignments or worker completion and propagates domain failures.
    pub fn transition_task(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        target: TaskState,
        note: &str,
    ) -> Result<Task, ApplicationError> {
        self.transition_task_naming_blocker(principal, task_id, target, note, None, 0)
    }

    /// A transition that may NAME what it waits on, in the same call that blocks.
    ///
    /// One call on purpose. A block and its reason recorded separately can drift
    /// apart — the block lands, the link fails, and the task rests with a
    /// blocker nobody can act on, which is the state this whole control exists
    /// to prevent. `now` is only read when a link is supplied.
    ///
    /// # Errors
    /// Refuses a caller that does not own the task, a move outside a worker's
    /// lifecycle, an unrecordable link, and a block that still names nothing.
    pub fn transition_task_naming_blocker(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
        target: TaskState,
        note: &str,
        blocker: Option<BlockerLink>,
        now: i64,
    ) -> Result<Task, ApplicationError> {
        if principal.role != WorkerRole::Queen {
            let session_id = principal
                .active_session_id
                .ok_or(ApplicationError::WorkerNotRunning)?;
            let task = self.store.get_task(task_id)?;
            if task.assigned_worker_id != Some(principal.worker_id)
                || task.assigned_session_id != Some(session_id)
            {
                return Err(ApplicationError::NotAuthorized);
            }
            if !matches!(
                target,
                TaskState::Active | TaskState::Blocked | TaskState::Review
            ) {
                return Err(ApplicationError::TransitionNotPermitted(
                    worker_transition_refusal(target),
                ));
            }
            // ⚠️ ORDER IS THE SECURITY PROPERTY HERE. Ownership is proven above
            // before anything is written, so a worker cannot record a blocker on
            // work that is not its own. The guard runs AFTER the link so that a
            // named blocker satisfies it, and still refuses when nothing was
            // named — the requirement is unchanged, only its affordance.
            if target == TaskState::Blocked {
                if let Some(blocker) = blocker {
                    self.record_named_blocker(principal, task_id, blocker, now)?;
                }
                self.refuse_a_block_nobody_owns(task_id)?;
            }
            return self
                .store
                .transition_worker_task(task_id, target, note, session_id)
                .map_err(Into::into);
        }
        if target == TaskState::Blocked {
            if let Some(blocker) = blocker {
                self.record_named_blocker(principal, task_id, blocker, now)?;
            }
            self.refuse_a_block_nobody_owns(task_id)?;
        }
        require_completion_evidence(&self.store, target, task_id, note)?;
        if target == TaskState::Active {
            let task = self.store.get_task(task_id)?;
            let session_id = task
                .assigned_session_id
                .ok_or(ApplicationError::WorkerNotRunning)?;
            return self
                .store
                .transition_assigned_task_with_note_as(
                    task_id,
                    target,
                    note,
                    session_id,
                    &TaskActivityActor::worker(principal.worker_id),
                )
                .map_err(|error| match error {
                    TaskStoreError::WorkerSessionNotActive => ApplicationError::WorkerNotRunning,
                    error => ApplicationError::Store(error),
                });
        }
        self.store
            .transition_task_with_note_as(
                task_id,
                target,
                note,
                &TaskActivityActor::worker(principal.worker_id),
            )
            .map_err(Into::into)
    }
    /// Lists the operator/Queen inbox, or only requests originated by a worker caller.
    ///
    /// # Errors
    /// Returns a persistence or persisted-data integrity error.
    pub fn list_visible_decisions(
        &self,
        principal: Option<AgentPrincipal>,
    ) -> Result<Vec<DecisionRequest>, ApplicationError> {
        // Scope before the persistence read cap, not after a global history page.
        let decisions = match principal {
            Some(principal) if principal.role != WorkerRole::Queen => self
                .store
                .list_worker_decision_requests(principal.worker_id)?,
            _ => self.store.list_decision_requests()?,
        };
        // Workers discover both their own requests and rulings on currently
        // assigned work, so they need not rely on Queen relaying an authority ID.
        Ok(decisions)
    }

    /// Creates a typed request for operator judgment using the authenticated agent identity.
    ///
    /// # Errors
    /// Denies foreign task correlation and propagates validation or persistence failures.
    pub fn create_decision(
        &self,
        principal: AgentPrincipal,
        input: &DecisionRequestInput,
    ) -> Result<DecisionRequest, ApplicationError> {
        if principal.role != WorkerRole::Queen
            && let Some(task_id) = input.task_id
        {
            let session_id = principal
                .active_session_id
                .ok_or(ApplicationError::WorkerNotRunning)?;
            let task = self.store.get_task(task_id)?;
            if task.assigned_worker_id != Some(principal.worker_id)
                || task.assigned_session_id != Some(session_id)
            {
                return Err(ApplicationError::NotAuthorized);
            }
        }
        self.store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: principal.worker_id,
                task_id: input.task_id,
                kind: input.kind,
                urgency: input.urgency,
                title: &input.title,
                summary: &input.summary,
                reason: &input.reason,
                risk: &input.risk,
                evidence: &input.evidence,
                suggested_action: &input.suggested_action,
                allowed_actions: &input.allowed_actions,
                operator_actions: &input.operator_actions,
                questions: &input.questions,
                deadline: input.deadline,
                requested_command: input.requested_command.as_deref(),
            })
            .map_err(Into::into)
    }

    /// Answers a pending interview as the authenticated local operator.
    ///
    /// # Errors
    /// Propagates invalid identity, state, completeness, integrity, or
    /// persistence failures.
    pub fn answer_operator_decision(
        &self,
        id: DecisionRequestId,
        answers: &std::collections::BTreeMap<String, Vec<String>>,
        note: &str,
        surface: &str,
    ) -> Result<DecisionRequest, ApplicationError> {
        self.store
            .answer_decision_request(id, answers, note, surface)
            .map_err(Into::into)
    }

    /// Resolves one pending decision as the authenticated local operator.
    ///
    /// # Errors
    /// Rejects stale rendered questions before any resolution or reply is saved.
    pub fn answer_operator_decision_from_snapshot(
        &self,
        id: DecisionRequestId,
        answers: &std::collections::BTreeMap<String, Vec<String>>,
        note: &str,
        surface: &str,
        displayed: Option<&[swarm_domain::DecisionQuestion]>,
    ) -> Result<DecisionRequest, ApplicationError> {
        self.store
            .answer_decision_request_from_snapshot(id, answers, note, surface, displayed)
            .map_err(Into::into)
    }

    /// Resolves one pending decision as the authenticated local operator.
    ///
    /// # Errors
    /// Propagates invalid identity, state, action, integrity, or persistence failures.
    pub fn resolve_operator_decision(
        &self,
        id: DecisionRequestId,
        action: &str,
        note: &str,
        surface: &str,
    ) -> Result<DecisionRequest, ApplicationError> {
        let resolved = self
            .store
            .resolve_decision_request(id, action, note, surface)?;
        self.release_blocks_this_answer_freed(&resolved);
        self.ask_about_blocks_this_answer_left_parked(&resolved);
        Ok(resolved)
    }

    /// Carries out "Release them to the queue" instead of merely recording it.
    ///
    /// ⚠️ THE ACTION WAS OFFERED AND NEVER EXECUTED, WHICH IS WHY THE CARD CAME
    /// BACK. Measured on this Hive 2026-09-21: fourteen "Work left parked by an
    /// answer" decisions, four of them inside seventeen seconds — 17:25:16,
    /// :22, :27 and :33 — every one answered "Release them to the queue", every
    /// one about the SAME task, each spawning the next within six seconds. The
    /// operator's report was "This needs you keeps coming back. I tell ur to
    /// release to the queue", and they were pressing a button that did nothing.
    ///
    /// A recorded answer that performs no act is worse than no button: it reads
    /// as done, and the only evidence otherwise is the question returning.
    fn release_blocks_this_answer_freed(&self, resolved: &DecisionRequest) {
        if resolved.resolution_action.as_deref() != Some(RELEASE_PARKED_WORK_ACTION) {
            return;
        }
        let Ok(parked) = self.store.blocks_left_parked_by_resolving(resolved.id) else {
            return;
        };
        for task in parked {
            // Ready rather than Active: releasing returns work to the queue for
            // routing, which is what the button says. Starting it would be a
            // different and larger claim about who should do it.
            if let Err(error) = self.store.transition_task(task, TaskState::Ready) {
                tracing::warn!(
                    message = %error,
                    decision = %resolved.id,
                    %task,
                    "parked work could not be released to the queue"
                );
            }
        }
    }

    /// Asks the operator about work their answer has just left with no way back.
    ///
    /// ⚠️ THE MOMENT AN ANSWER ARRIVES IS WHEN A BLOCK CAN STOP BEING A WAIT.
    /// A task blocked on a PENDING decision is waiting correctly — the board can
    /// see what will end it. The instant the operator replies, the event it
    /// waited for has happened and the task is parked on an answer instead. Its
    /// row does not change, its state does not change, and so nothing notices.
    /// Measured: five tasks linked to a decision at 12:52 while it was pending,
    /// which resolved at 20:06 seven hours later, and sat invisible afterwards.
    ///
    /// ⚠️ A FAILURE HERE MUST NEVER FAIL THE OPERATOR'S ANSWER. Their resolution
    /// is already committed before this runs, and every outcome below is
    /// swallowed with a warning. Refusing to record an answer because some third
    /// task's bookkeeping is incomplete would be a far worse defect than the one
    /// this fixes.
    ///
    /// ONE QUESTION FOR THE BATCH, not one per task. The failure this addresses
    /// is work nobody can see; replacing it with a queue of near-identical cards
    /// would be the same silence wearing a different coat.
    fn ask_about_blocks_this_answer_left_parked(&self, resolved: &DecisionRequest) {
        // ⚠️ DO NOT ASK ABOUT THE CONSEQUENCES OF YOUR OWN QUESTION. This check
        // raises a follow-up LINKED TO the parked task, which makes that
        // follow-up a gate on the same task — so resolving it re-triggers this
        // check, which raises another, forever. That is the loop the operator
        // hit, and the card's own self-referential title said so: "Answering
        // 'Work left parked by an answer' left 1 blocked task(s)".
        //
        // Belt and braces beside the release above. Releasing moves the task out
        // of Blocked so there is nothing left to find — but the OTHER option,
        // "Keep them parked and set a revisit date", has no way to record a date
        // here, so without this guard that branch would loop exactly as before.
        if resolved.title == PARKED_WORK_QUESTION_TITLE {
            return;
        }
        let parked = match self.store.blocks_left_parked_by_resolving(resolved.id) {
            Ok(parked) if !parked.is_empty() => parked,
            Ok(_) => return,
            Err(error) => {
                tracing::warn!(
                    message = %error,
                    decision = %resolved.id,
                    "could not check whether this answer left work parked"
                );
                return;
            }
        };
        let Ok(Some(queen)) = self.store.queen_worker_id() else {
            tracing::warn!(
                decision = %resolved.id,
                parked = parked.len(),
                "an answer left work parked and there is no Queen to raise it to"
            );
            return;
        };
        let titles = parked
            .iter()
            .filter_map(|task| self.store.get_task(*task).ok())
            .map(|task| format!("- {} ({})", task.title, task.id))
            .collect::<Vec<_>>()
            .join("\n");
        let summary = format!(
            "Answering \"{}\" left {} blocked task(s) with nothing left to wait for. \
             The decision they were gated on is now resolved, so no event will \
             bring them back. Give them a date or release them.\n{titles}",
            resolved.title,
            parked.len()
        );
        let actions = vec![
            RELEASE_PARKED_WORK_ACTION.to_owned(),
            "Keep them parked and set a revisit date".to_owned(),
        ];
        if let Err(error) = self.store.create_decision_request(&NewDecisionRequest {
            requesting_worker_id: queen,
            task_id: parked.first().copied(),
            kind: swarm_domain::DecisionRequestKind::Input,
            urgency: swarm_domain::DecisionUrgency::Normal,
            title: PARKED_WORK_QUESTION_TITLE,
            summary: &summary,
            reason: "A block whose only recorded gate was this decision has no terminating \
                     condition now that it is answered. Nothing will fire to raise it again.",
            risk: "Left as is, these tasks are invisible until somebody happens to remember \
                   them, which is the failure this check exists to prevent.",
            evidence: &summary,
            suggested_action: RELEASE_PARKED_WORK_ACTION,
            allowed_actions: &actions,
            // Neither option is the operator doing the work themselves; both
            // route the tasks back. Nothing here should park.
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: None,
        }) {
            tracing::warn!(
                message = %error,
                decision = %resolved.id,
                parked = parked.len(),
                "an answer left work parked and the follow-up question could not be raised"
            );
        }
    }

    /// Withdraws an obsolete request without exercising operator authority.
    ///
    /// # Errors
    /// Rejects stale agent sessions and unauthorized or invalid withdrawals.
    pub fn withdraw_agent_decision(
        &self,
        principal: AgentPrincipal,
        id: DecisionRequestId,
        reason: &str,
    ) -> Result<DecisionRequest, ApplicationError> {
        let session = principal
            .active_session_id
            .ok_or(ApplicationError::WorkerNotRunning)?;
        let worker = self.store.get_worker_profile(principal.worker_id)?;
        if worker.active_session_id != Some(session) {
            return Err(ApplicationError::WorkerNotRunning);
        }
        self.store
            .withdraw_decision_request(id, principal.worker_id, reason)
            .map_err(Into::into)
    }

    /// Records that a RESOLVED decision was replaced, without rewriting it.
    ///
    /// ⚠️ NOT A WIDENED WITHDRAWAL. Withdrawal would set the record's state and
    /// make it stop reading as resolved; a resolved decision read from the store
    /// IS the operator, and someone may already have acted on it. This only adds
    /// a pointer and a reason. The store enforces the rest.
    ///
    /// # Errors
    /// Rejects stale agent sessions and anything the store refuses.
    pub fn supersede_agent_decision(
        &self,
        principal: AgentPrincipal,
        id: DecisionRequestId,
        superseding: DecisionRequestId,
        reason: &str,
    ) -> Result<DecisionRequest, ApplicationError> {
        let session = principal
            .active_session_id
            .ok_or(ApplicationError::WorkerNotRunning)?;
        let worker = self.store.get_worker_profile(principal.worker_id)?;
        if worker.active_session_id != Some(session) {
            return Err(ApplicationError::WorkerNotRunning);
        }
        self.store
            .supersede_decision_request(id, superseding, principal.worker_id, reason)
            .map_err(Into::into)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionRequestInput {
    pub task_id: Option<TaskId>,
    pub kind: DecisionRequestKind,
    pub urgency: DecisionUrgency,
    pub title: String,
    /// One or two sentences on what the operator is deciding.
    pub summary: String,
    pub reason: String,
    pub risk: String,
    pub evidence: String,
    pub suggested_action: String,
    pub allowed_actions: Vec<String>,
    /// The subset of `allowed_actions` the OPERATOR carries out themselves.
    ///
    /// Picking one of these parks the linked task instead of handing it back to
    /// a worker who cannot act. Marking is only an offer: the park fires on the
    /// operator's choice, never on the marking, so a worker cannot park its own
    /// work. Empty means nothing parks.
    pub operator_actions: Vec<String>,
    /// Present makes this an interview rather than a ruling.
    pub questions: Vec<DecisionQuestion>,
    pub deadline: Option<i64>,
    /// The one command this request is asking to be allowed to run.
    ///
    /// Present makes the store append the grant button; absent leaves the
    /// request an ordinary approval that authorises nothing runnable. The
    /// caller supplies the COMMAND, never the button label — see
    /// `GRANT_COMMAND_ACTION` for why a worker choosing that label would be
    /// checking a string it chose.
    pub requested_command: Option<String>,
}

fn require_queen(principal: AgentPrincipal) -> Result<(), ApplicationError> {
    if principal.role == WorkerRole::Queen {
        Ok(())
    } else {
        Err(ApplicationError::NotAuthorized)
    }
}

/// A task may not be called done on prose alone.
///
/// The operator's ruling, 2026-08-21: nothing closes without a deployment,
/// whether it came from email, Jira, or was written here. Before this, only the
/// email path enforced it, so "COMPLETED" meant two different things depending
/// on where the task came from — and the weaker meaning was the common one.
///
/// A worker that genuinely has nothing to deploy says so and gives a reason,
/// and Queen approves that claim. The claim alone is not enough: the worker
/// asserting its own work needs no evidence cannot also be the one who accepts
/// that assertion.
fn require_completion_evidence(
    store: &TaskStore,
    target: TaskState,
    task_id: TaskId,
    note: &str,
) -> Result<(), ApplicationError> {
    if target != TaskState::Completed {
        return Ok(());
    }
    if note.trim().is_empty() || !store.completion_evidence(task_id)?.closes_a_task() {
        return Err(ApplicationError::Store(
            TaskStoreError::CompletionEvidenceRequired,
        ));
    }
    Ok(())
}

/// Why a worker cannot make this move, and what to do instead.
///
/// NAMES THE REMEDY, not just the rule. A refusal that only states a
/// prohibition leaves the reader to guess the permitted route, and guessing is
/// how work ends up in the wrong state — returning finished work to Ready to
/// get attention invalidated a valid evidence claim on the day this was
/// written.
fn worker_transition_refusal(target: TaskState) -> String {
    match target {
        TaskState::Ready => "Returning work to Ready is Queen's, not yours: Ready means UNSTARTED to everything that reads it, so moving finished work there erases that it was done. If you need it re-routed or picked up by somebody else, say so with swarm_message_queen. If you cannot continue, use Blocked with the reason.".to_owned(),
        TaskState::Completed => "You cannot self-certify completion: evidence must be checked by machinery or someone OTHER than the author. Move it to Review with your handoff and record the evidence. The deterministic coordinator settles supported evidence without another Queen approval. Queen handles exceptions, conflicting evidence, and genuine judgment; an unsupported no-deployment claim is not approval.".to_owned(),
        TaskState::AwaitingRelease => "Awaiting Release is for work Queen has ACCEPTED and that is merely unshipped, so it is hers to set. Move it to Review with your handoff; if it is finished and waiting on a merge, say that in the handoff and she can park it there.".to_owned(),
        TaskState::Abandoned => "Abandoning work is Queen's. If you believe it is superseded or should not continue, move it to Review and say so in the handoff, or raise it with swarm_message_queen.".to_owned(),
        TaskState::Draft => "Nothing goes back to Draft. Work does not become unfiled again; if it was filed wrongly, say so with swarm_message_queen.".to_owned(),
        TaskState::Active | TaskState::Blocked | TaskState::Review => {
            "You may report Active, Blocked or Review for your own assignment.".to_owned()
        }
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("this agent is not authorized for that outcome")]
    NotAuthorized,
    // A MALFORMED IDENTIFIER IS NOT AN AUTHORISATION PROBLEM, and calling it
    // one sends the reader to check assignment, principal, role and routing
    // when the remedy is to read the id. Twenty-one call sites reported it as
    // NotAuthorized; the operator hit it while approving M0 with an id they had
    // guessed rather than read, and the refusal said nothing true.
    #[error(
        "that is not a valid {0} — check the identifier you passed rather than your permissions"
    )]
    MalformedIdentifier(&'static str),
    // A REFUSED TRANSITION SAYS WHICH RULE AND WHAT TO DO INSTEAD, for the same
    // reason MalformedIdentifier exists above: "not authorized" sends the
    // reader to check assignment, principal and routing when the answer is a
    // lifecycle rule they are one sentence away from knowing.
    //
    // Queen spent hours on 2026-09-01 acting on a belief that Blocked work
    // could not be assigned. It can — the store refuses only Completed — and
    // her run brief already carried the correct procedure. So the words existed
    // and did not land, which is why this is at the moment of the act rather
    // than in more standing instruction.
    #[error("{0}")]
    TransitionNotPermitted(String),
    #[error("the target worker does not have an active session")]
    WorkerNotRunning,
    #[error("integration unavailable: {0}")]
    IntegrationUnavailable(String),
    #[error("that Apiary shared-work backend is not available yet")]
    SharedWorkBackendUnavailable,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{CommitRepositoryState, JiraProjectScope, JiraStatusMapping, ProviderKind};
    use swarm_persistence::JiraProjectBindingInput;

    /// ⚠️ THE OPERATOR'S ACTUAL COMPLAINT, REPRODUCED: "This needs you keeps
    /// coming back. I tell ur to release to the queue."
    ///
    /// Measured on the live Hive 2026-09-21 before the fix: fourteen of these
    /// decisions, four inside seventeen seconds, every one answered "Release
    /// them to the queue", every one about the same task. The button was offered
    /// in two places and executed in none.
    #[test]
    fn releasing_parked_work_actually_releases_it_and_does_not_ask_again() {
        let (service, _queen, worker) = setup();
        let task = service
            .store
            .create_task("Parked work", "/workspace/petal")
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();

        let actions = vec!["Proceed".to_owned()];
        let gate = service
            .store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task.id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "The gate this work waits on",
                summary: "Whether to proceed, and what it costs if we do not.",
                reason: "Needs an operator answer.",
                risk: "None.",
                evidence: "None.",
                suggested_action: "Proceed",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Blocked)
            .unwrap();

        // Answering the gate leaves the work with nothing to wait for, so the
        // check asks about it. That part is correct and stays.
        service
            .resolve_operator_decision(gate.id, "Proceed", "", "control_room")
            .unwrap();
        let asked = service
            .store
            .list_decision_requests()
            .unwrap()
            .into_iter()
            .find(|decision| {
                decision.title == PARKED_WORK_QUESTION_TITLE
                    && decision.state == swarm_domain::DecisionRequestState::Pending
            })
            .expect("the check should ask once");

        // The operator presses the button it recommends.
        service
            .resolve_operator_decision(asked.id, RELEASE_PARKED_WORK_ACTION, "", "control_room")
            .unwrap();

        assert_eq!(
            service.store.get_task(task.id).unwrap().state,
            TaskState::Ready,
            "Release them to the queue must RELEASE the work, not merely record that it was chosen"
        );
        let asked_again = service
            .store
            .list_decision_requests()
            .unwrap()
            .into_iter()
            .filter(|decision| {
                decision.title == PARKED_WORK_QUESTION_TITLE
                    && decision.state == swarm_domain::DecisionRequestState::Pending
            })
            .count();
        assert_eq!(
            asked_again, 0,
            "answering the question must not raise the same question again"
        );
    }

    /// The other option has no way to record a date here, so without the
    /// self-reference guard it would loop exactly as Release did.
    #[test]
    fn keeping_work_parked_does_not_ask_the_same_question_forever() {
        let (service, _queen, worker) = setup();
        let task = service
            .store
            .create_task("Parked work", "/workspace/petal")
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        let actions = vec!["Proceed".to_owned()];
        let gate = service
            .store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task.id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "The gate this work waits on",
                summary: "Whether to proceed, and what it costs if we do not.",
                reason: "Needs an operator answer.",
                risk: "None.",
                evidence: "None.",
                suggested_action: "Proceed",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Blocked)
            .unwrap();
        service
            .resolve_operator_decision(gate.id, "Proceed", "", "control_room")
            .unwrap();
        let asked = service
            .store
            .list_decision_requests()
            .unwrap()
            .into_iter()
            .find(|decision| decision.title == PARKED_WORK_QUESTION_TITLE)
            .expect("the check should ask once");

        service
            .resolve_operator_decision(
                asked.id,
                "Keep them parked and set a revisit date",
                "",
                "control_room",
            )
            .unwrap();

        assert_eq!(
            service.store.get_task(task.id).unwrap().state,
            TaskState::Blocked,
            "keeping it parked must leave it parked"
        );
        let pending = service
            .store
            .list_decision_requests()
            .unwrap()
            .into_iter()
            .filter(|decision| {
                decision.title == PARKED_WORK_QUESTION_TITLE
                    && decision.state == swarm_domain::DecisionRequestState::Pending
            })
            .count();
        assert_eq!(pending, 0, "the check must not ask about its own answer");
    }

    fn setup() -> (TaskService, WorkerProfile, WorkerProfile) {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        (TaskService::new(store), queen, worker)
    }

    #[test]
    fn prerequisite_commands_require_queen_or_operator_and_preserve_state() {
        let (service, queen, worker) = setup();
        let task = service
            .store
            .create_task("Consumer", "/workspace/petal")
            .unwrap();
        let upstream = service
            .store
            .create_task("Contract", "/workspace/queen")
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Blocked)
            .unwrap();
        let mut change = swarm_domain::TaskPrerequisiteChange {
            task_id: task.id,
            prerequisite_id: upstream.id,
            operation: swarm_domain::PrerequisiteOperation::Add,
            reason: "Needs the contract".into(),
        };
        assert!(
            service
                .change_task_prerequisite(AgentPrincipal::from(&worker), &change, 10)
                .is_err()
        );
        assert!(
            service
                .store
                .get_task(task.id)
                .unwrap()
                .prerequisites
                .is_empty()
        );
        let linked = service
            .change_task_prerequisite(AgentPrincipal::from(&queen), &change, 11)
            .unwrap();
        assert_eq!(linked.state, TaskState::Blocked);
        assert_eq!(linked.prerequisites.len(), 1);
        change.operation = swarm_domain::PrerequisiteOperation::Remove;
        change.reason = "Operator removed the dependency".into();
        let unlinked = service
            .change_operator_task_prerequisite(&change, 12)
            .unwrap();
        assert!(unlinked.prerequisites.is_empty());
        assert_eq!(unlinked.state, TaskState::Blocked);
    }

    /// Nineteen finished tasks sat "waiting on evidence" for ten days and the
    /// operator reported there was no clear path to close them. The obvious
    /// theory was a dead end: evidence is a worker tool, so a task whose worker
    /// session had ended would have nobody left who could supply it.
    ///
    /// THAT THEORY IS WRONG, and this test is what says so rather than a
    /// reading of the code. Evidence is keyed on the WORKER, not the session,
    /// and Queen can reach any task at all. The path was there the whole time;
    /// nothing surfaced it.
    ///
    /// The third assertion is the one that must never stop holding: none of
    /// this makes it easier to close work without evidence.
    /// Abandoning work asks for no evidence, and that is the whole point.
    ///
    /// Completing the same task without evidence is refused, so this pair is
    /// the difference the state was added to make: one outcome asks what shows
    /// the work is running, the other never asks. If this test ever passes for
    /// both, the state has become Completed-with-a-flag.
    #[test]
    fn abandoning_work_needs_no_evidence_while_completing_it_still_does() {
        let (service, _queen, worker) = setup();
        let store = service.store.clone();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        let refused = store
            .create_task("Spike a thing", "/workspace/petal")
            .unwrap();
        store.transition_task(refused.id, TaskState::Ready).unwrap();
        store
            .transition_task(refused.id, TaskState::Active)
            .unwrap();
        store
            .transition_task(refused.id, TaskState::Review)
            .unwrap();
        assert!(
            require_completion_evidence(&store, TaskState::Completed, refused.id, "done").is_err(),
            "completing without evidence must still be refused"
        );

        // Same task, same absence of evidence, different outcome.
        assert!(
            require_completion_evidence(&store, TaskState::Abandoned, refused.id, "superseded")
                .is_ok(),
            "abandoning must not ask for evidence"
        );
        store
            .transition_task(refused.id, TaskState::Abandoned)
            .unwrap();
        assert_eq!(
            store.get_task(refused.id).unwrap().state,
            TaskState::Abandoned
        );
    }

    /// Abandoned work stops being the worker's problem.
    ///
    /// `archive_worker_profile` refuses while a worker still owns open work,
    /// and it decided that with `state != 'completed'` -- written when completed
    /// was the only way to close anything. Left alone, an abandoned task would
    /// have pinned its worker open forever: closed on the board, and still
    /// blocking the roster.
    #[test]
    fn abandoned_work_stops_blocking_its_worker() {
        let (service, _queen, worker) = setup();
        let store = service.store.clone();
        let task = store
            .create_task("Chase a dead end", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();

        assert!(
            matches!(
                store.archive_worker_profile(worker.id),
                Err(TaskStoreError::WorkerOwnsOpenTasks)
            ),
            "precondition: open work blocks archiving"
        );

        store
            .transition_task(task.id, TaskState::Abandoned)
            .unwrap();

        store
            .archive_worker_profile(worker.id)
            .expect("abandoned work must not keep its worker open");
    }

    /// A worker that files a draft can correct it, and loses that reach the
    /// moment the draft is routed.
    ///
    /// The filer was the one party who could not fix its own ticket: a draft is
    /// nobody's assignment, so it fails visibility, and it never started, so it
    /// fails `task_this_worker_finished`. The narrowness is the point -- the
    /// three denials below are what stop this being a way into live work.
    #[test]
    fn a_worker_may_correct_its_own_draft_until_the_moment_it_is_routed() {
        let (service, _queen, worker) = setup();
        let store = service.store.clone();
        let other = store
            .create_worker(
                "Thistle",
                ProviderKind::ClaudeCode,
                "/workspace/thistle",
                false,
                1,
            )
            .unwrap();
        let principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: None,
        };
        let other_principal = AgentPrincipal {
            worker_id: other.id,
            role: WorkerRole::Worker,
            active_session_id: None,
        };

        let draft = service
            .create_task(
                principal,
                "Rotate the signing key",
                "Noticed while packaging.",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        assert_eq!(draft.state, TaskState::Draft, "precondition: it is a draft");

        // THE POINT: the filer reaches it.
        assert_eq!(
            service
                .unrouted_draft_this_worker_filed(principal, draft.id)
                .unwrap()
                .id,
            draft.id,
            "the worker that wrote the ticket must be able to correct it"
        );

        // And nobody else's draft becomes reachable by filing one of your own.
        let theirs = service
            .create_task(
                other_principal,
                "Something else entirely",
                "Filed by a different worker.",
                TaskPriority::Normal,
                "/workspace/thistle",
            )
            .unwrap();
        assert!(
            matches!(
                service.unrouted_draft_this_worker_filed(principal, theirs.id),
                Err(ApplicationError::NotAuthorized)
            ),
            "another worker's draft is not this worker's to edit"
        );

        // THE BOUNDARY. Routing hands the ticket to Queen's judgement, and the
        // filer's authorship stops being authority over it.
        store.transition_task(draft.id, TaskState::Ready).unwrap();
        assert!(
            matches!(
                service.unrouted_draft_this_worker_filed(principal, draft.id),
                Err(ApplicationError::NotAuthorized)
            ),
            "a routed ticket is governed by assignment again, not by who filed it"
        );

        // Assignment is the other half of routed: a draft handed straight to a
        // worker is no longer an unrouted draft either.
        let assigned = service
            .create_task(
                principal,
                "Handed over before routing",
                "Filed, then assigned while still a draft.",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        store
            .assign_task_to_worker_as(assigned.id, other.id, &TaskActivityActor::operator())
            .unwrap();
        assert!(
            matches!(
                service.unrouted_draft_this_worker_filed(principal, assigned.id),
                Err(ApplicationError::NotAuthorized)
            ),
            "once a draft carries an assignee it belongs to the assignee, not the filer"
        );
    }

    #[test]
    fn finished_work_without_evidence_can_still_be_reached_after_its_session_ends() {
        let (service, queen, worker) = setup();
        let store = service.store.clone();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Ship the guard", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        store
            .record_task_deployment(task.id, "production", "sha abc123", 1_000)
            .unwrap();
        store
            .transition_task(task.id, TaskState::Completed)
            .unwrap();

        // The session that did the work is gone, exactly like the 19.
        store.release_worker_session(session).unwrap();

        let worker_principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: None,
        };
        assert_eq!(
            service
                .task_this_worker_finished(worker_principal, task.id)
                .unwrap()
                .id,
            task.id,
            "the worker that did the work can still reach it after its session ended -- keyed on \
             the worker, not the session, so an ended session strands nothing"
        );

        let queen_principal = AgentPrincipal {
            worker_id: queen.id,
            role: WorkerRole::Queen,
            active_session_id: None,
        };
        assert_eq!(
            service
                .task_this_worker_finished(queen_principal, task.id)
                .unwrap()
                .id,
            task.id,
            "and Queen reaches any finished task, so there is no state with nobody able to act"
        );

        // NOTHING ABOVE MAKES CLOSING EASIER. A second task with no evidence
        // still cannot be completed by anyone, Queen included.
        let bare = store
            .create_task("No evidence", "/workspace/petal")
            .unwrap();
        store.transition_task(bare.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(bare.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        store.transition_task(bare.id, TaskState::Active).unwrap();
        store.transition_task(bare.id, TaskState::Review).unwrap();
        assert!(
            matches!(
                service.transition_task(
                    queen_principal,
                    bare.id,
                    TaskState::Completed,
                    "verified by hand",
                ),
                Err(ApplicationError::Store(
                    TaskStoreError::CompletionEvidenceRequired
                ))
            ),
            "the 2026-08-21 ruling still holds: nothing closes without evidence, not even for Queen"
        );
    }

    #[test]
    fn connection_card_is_a_public_identity_action_not_membership() {
        let store = TaskStore::in_memory().unwrap();
        let service = ApiaryService::new(store.clone());
        let card = service.connection_card(10_000).unwrap();

        assert_eq!(card.payload.expires_at, 10_000 + 24 * 60 * 60);
        assert_eq!(
            card.payload.hive_id,
            store.local_hive_identity().unwrap().hive.id
        );
        assert_eq!(
            store.local_apiary_context().unwrap(),
            LocalApiaryContext::Personal
        );
    }

    #[test]
    fn keeper_candidate_import_stays_separate_from_membership_and_invitations() {
        let remote_service = ApiaryService::new(TaskStore::in_memory().unwrap());
        let card = remote_service.connection_card(10_000).unwrap();
        let keeper_store = TaskStore::in_memory().unwrap();
        let keeper = ApiaryService::new(keeper_store.clone());
        keeper
            .create_from_personal_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();

        let pinned = keeper.pin_hive_candidate(&card, 10_001).unwrap();
        assert_eq!(keeper.hive_candidates().unwrap(), vec![pinned]);
        assert!(
            keeper
                .pending_invitations(JiraConnectionState::Ready, 10_001)
                .unwrap()
                .is_empty()
        );
        assert_eq!(keeper.collapse_readiness().unwrap().active_hive_count, 1);
    }

    #[test]
    fn keeper_invitation_is_bound_to_a_pinned_independent_hive() {
        let remote_service = ApiaryService::new(TaskStore::in_memory().unwrap());
        let card = remote_service.connection_card(10_000).unwrap();
        let keeper_store = TaskStore::in_memory().unwrap();
        let keeper = ApiaryService::new(keeper_store.clone());
        keeper
            .create_from_personal_hive("Garden", SharedWorkBackend::Jira, 9_000)
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&card, 10_001).unwrap();

        let bundle = keeper
            .invite_hive_candidate(candidate.hive_id, "https://keeper.example.test", 10_100)
            .unwrap();

        assert_eq!(bundle.invitation.payload.invited_hive_id, candidate.hive_id);
        assert_eq!(bundle.invitation.payload.invited_node_id, candidate.node_id);
        assert_eq!(keeper.collapse_readiness().unwrap().active_hive_count, 1);
        assert_eq!(
            keeper
                .collapse_readiness()
                .unwrap()
                .pending_invitation_count,
            1
        );
        assert_eq!(
            keeper_store.local_hive_identity().unwrap().hive.apiary_id,
            Some(candidate.apiary_id)
        );
    }

    #[test]
    fn apiary_creation_is_a_single_application_command_with_keeper_outcome() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let service = ApiaryService::new(store.clone());

        let context = service
            .create_from_personal_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        assert!(matches!(
            context,
            LocalApiaryContext::Federated {
                apiary,
                local_role: swarm_domain::LocalApiaryRole::Keeper,
            } if apiary.keeper_operator_id == identity.operator.id
                && apiary.shared_work_backend() == SharedWorkBackend::Jira
        ));
        assert!(matches!(
            service.create_from_personal_hive("Second", SharedWorkBackend::Jira, 20),
            Err(ApplicationError::Store(
                TaskStoreError::ApiaryMembershipConflict
            ))
        ));
    }

    #[test]
    fn native_apiary_creation_stays_unavailable_until_distributed_guarantees_exist() {
        let service = ApiaryService::new(TaskStore::in_memory().unwrap());
        assert!(matches!(
            service.create_from_personal_hive("Orchard", SharedWorkBackend::Native, 10),
            Err(ApplicationError::SharedWorkBackendUnavailable)
        ));
        assert_eq!(
            service.store.local_apiary_context().unwrap(),
            LocalApiaryContext::Personal
        );
    }

    #[test]
    fn apiary_collapse_is_exposed_only_as_a_revalidated_application_command() {
        let store = TaskStore::in_memory().unwrap();
        let service = ApiaryService::new(store);
        let context = service
            .create_from_personal_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected a federated Hive");
        };
        assert_eq!(
            service.collapse_readiness().unwrap(),
            ApiaryCollapseReadiness {
                active_hive_count: 1,
                ..ApiaryCollapseReadiness::default()
            }
        );
        assert_eq!(service.collapse(20).unwrap(), LocalApiaryContext::Personal);
        assert!(matches!(
            service.collapse_readiness(),
            Err(ApplicationError::Store(TaskStoreError::ApiaryNotFound))
        ));
        assert!(matches!(
            service.store.get_apiary(apiary.id),
            Ok(preserved) if preserved.id == apiary.id
        ));
    }

    #[test]
    fn apiary_project_promotion_is_one_revalidated_application_command() {
        let store = TaskStore::in_memory().unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        store
            .replace_jira_status_mappings(
                binding.id,
                &[JiraStatusMapping {
                    jira_status_id: "1".into(),
                    jira_status_name: "To Do".into(),
                    task_state: TaskState::Ready,
                }],
            )
            .unwrap();
        let service = ApiaryService::new(store);
        let LocalApiaryContext::Federated { apiary, .. } = service
            .create_from_personal_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap()
        else {
            panic!("expected a federated Hive");
        };

        let promoted = service.promote_jira_binding(binding.id, 20).unwrap();
        assert_eq!(promoted.apiary_id, apiary.id);
        assert_eq!(service.promoted_jira_projects().unwrap(), vec![promoted]);
    }

    #[test]
    fn apiary_join_checks_use_catalog_evidence_and_block_native_integration() {
        let identity = TaskStore::in_memory()
            .unwrap()
            .local_hive_identity()
            .unwrap();
        let jira_apiary = Apiary::new("Garden", identity.operator.id, SharedWorkBackend::Jira);
        let ready_connection = apiary_join_checks(&jira_apiary, JiraConnectionState::Ready, true);
        assert_eq!(ready_connection.integration, ApiaryJoinCheckState::Ready);
        assert_eq!(ready_connection.project_access, ApiaryJoinCheckState::Ready);
        assert_eq!(
            apiary_join_checks(&jira_apiary, JiraConnectionState::Ready, false).project_access,
            ApiaryJoinCheckState::Blocked
        );

        let native_apiary = Apiary::new("Orchard", identity.operator.id, SharedWorkBackend::Native);
        assert_eq!(
            apiary_join_checks(&native_apiary, JiraConnectionState::Ready, true).integration,
            ApiaryJoinCheckState::Blocked
        );
    }

    #[test]
    fn worker_visibility_is_limited_to_its_active_assignment() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let mine = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Mine",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let other = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Other",
                "",
                TaskPriority::Normal,
                "/workspace/other",
            )
            .unwrap();
        service
            .assign_task(AgentPrincipal::from(&queen), mine.id, worker.id)
            .unwrap();
        let queen_activity = service.store().list_task_activity(mine.id, 10).unwrap();
        let queen_id = queen.id.to_string();
        assert!(queen_activity.events.iter().all(|entry| {
            entry.actor_kind == swarm_domain::TaskActivityActorKind::Worker
                && entry.actor_id.as_deref() == Some(queen_id.as_str())
        }));

        let current = service.store().get_worker_profile(worker.id).unwrap();
        assert_eq!(
            service
                .list_visible_tasks(AgentPrincipal::from(&current))
                .unwrap(),
            [service.store().get_task(mine.id).unwrap()]
        );
        assert!(service.store().get_task(other.id).is_ok());

        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            service.transition_operator_task(mine.id, state).unwrap();
        }
        service
            .store()
            .record_task_deployment(mine.id, "production", "release 42", 1_000)
            .unwrap();
        service
            .transition_operator_task_with_note(
                mine.id,
                TaskState::Completed,
                "Desktop and Android verification passed; release is live.",
            )
            .unwrap();
        let activity = service.store().list_task_activity(mine.id, 20).unwrap();
        assert!(activity.events[2..].iter().all(|entry| {
            entry.actor_kind == swarm_domain::TaskActivityActorKind::Operator
                && entry.actor_id.is_none()
        }));
        assert!(
            service
                .list_visible_tasks(AgentPrincipal::from(&current))
                .unwrap()
                .is_empty()
        );
        assert!(
            service
                .list_visible_tasks(AgentPrincipal::from(&queen))
                .unwrap()
                .iter()
                .any(|task| task.id == mine.id)
        );
    }

    /// A worker records work and cannot route or bless it. Creating a draft
    /// is deliberately on the near side of that line: the work a worker finds
    /// is lost otherwise, and a draft is inert until someone readies it.
    #[test]
    fn a_worker_records_work_but_cannot_route_or_approve_it() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let running_worker = service.store().get_worker_profile(worker.id).unwrap();
        let worker_principal = AgentPrincipal::from(&running_worker);
        let task = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Guarded",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .assign_task(AgentPrincipal::from(&queen), task.id, worker.id)
            .unwrap();

        let recorded = service
            .create_task(
                worker_principal,
                "Follow-up the worker found",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        assert_eq!(recorded.state, TaskState::Draft);
        assert!(recorded.assigned_worker_id.is_none());
        assert!(matches!(
            service.assign_task(worker_principal, recorded.id, worker.id),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(matches!(
            service.assign_task(worker_principal, task.id, worker.id),
            Err(ApplicationError::NotAuthorized)
        ));
        // STILL REFUSED, and now it says why and what to do instead. The
        // refusal changing shape here is the point: a worker that reads
        // "not authorized" goes looking at its assignment and its role, which
        // is the wrong place — the answer is a lifecycle rule and the route
        // that IS open to it.
        let refused = service.transition_task(worker_principal, task.id, TaskState::Completed, "");
        let Err(ApplicationError::TransitionNotPermitted(reason)) = refused else {
            panic!("a worker must not be able to complete its own work: {refused:?}");
        };
        assert!(
            reason.contains("OTHER than the author") && reason.contains("Review"),
            "and the refusal has to name the rule and the remedy: {reason}"
        );
    }

    #[test]
    fn operator_and_queen_completion_require_verification_evidence() {
        let (service, queen, worker) = setup();
        let task = service
            .create_task(
                AgentPrincipal::from(&queen),
                "Prove completion",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            service.transition_operator_task(task.id, state).unwrap();
        }

        assert!(matches!(
            service.transition_operator_task_with_note(task.id, TaskState::Completed, "  "),
            Err(ApplicationError::Store(
                TaskStoreError::CompletionEvidenceRequired
            ))
        ));
        assert!(matches!(
            service.transition_task(
                AgentPrincipal::from(&queen),
                task.id,
                TaskState::Completed,
                ""
            ),
            Err(ApplicationError::Store(
                TaskStoreError::CompletionEvidenceRequired
            ))
        ));
        // Prose no longer closes a task on its own. Operator ruling,
        // 2026-08-21: nothing completes without a deployment, whatever the
        // task's source.
        assert!(matches!(
            service.transition_task(
                AgentPrincipal::from(&queen),
                task.id,
                TaskState::Completed,
                "Tests passed and the approved release is live.",
            ),
            Err(ApplicationError::Store(
                TaskStoreError::CompletionEvidenceRequired
            ))
        ));
        service
            .store()
            .record_task_deployment(task.id, "production", "release 42", 1_000)
            .unwrap();
        let completed = service
            .transition_task(
                AgentPrincipal::from(&queen),
                task.id,
                TaskState::Completed,
                "Tests passed and the approved release is live.",
            )
            .unwrap();
        assert_eq!(completed.state, TaskState::Completed);
        assert_eq!(
            service
                .store()
                .list_task_activity(task.id, 10)
                .unwrap()
                .events
                .last()
                .unwrap()
                .note,
            "Tests passed and the approved release is live."
        );
    }

    /// ⚠️ THE STATE THAT HAD 25 OCCUPANTS IN ONE EVENING. Queen turned 22 drafts
    /// into blocked tasks in 25 minutes and assigned none of them; 24 of the 25
    /// had never been assigned to anyone at any point. Nothing objected, and
    /// `unattended_block_candidates` joins on the assignee, so nothing could see
    /// them afterwards either — the board read 32 blocked while Needs You read 0.
    #[test]
    fn work_nobody_owns_cannot_be_parked_as_blocked() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Under-specified",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();

        let refused = service.transition_task(
            queen_principal,
            task.id,
            TaskState::Blocked,
            "Needs scoping",
        );

        match refused {
            Err(ApplicationError::TransitionNotPermitted(reason)) => assert!(
                reason.contains("somebody who owes it"),
                "the refusal has to name the missing half: {reason}"
            ),
            other => panic!("an ownerless block must be refused, got {other:?}"),
        }
        assert_ne!(
            service.store().get_task(task.id).unwrap().state,
            TaskState::Blocked
        );
    }

    /// Blocked has to name what it waits for, as a link the board can act on.
    /// Only 6 of 32 blocked tasks carried one when this was written.
    #[test]
    fn blocked_work_must_name_what_it_waits_for() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Owned",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Ready, "")
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();

        let refused = service.transition_task(
            queen_principal,
            task.id,
            TaskState::Blocked,
            "Waiting on something",
        );

        match refused {
            Err(ApplicationError::TransitionNotPermitted(reason)) => assert!(
                reason.contains("link rather than a sentence"),
                "an owner alone is not enough: {reason}"
            ),
            other => panic!("a block naming nothing must be refused, got {other:?}"),
        }
    }

    /// Parks `task` on a decision that is PENDING at the moment it blocks.
    ///
    /// Built at the store level on purpose: the unit under test is what happens
    /// when the ANSWER arrives, and driving the block through the transition
    /// guard would test that guard instead. A decision link attaches to Blocked
    /// or Review work, so the link is recorded after the task is parked, which
    /// is also the order the real board produced.
    fn park_on_a_pending_decision(
        service: &TaskService,
        queen: &WorkerProfile,
        worker: &WorkerProfile,
        task: TaskId,
    ) -> swarm_domain::DecisionRequestId {
        let actions = vec!["Keep parked".to_owned(), "Release it".to_owned()];
        let decision = service
            .store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Park or release?",
                summary: "Fixture.",
                reason: "Fixture.",
                risk: "",
                evidence: "",
                suggested_action: "Keep parked",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        service
            .store
            .transition_task(task, TaskState::Ready)
            .unwrap();
        service
            .store
            .transition_task(task, TaskState::Active)
            .unwrap();
        service
            .store
            .transition_task(task, TaskState::Blocked)
            .unwrap();
        let revision = service
            .store
            .queen_task_review_evidence(task)
            .unwrap()
            .evidence_revision;
        service
            .store
            .add_task_decision_link(
                task,
                decision.id,
                "Waiting on the operator.",
                &revision,
                // Recording a decision link is Queen's, not a worker's.
                &TaskActivityActor::worker(queen.id),
                2_000,
            )
            .unwrap();
        decision.id
    }

    /// ⚠️ THE GUARD TOLD A WORKER TO NAME A BLOCKER IT HAD ALREADY NAMED.
    ///
    /// `swarm_request_decision` raises a decision FROM a task, which records the
    /// gate as `decision_requests.task_id` — primary membership — and writes no
    /// row in `task_decision_links`. `task_has_structured_blocker` read only the
    /// link table, so the commonest gating in the system was invisible to it and
    /// the block was refused as "under-specified".
    ///
    /// This is a LIVE instance, not a shape: on the board the day it was found,
    /// 6 of the 13 blocked tasks that appeared link-less were gated this way.
    /// Whoever hit it would have been told to pass an id for something already
    /// recorded, with nothing to pass that would have satisfied the check.
    #[test]
    fn a_block_gated_by_the_decision_raised_from_it_is_not_called_under_specified() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Gated by its own decision",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let actions = vec!["Go".to_owned(), "Stop".to_owned()];
        service
            .store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: Some(task.id),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Which way?",
                summary: "Fixture.",
                reason: "Fixture.",
                risk: "",
                evidence: "",
                suggested_action: "Go",
                allowed_actions: &actions,
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        assert!(
            service
                .store
                .task_decision_links(task.id)
                .unwrap()
                .is_empty(),
            "the fixture must exercise PRIMARY membership, with no link row at all"
        );

        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();
        let principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };
        service
            .transition_task_naming_blocker(
                principal,
                task.id,
                TaskState::Active,
                "Starting",
                None,
                1_000,
            )
            .unwrap();

        service
            .transition_task_naming_blocker(
                principal,
                task.id,
                TaskState::Blocked,
                "Waiting on the decision this task raised.",
                None,
                2_000,
            )
            .expect("a gate recorded as primary membership IS a named blocker");
    }

    /// ⚠️ THE ANSWER IS WHAT TURNS A WAIT INTO A PARK, AND NOTHING NOTICED.
    ///
    /// This is the case a check at BLOCK time can never catch, and the reason
    /// that version was abandoned. The task blocks on a PENDING decision, which
    /// is correct and needs no date — the board can see exactly what will end
    /// it. Seven hours later on the real board, the operator answered, and at
    /// that instant the task stopped having anything to wait for. Its row did
    /// not change. Its state did not change. Only the meaning did.
    ///
    /// Measured: five tasks linked to 01a08715-9309 at 2026-09-09 12:52 while
    /// pending; it resolved at 20:06 and they sat invisible afterwards.
    #[test]
    fn answering_a_decision_asks_about_the_work_it_leaves_parked() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Parked by an answer",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let decision = park_on_a_pending_decision(&service, &queen, &worker, task.id);
        let before = service.store.list_decision_requests().unwrap().len();

        service
            .resolve_operator_decision(decision, "Keep parked", "", "test")
            .unwrap();

        // ⚠️ NOT ASSERTED BY RE-RUNNING THE DETECTION HERE. That read would be
        // confounded by this very feature: the question it raises is itself a
        // PENDING decision on the same task, which the detection then correctly
        // excludes. Asserting on the raised question is the only uncontaminated
        // observation available after the fact.
        let after = service.store.list_decision_requests().unwrap();
        assert_eq!(
            after.len(),
            before + 1,
            "answering must raise ONE question about the work it left with no way back"
        );
        let raised = after
            .iter()
            .find(|request| request.title == "Work left parked by an answer")
            .expect("the follow-up question exists");
        assert!(
            raised.summary.contains(&task.id.to_string()),
            "it must name the task it is about: {}",
            raised.summary
        );
        assert_eq!(
            service.store.get_task(task.id).unwrap().state,
            TaskState::Blocked,
            "the task STAYS PARKED — only the question comes back"
        );
    }

    /// ⚠️ AND IT MUST STAY SILENT WHEN SOMETHING ELSE STILL ENDS THE WAIT.
    ///
    /// A task that also waits on unfinished work has a terminating condition
    /// after this answer, so there is nothing to ask about. Without this the
    /// rule would raise a card every time any decision resolved anywhere near a
    /// blocked task, which is the "queue of near-identical cards" failure the
    /// operator explicitly rejected.
    #[test]
    fn answering_asks_nothing_when_the_task_still_waits_on_real_work() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let still_open = service
            .create_task(
                queen_principal,
                "Still open",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let task = service
            .create_task(
                queen_principal,
                "Waits on both",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let decision = park_on_a_pending_decision(&service, &queen, &worker, task.id);
        service
            .store
            .add_task_prerequisite(
                task.id,
                still_open.id,
                "Also waiting on this",
                // Recording a prerequisite is Queen's, like a decision link.
                &TaskActivityActor::worker(queen.id),
                2_500,
            )
            .unwrap();
        let before = service.store.list_decision_requests().unwrap().len();

        service
            .resolve_operator_decision(decision, "Keep parked", "", "test")
            .unwrap();

        assert_eq!(
            service.store.list_decision_requests().unwrap().len(),
            before,
            "work that still waits on an unfinished task needs no question asked"
        );
    }

    /// ⚠️ THE BOARD NAMES YOUR BLOCKER AND THEN REFUSES TO SHOW IT TO YOU.
    ///
    /// A blocked worker sees THAT it is blocked and by which task — the
    /// prerequisite arrives on its own task with a title and a state — and could
    /// not read the task it was waiting on. Hit first-hand on 01a091a1-4b43,
    /// where the prerequisite's answer had to be read out of the database
    /// directly because `swarm_read_task_history` refused it.
    ///
    /// Narrow on purpose: a prerequisite RECORDED on a task this worker holds,
    /// and a read only. Nothing about tasks the board has not already named to
    /// this worker as its own blocker.
    #[test]
    fn a_worker_may_read_the_task_its_own_work_waits_on() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let waits_on = service
            .create_task(
                queen_principal,
                "The thing it waits on",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let mine = service
            .create_task(
                queen_principal,
                "Waiting",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        service
            .assign_task(queen_principal, mine.id, worker.id)
            .unwrap();
        let worker_principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };

        // Before the link exists it is somebody else's task and stays refused.
        assert!(
            service
                .may_read_task(worker_principal, waits_on.id)
                .is_err(),
            "an unrelated task must not become readable just by existing"
        );

        service
            .store
            .add_task_prerequisite(
                mine.id,
                waits_on.id,
                "Named by the worker",
                &TaskActivityActor::worker(worker.id),
                1,
            )
            .unwrap();

        service
            .may_read_task(worker_principal, waits_on.id)
            .expect("the task the board says you are waiting on must be readable");
    }

    /// ⚠️ AND IT MUST NOT WIDEN INTO A GENERAL BOARD READ. The link is what
    /// grants the read; another worker's prerequisite grants nothing.
    #[test]
    fn a_prerequisite_of_somebody_elses_task_stays_refused() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let waits_on = service
            .create_task(
                queen_principal,
                "Somebody else's blocker",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        // Assigned to NOBODY, so the prerequisite below is not this worker's.
        let theirs = service
            .create_task(
                queen_principal,
                "Not mine",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .store
            .add_task_prerequisite(
                theirs.id,
                waits_on.id,
                "Named by Queen",
                &TaskActivityActor::worker(queen.id),
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();

        assert!(
            service
                .may_read_task(
                    AgentPrincipal {
                        worker_id: worker.id,
                        role: WorkerRole::Worker,
                        active_session_id: Some(session),
                    },
                    waits_on.id
                )
                .is_err(),
            "a prerequisite recorded on work this worker does not hold grants nothing"
        );
    }

    /// ⚠️ THE AFFORDANCE THIS CONTROL SHIPPED WITHOUT, for one working day.
    ///
    /// The guard above demanded a structured blocker and the refusal named two
    /// tools to record one — both Queen-only. A worker with a real, evidenced
    /// blocker was told to do something it had no way to do, and because the
    /// message named specific tools it read as a malformed call rather than a
    /// permission boundary. The honest response is to retry, reword, and leave
    /// the task Active: the exact silent stall the guard exists to prevent,
    /// reached from the other side. Hit twice in one day before it was found.
    #[test]
    fn a_worker_can_name_its_own_blocker_on_the_call_that_blocks() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let waits_on = service
            .create_task(
                queen_principal,
                "The thing it waits on",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let task = service
            .create_task(
                queen_principal,
                "Waiting",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Ready, "")
            .unwrap();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();
        let worker_principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };
        service
            .transition_task(worker_principal, task.id, TaskState::Active, "")
            .unwrap();

        let blocked = service
            .transition_task_naming_blocker(
                worker_principal,
                task.id,
                TaskState::Blocked,
                "Waiting on the other task",
                Some(BlockerLink::Prerequisite(waits_on.id)),
                1,
            )
            .expect("a worker naming its blocker must be able to park its own work");

        assert_eq!(blocked.state, TaskState::Blocked);
        assert!(
            service.store.task_has_structured_blocker(task.id).unwrap(),
            "the link must actually be recorded, not merely accepted"
        );
    }

    /// ⚠️ THE REQUIREMENT IS UNCHANGED; ONLY ITS AFFORDANCE MOVED.
    /// Supplying the link inline must not become a way to block without one.
    #[test]
    fn a_worker_still_cannot_block_while_naming_nothing() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Waiting",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Ready, "")
            .unwrap();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();
        let worker_principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };
        service
            .transition_task(worker_principal, task.id, TaskState::Active, "")
            .unwrap();

        match service.transition_task_naming_blocker(
            worker_principal,
            task.id,
            TaskState::Blocked,
            "Waiting on something",
            None,
            1,
        ) {
            Err(ApplicationError::TransitionNotPermitted(reason)) => assert!(
                reason.contains("link rather than a sentence"),
                "the guard must still bite: {reason}"
            ),
            other => panic!("a block naming nothing must still be refused, got {other:?}"),
        }
    }

    /// ⚠️ NAMING A BLOCKER IS SAFE; UNNAMING ONE IS NOT.
    ///
    /// The add path was widened so an assigned worker can record what its own
    /// work waits on. The remove path must NOT follow it: a worker that could
    /// delete its own prerequisite could walk itself out of the gate it was
    /// parked behind, which is a worse failure than the stall this fixed.
    #[test]
    fn a_worker_cannot_unname_the_blocker_it_is_parked_behind() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let waits_on = service
            .create_task(
                queen_principal,
                "The thing it waits on",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let task = service
            .create_task(
                queen_principal,
                "Waiting",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();
        service
            .store
            .add_task_prerequisite(
                task.id,
                waits_on.id,
                "Named by the worker",
                &TaskActivityActor::worker(worker.id),
                1,
            )
            .unwrap();

        let refused = service.store.remove_task_prerequisite(
            task.id,
            waits_on.id,
            "Trying to let myself out",
            &TaskActivityActor::worker(worker.id),
            2,
        );

        assert!(
            refused.is_err(),
            "a worker removing its own prerequisite must be refused, got {refused:?}"
        );
        assert!(
            service.store.task_has_structured_blocker(task.id).unwrap(),
            "and the blocker must survive the attempt"
        );
    }

    /// ⚠️ ORDER IS THE SECURITY PROPERTY. Ownership is proven BEFORE any link is
    /// written, so a worker cannot annotate work that is not its own by
    /// attaching a blocker to somebody else's task on the way past.
    #[test]
    fn naming_a_blocker_on_a_task_that_is_not_yours_writes_nothing() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let waits_on = service
            .create_task(
                queen_principal,
                "The thing it waits on",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        // Never assigned to this worker.
        let other = service
            .create_task(
                queen_principal,
                "Somebody else's",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();

        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        // A RUNNING worker, so the refusal is about ownership rather than
        // liveness — otherwise this would pass for the wrong reason.
        let refused = service.transition_task_naming_blocker(
            AgentPrincipal {
                worker_id: worker.id,
                role: WorkerRole::Worker,
                active_session_id: Some(session),
            },
            other.id,
            TaskState::Blocked,
            "Not mine",
            Some(BlockerLink::Prerequisite(waits_on.id)),
            1,
        );

        assert!(
            matches!(refused, Err(ApplicationError::NotAuthorized)),
            "a worker must not touch a task it does not hold, got {refused:?}"
        );
        assert!(
            !service.store.task_has_structured_blocker(other.id).unwrap(),
            "and the refusal must leave NOTHING written behind"
        );
    }

    /// ⚠️ THE DEADLOCK THIS NEARLY SHIPPED WITH, and the reason it is a test.
    /// A prerequisite could only be added to work that was ALREADY blocked,
    /// while blocking now requires the prerequisite first — so neither could
    /// happen. In production the only symptom would have been a refusal that
    /// reads perfectly reasonably on its own.
    #[test]
    fn a_prerequisite_can_be_named_before_the_work_is_parked() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Waits",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let blocker = service
            .create_task(
                queen_principal,
                "Waited on",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Ready, "")
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();

        // Named while the work is Ready — impossible before this change.
        service
            .store()
            .add_task_prerequisite(
                task.id,
                blocker.id,
                "Cannot start until the other lands",
                &TaskActivityActor::worker(queen.id),
                9,
            )
            .unwrap();

        let parked = service
            .transition_task(
                queen_principal,
                task.id,
                TaskState::Blocked,
                "Waiting on the other task",
            )
            .unwrap();

        assert_eq!(parked.state, TaskState::Blocked);
    }

    #[test]
    fn queen_must_wake_the_assigned_worker_before_starting_or_resuming_work() {
        let (service, queen, worker) = setup();
        let queen_principal = AgentPrincipal::from(&queen);
        let task = service
            .create_task(
                queen_principal,
                "Wake before work",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Ready, "")
            .unwrap();
        service
            .assign_task(queen_principal, task.id, worker.id)
            .unwrap();

        assert!(matches!(
            service.transition_task(queen_principal, task.id, TaskState::Active, "Starting"),
            Err(ApplicationError::WorkerNotRunning)
        ));
        assert_eq!(
            service.store().get_task(task.id).unwrap().state,
            TaskState::Ready
        );

        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let active = service
            .transition_task(queen_principal, task.id, TaskState::Active, "Worker loaded")
            .unwrap();
        assert_eq!(active.state, TaskState::Active);

        // Blocked now has to name what it waits for, so this setup names one.
        // The rule exists because 25 tasks were parked in one evening naming
        // nothing, and this test blocked a task as a step on the way to
        // something else — exactly the habit the rule is there to stop.
        let blocker = service
            .store()
            .create_task("The thing it waits on", "/workspace")
            .unwrap();
        service
            .store()
            .add_task_prerequisite(
                task.id,
                blocker.id,
                "Waits on the other task",
                &TaskActivityActor::worker(queen.id),
                9,
            )
            .unwrap();
        service
            .transition_task(queen_principal, task.id, TaskState::Blocked, "Waiting")
            .unwrap();
        service.store().release_worker_session(session_id).unwrap();
        assert!(matches!(
            service.transition_task(queen_principal, task.id, TaskState::Active, "Resume"),
            Err(ApplicationError::WorkerNotRunning)
        ));
        assert_eq!(
            service.store().get_task(task.id).unwrap().state,
            TaskState::Blocked
        );
    }

    /// A decision request with the fields this test does not vary.
    fn decision_input(
        task_id: Option<TaskId>,
        kind: DecisionRequestKind,
        title: &str,
        actions: &[&str],
    ) -> DecisionRequestInput {
        DecisionRequestInput {
            task_id,
            kind,
            urgency: DecisionUrgency::Normal,
            title: title.into(),
            summary: "Whether to proceed, and what it costs if we do not.".into(),
            reason: "Two valid implementations remain".into(),
            risk: String::new(),
            evidence: "Both prototypes pass".into(),
            suggested_action: actions[0].into(),
            allowed_actions: actions.iter().map(|action| (*action).to_string()).collect(),
            operator_actions: Vec::new(),
            questions: Vec::new(),
            deadline: None,
            requested_command: None,
        }
    }

    #[test]
    fn decision_overlaps_are_read_only_queen_candidates_and_clear_after_resolution() {
        let (service, queen, worker) = setup();
        let principal = AgentPrincipal::from(&queen);
        let task = service
            .store
            .create_task("Same gate", "/workspace/petal")
            .unwrap();
        let first = service
            .create_decision(
                principal,
                &decision_input(
                    Some(task.id),
                    DecisionRequestKind::Help,
                    "Operator execution",
                    &["I will do it"],
                ),
            )
            .unwrap();
        let mut command = decision_input(
            Some(task.id),
            DecisionRequestKind::Approval,
            "Worker permission",
            &["Hold"],
        );
        command.requested_command = Some("fictional-command --fixture".into());
        let second = service.create_decision(principal, &command).unwrap();
        let before = service.store.list_decision_requests().unwrap();
        assert!(matches!(
            service.queen_pending_decision_overlaps(AgentPrincipal::from(&worker)),
            Err(ApplicationError::NotAuthorized)
        ));
        let snapshot = service.queen_pending_decision_overlaps(principal).unwrap();
        assert!(!snapshot.truncated);
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].task_id, task.id);
        assert_eq!(snapshot.groups[0].pending_count, 2);
        assert!(snapshot.groups[0].decision_ids.contains(&first.id));
        assert!(snapshot.groups[0].decision_ids.contains(&second.id));
        assert_eq!(service.store.list_decision_requests().unwrap(), before);
        service
            .resolve_operator_decision(first.id, "I will do it", "", "test")
            .unwrap();
        assert!(
            service
                .queen_pending_decision_overlaps(principal)
                .unwrap()
                .groups
                .is_empty()
        );
        assert_eq!(
            service.store.get_decision_request(second.id).unwrap(),
            second
        );
    }

    #[test]
    fn decision_overlap_output_has_explicit_group_and_request_bounds() {
        let (service, queen, _) = setup();
        let principal = AgentPrincipal::from(&queen);
        for index in 0..33 {
            let task = service
                .store
                .create_task(&format!("Gate {index}"), "/workspace/petal")
                .unwrap();
            for kind in [DecisionRequestKind::Help, DecisionRequestKind::Approval] {
                service
                    .create_decision(
                        principal,
                        &decision_input(Some(task.id), kind, "Check scope", &["Hold"]),
                    )
                    .unwrap();
            }
        }
        let snapshot = service.queen_pending_decision_overlaps(principal).unwrap();
        assert!(snapshot.truncated);
        assert_eq!(snapshot.groups.len(), 32);
        let task = snapshot.groups[0].task_id;
        for index in 0..20 {
            service
                .create_decision(
                    principal,
                    &decision_input(
                        Some(task),
                        DecisionRequestKind::Help,
                        &format!("Another scope {index}"),
                        &["Hold"],
                    ),
                )
                .unwrap();
        }
        let snapshot = service.queen_pending_decision_overlaps(principal).unwrap();
        assert!(snapshot.truncated);
        let group = snapshot
            .groups
            .iter()
            .find(|group| group.task_id == task)
            .unwrap();
        assert_eq!(group.pending_count, 22);
        assert_eq!(group.decision_ids.len(), 16);
    }

    #[test]
    fn withdrawal_rejects_stale_sessions_and_allows_current_requester() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };
        let request = service
            .create_decision(
                principal,
                &decision_input(
                    None,
                    DecisionRequestKind::Help,
                    "No longer blocked",
                    &["Wait"],
                ),
            )
            .unwrap();
        let stale = AgentPrincipal {
            active_session_id: Some(WorkerSessionId::new()),
            ..principal
        };
        assert!(matches!(
            service.withdraw_agent_decision(stale, request.id, "Recovered"),
            Err(ApplicationError::WorkerNotRunning)
        ));
        assert_eq!(
            service
                .withdraw_agent_decision(principal, request.id, "Recovered")
                .unwrap()
                .state,
            swarm_domain::DecisionRequestState::Withdrawn
        );
    }

    #[test]
    fn unrelated_pending_decisions_cannot_hide_a_workers_resolved_ruling() {
        let (service, queen, worker) = setup();
        let principal = AgentPrincipal::from(&worker);
        let own = service
            .create_decision(
                principal,
                &decision_input(None, DecisionRequestKind::Input, "My question", &["wait"]),
            )
            .unwrap();
        service
            .resolve_operator_decision(own.id, "wait", "", "test")
            .unwrap();
        for _ in 0..swarm_persistence::MAX_DECISION_RESULTS {
            service
                .create_decision(
                    AgentPrincipal::from(&queen),
                    &decision_input(None, DecisionRequestKind::Input, "Other work", &["wait"]),
                )
                .unwrap();
        }
        assert!(
            service
                .list_visible_decisions(None)
                .unwrap()
                .iter()
                .all(|d| d.id != own.id)
        );
        let visible = service.list_visible_decisions(Some(principal)).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, own.id);
    }

    #[test]
    fn decision_visibility_and_task_correlation_follow_agent_authority() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let running_worker = service.store().get_worker_profile(worker.id).unwrap();
        let worker_principal = AgentPrincipal::from(&running_worker);
        let queen_principal = AgentPrincipal::from(&queen);
        let assigned = service
            .create_task(
                queen_principal,
                "Assigned",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        let foreign = service
            .create_task(
                queen_principal,
                "Foreign",
                "",
                TaskPriority::Normal,
                "/workspace/other",
            )
            .unwrap();
        service
            .assign_task(queen_principal, assigned.id, worker.id)
            .unwrap();

        let worker_request = service
            .create_decision(
                worker_principal,
                &decision_input(
                    Some(assigned.id),
                    DecisionRequestKind::Input,
                    "Choose the safer path",
                    &["durable", "minimal"],
                ),
            )
            .unwrap();
        service
            .create_decision(
                queen_principal,
                &decision_input(
                    None,
                    DecisionRequestKind::Approval,
                    "Approve release",
                    &["ship", "hold"],
                ),
            )
            .unwrap();

        assert_eq!(
            service
                .list_visible_decisions(Some(worker_principal))
                .unwrap(),
            [worker_request]
        );
        assert_eq!(
            service
                .list_visible_decisions(Some(queen_principal))
                .unwrap()
                .len(),
            2
        );
        assert!(matches!(
            service.create_decision(
                worker_principal,
                &DecisionRequestInput {
                    task_id: Some(foreign.id),
                    kind: DecisionRequestKind::Help,
                    urgency: DecisionUrgency::Normal,
                    title: "Foreign work".into(),
                    summary: "Whether to proceed, and what it costs if we do not.".into(),
                    reason: "Should remain private".into(),
                    risk: String::new(),
                    evidence: String::new(),
                    suggested_action: "Do not allow".into(),
                    allowed_actions: vec!["acknowledge".into()],
                    operator_actions: Vec::new(),
                    questions: Vec::new(),
                    deadline: None,
                    requested_command: None,
                },
            ),
            Err(ApplicationError::NotAuthorized)
        ));
    }

    /// A worker has no channel to Queen: her inbox is written by detectors and
    /// nothing else, so a worker wanting work routed had to interrupt the
    /// operator instead. It can file a draft, but nothing said one was waiting.
    ///
    /// Deliberately not a messaging system. The worker states a fact once, to
    /// the one who routes, and cannot reply or address another worker — there
    /// is nothing here for two workers to argue over.
    #[test]
    fn filing_work_a_worker_cannot_route_tells_queen() {
        let (service, queen, worker) = setup();
        let session_id = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(worker.id, session_id)
            .unwrap();
        let running_worker = service.store().get_worker_profile(worker.id).unwrap();

        let filed = service
            .create_task(
                AgentPrincipal::from(&running_worker),
                "Cross-repository follow-up the worker found",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();

        let waiting = service.store().current_coordinator_attention(0).unwrap();
        let notice = waiting
            .iter()
            .find(|item| item.kind == "worker_filed_draft_attention")
            .expect("Queen is told a worker filed work it cannot route");
        assert_eq!(notice.task_id, Some(filed.id));
        assert_eq!(notice.worker_id, worker.id);

        // Once Queen routes it, the notice has done its job and stops asking.
        service
            .store()
            .transition_task(filed.id, TaskState::Ready)
            .unwrap();
        service
            .assign_task(AgentPrincipal::from(&queen), filed.id, worker.id)
            .unwrap();
        assert!(
            service
                .store()
                .current_coordinator_attention(0)
                .unwrap()
                .iter()
                .all(|item| item.kind != "worker_filed_draft_attention"),
            "a routed draft is no longer waiting on Queen"
        );
    }

    /// Queen filing her own draft needs no notice to herself.
    #[test]
    fn queen_filing_her_own_draft_tells_no_one() {
        let (service, queen, worker) = setup();
        // THE SESSION IS THE WHOLE TEST. The rule reads
        // `role != Queen && let Some(session_id) = active_session_id`, and
        // setup() leaves Queen with no session — so this short-circuited on the
        // SESSION and never reached the role check. Deleting the Queen
        // exemption entirely left this test green, which is the definition of a
        // test that cannot fail. With a session bound it goes red, so the check
        // is real and was merely out of reach.
        //
        // Found 2026-09-02 by the stratified sweep in 01a0635f, not by anyone
        // depending on it — the first instance of this class caught on purpose
        // rather than as collateral inside another fix.
        let queen_session = WorkerSessionId::new();
        service
            .store()
            .bind_worker_session(queen.id, queen_session)
            .unwrap();
        let queen = service.store().get_worker_profile(queen.id).unwrap();
        service
            .create_task(
                AgentPrincipal::from(&queen),
                "Queen's own note",
                "",
                TaskPriority::Normal,
                &worker.workspace,
            )
            .unwrap();
        assert!(
            service
                .store()
                .current_coordinator_attention(0)
                .unwrap()
                .iter()
                .all(|item| item.kind != "worker_filed_draft_attention")
        );
    }

    /// Queen's job is to accept or reject finished work. The outcome
    /// notification carries only an excerpt of a handoff — it has to — and
    /// until this existed the excerpt pointed at task history nothing could
    /// read. She was asked to judge work on evidence she could not see.
    #[test]
    fn queen_can_read_the_whole_handoff_the_excerpt_pointed_at() {
        let (service, queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details("Fix the gate", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        service.store.assign_task(task.id, session).unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Active)
            .unwrap();

        // The kind of report that does not survive an excerpt.
        let handoff = format!(
            "TWO INCIDENTS WERE MERGED INTO ONE TICKET. {}",
            "detail ".repeat(500)
        );
        service
            .transition_task(
                AgentPrincipal {
                    worker_id: worker.id,
                    role: WorkerRole::Worker,
                    active_session_id: Some(session),
                },
                task.id,
                TaskState::Review,
                &handoff,
            )
            .unwrap();

        let page = service
            .read_task_history(AgentPrincipal::from(&queen), task.id, 50)
            .unwrap();

        let notes = page
            .events
            .iter()
            .map(|event| event.note.as_str())
            .collect::<Vec<_>>();
        assert!(
            notes.iter().any(|note| *note == handoff),
            "the complete handoff must be readable, not an excerpt of it"
        );
    }

    #[test]
    fn worker_history_survives_completion_and_session_replacement() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details(
                "Finished evidence",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        service.store.assign_task(task.id, session).unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            service.store.transition_task(task.id, state).unwrap();
        }
        let replacement = WorkerSessionId::new();
        service.store.release_worker_session(session).unwrap();
        service
            .store
            .bind_worker_session(worker.id, replacement)
            .unwrap();
        let principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(replacement),
        };
        assert!(
            !service
                .list_visible_tasks(principal)
                .unwrap()
                .iter()
                .any(|item| item.id == task.id)
        );
        let history = service.read_task_history(principal, task.id, 50).unwrap();
        assert!(!history.events.is_empty());
        service.read_task_evidence(principal, task.id).unwrap();
        service.read_task_messages(principal, task.id).unwrap();
        service
            .read_returned_review_request(principal, task.id)
            .unwrap();
    }

    /// Reading durable history does not grant a worker unrelated task access.
    #[test]
    fn a_worker_cannot_read_the_history_of_a_task_that_is_not_its_own() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let other = service
            .store
            .create_task_with_details("Not yours", "", TaskPriority::Normal, "/workspace/other")
            .unwrap();

        let denied = service.read_task_history(
            AgentPrincipal {
                worker_id: worker.id,
                role: WorkerRole::Worker,
                active_session_id: Some(session),
            },
            other.id,
            50,
        );

        assert!(matches!(denied, Err(ApplicationError::NotAuthorized)));
    }

    /// WHO CAN TAKE WORK OUT OF BLOCKED, established by trying it.
    ///
    /// Recorded because the board says otherwise. The escalation task asserts
    /// "Queen remains the only actor that moves a task out of Blocked... that
    /// property is already built and tested", and the operator asked for the
    /// arbitrator design to be preserved. Neither the lifecycle nor the role
    /// gate enforces it: `Blocked -> Active` is a legal transition and a worker
    /// is permitted to reach Active for its own assignment.
    ///
    /// This test does not argue for changing that -- a worker resuming its own
    /// work when its blocker clears is defensible, and narrowing it would be a
    /// behaviour change nobody asked for. It exists so the next person who
    /// relies on the stronger claim finds out here rather than from a worker
    /// that resumed itself.
    #[test]
    fn a_worker_can_move_its_own_blocked_task_back_to_active() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details("Mine", "", TaskPriority::Normal, "/workspace/worker")
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::operator())
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Active)
            .unwrap();
        service
            .store
            .transition_task_with_note(task.id, TaskState::Blocked, "Blocked on Queen deciding")
            .unwrap();

        let principal = AgentPrincipal {
            worker_id: worker.id,
            role: WorkerRole::Worker,
            active_session_id: Some(session),
        };
        let resumed = service.transition_task(principal, task.id, TaskState::Active, "Resuming");

        assert!(
            resumed.is_ok(),
            "a worker CAN unblock its own task today; the arbitrator property is \
             weaker than the board records it as"
        );
        assert_eq!(
            service.store.get_task(task.id).unwrap().state,
            TaskState::Active
        );
    }

    /// The dead end Queen hit: a worker records that a task had nothing to
    /// deploy, the store waits for an approval, and nothing above the store
    /// could ever give one. Three finished tasks sat in review with no legal
    /// path to completed.
    #[test]
    fn queen_can_approve_work_that_had_nothing_to_deploy() {
        let (service, queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details(
                "Correct a standard",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();
        service.store.assign_task(task.id, session).unwrap();
        service
            .store
            // Reported before claiming, which is the documented path since
            // 2026-09-04: a no-deployment claim needs a commit report to stand
            // on, and an empty list is how "nothing was built" is said. This
            // test is about who may APPROVE a claim, so it takes that route
            // rather than exercising a shape the tools no longer permit.
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        service
            .store
            .claim_completion_exemption(
                task.id,
                "Documentation only; nothing ships.",
                Some(worker.id),
                1_000,
            )
            .unwrap();

        service
            .approve_completion_exemption(
                AgentPrincipal::from(&queen),
                task.id,
                "Read the handoff.",
            )
            .unwrap();

        // The gate is satisfied, so the work can finally be recorded as done.
        service
            .store
            .transition_task(task.id, TaskState::Ready)
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Active)
            .unwrap();
        service
            .store
            .transition_task(task.id, TaskState::Review)
            .unwrap();
        service
            .transition_task(
                AgentPrincipal::from(&queen),
                task.id,
                TaskState::Completed,
                "Approved: nothing to deploy.",
            )
            .unwrap();
    }

    /// The second pair of eyes is the whole point. A worker approving its own
    /// claim would make the gate a formality.
    /// A refusal names the rule AND the route, because the rule alone is not
    /// actionable.
    ///
    /// Queen lost hours on 2026-09-01 to a belief about the lifecycle that was
    /// simply false, and her brief already carried the correct procedure — so
    /// more standing instruction was the one fix with a proven failure record.
    /// This is the same lesson `MalformedIdentifier` records: a refusal that says
    /// nothing true sends the reader to check the wrong thing.
    #[test]
    fn a_refused_transition_says_which_rule_and_what_to_do_instead() {
        let ready = worker_transition_refusal(TaskState::Ready);
        assert!(
            ready.contains("UNSTARTED"),
            "it must say WHY, not just that it is refused: {ready}"
        );
        assert!(
            ready.contains("swarm_message_queen") && ready.contains("Blocked"),
            "and it must name the routes that ARE the worker's: {ready}"
        );

        let completed = worker_transition_refusal(TaskState::Completed);
        assert!(completed.contains("deterministic coordinator"));
        assert!(completed.contains("without another Queen approval"));
        assert!(
            completed.contains("OTHER than the author"),
            "the completion rule is about a second pair of eyes, not about trust: {completed}"
        );
        assert!(
            completed.contains("Review"),
            "and Review is the move that IS available: {completed}"
        );

        // The permitted ones must not read as refusals if they are ever shown.
        for target in [TaskState::Active, TaskState::Blocked, TaskState::Review] {
            assert!(
                worker_transition_refusal(target).contains("may report"),
                "a permitted move must never imply otherwise"
            );
        }
    }

    #[test]
    fn a_worker_cannot_approve_its_own_no_deployment_claim() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details("Spike", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        service.store.assign_task(task.id, session).unwrap();
        service
            .store
            // Same precondition as above; this test is about a worker being
            // refused its own approval, not about how the claim was made.
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        service
            .store
            .claim_completion_exemption(task.id, "Investigation only.", Some(worker.id), 1_000)
            .unwrap();

        let denied = service.approve_completion_exemption(
            AgentPrincipal {
                worker_id: worker.id,
                role: WorkerRole::Worker,
                active_session_id: Some(session),
            },
            task.id,
            "Read the handoff.",
        );

        assert!(matches!(denied, Err(ApplicationError::NotAuthorized)));
    }

    /// The operator ruled "retire all four" and Queen had no way to carry it
    /// out, so four tasks went to Blocked with notes saying they were retired.
    /// Blocked means work waiting on something, so the board was lying and the
    /// next reader would try to unblock them.
    #[test]
    fn queen_can_retire_work_that_should_not_exist_and_the_reason_survives() {
        let (service, queen, _worker) = setup();
        let task = service
            .store
            .create_task_with_details(
                "Measures the legacy swarm",
                "",
                TaskPriority::Normal,
                "/workspace/petal",
            )
            .unwrap();

        service
            .retire_task(
                AgentPrincipal::from(&queen),
                task.id,
                "Operator ruling: the system it measures is gone.",
            )
            .unwrap();

        // Off the live board.
        assert!(
            !service
                .list_tasks()
                .unwrap()
                .iter()
                .any(|open| open.id == task.id)
        );

        // And the reason is what a later reader finds, not a bare "removed".
        let history = service.store.list_task_activity(task.id, 50).unwrap();
        assert!(
            history
                .events
                .iter()
                .any(|event| event.note.contains("the system it measures is gone")),
            "the reason for retiring must survive on the task"
        );
    }

    /// Retiring is a routing judgement. A worker deciding its own work should
    /// not exist is exactly the call Queen is there to make.
    #[test]
    fn a_worker_cannot_retire_a_task() {
        let (service, _queen, worker) = setup();
        let session = WorkerSessionId::new();
        service
            .store
            .bind_worker_session(worker.id, session)
            .unwrap();
        let task = service
            .store
            .create_task_with_details("Still wanted", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();

        let denied = service.retire_task(
            AgentPrincipal {
                worker_id: worker.id,
                role: WorkerRole::Worker,
                active_session_id: Some(session),
            },
            task.id,
            "I would rather not",
        );

        assert!(matches!(denied, Err(ApplicationError::NotAuthorized)));
        assert!(
            service
                .list_tasks()
                .unwrap()
                .iter()
                .any(|open| open.id == task.id)
        );
    }
}
