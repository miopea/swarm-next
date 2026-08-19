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

impl ApiaryService {
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
        now: i64,
    ) -> Result<(OperatorPresence, bool), ApplicationError> {
        let mutation =
            self.store
                .record_presence_observation(device_id, device_class, state, now)?;
        Ok((mutation.presence, mutation.changed))
    }
    /// Lists the complete local Hive queue for an operator or Queen coordinator.
    ///
    /// # Errors
    /// Returns a persistence error when the task snapshot cannot be read.
    pub fn list_tasks(&self) -> Result<Vec<Task>, ApplicationError> {
        self.store.list_tasks().map_err(Into::into)
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
            .remove_task_as(task_id, &TaskActivityActor::operator())
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
        require_completion_evidence(target, note)?;
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

    /// Creates a draft in the local Hive. Only Queen may originate durable work through MCP.
    ///
    /// # Errors
    /// Denies worker callers and propagates validation or persistence failures.
    pub fn create_task(
        &self,
        principal: AgentPrincipal,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, ApplicationError> {
        require_queen(principal)?;
        self.store
            .create_task_with_details_as(
                title,
                description,
                priority,
                workspace,
                &TaskActivityActor::worker(principal.worker_id),
            )
            .map_err(Into::into)
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
                return Err(ApplicationError::NotAuthorized);
            }
            return self
                .store
                .transition_worker_task(task_id, target, note, session_id)
                .map_err(Into::into);
        }
        require_completion_evidence(target, note)?;
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
        let decisions = self.store.list_decision_requests()?;
        Ok(match principal {
            None
            | Some(AgentPrincipal {
                role: WorkerRole::Queen,
                ..
            }) => decisions,
            Some(principal) => decisions
                .into_iter()
                .filter(|decision| decision.requesting_worker_id == principal.worker_id)
                .collect(),
        })
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
                reason: &input.reason,
                risk: &input.risk,
                evidence: &input.evidence,
                suggested_action: &input.suggested_action,
                allowed_actions: &input.allowed_actions,
                questions: &input.questions,
                deadline: input.deadline,
            })
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
        self.store
            .resolve_decision_request(id, action, note, surface)
            .map_err(Into::into)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionRequestInput {
    pub task_id: Option<TaskId>,
    pub kind: DecisionRequestKind,
    pub urgency: DecisionUrgency,
    pub title: String,
    pub reason: String,
    pub risk: String,
    pub evidence: String,
    pub suggested_action: String,
    pub allowed_actions: Vec<String>,
    /// Present makes this an interview rather than a ruling.
    pub questions: Vec<DecisionQuestion>,
    pub deadline: Option<i64>,
}

fn require_queen(principal: AgentPrincipal) -> Result<(), ApplicationError> {
    if principal.role == WorkerRole::Queen {
        Ok(())
    } else {
        Err(ApplicationError::NotAuthorized)
    }
}

fn require_completion_evidence(target: TaskState, note: &str) -> Result<(), ApplicationError> {
    if target == TaskState::Completed && note.trim().is_empty() {
        Err(ApplicationError::Store(
            TaskStoreError::CompletionEvidenceRequired,
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("this agent is not authorized for that outcome")]
    NotAuthorized,
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
    use swarm_domain::{JiraProjectScope, JiraStatusMapping, ProviderKind};
    use swarm_persistence::JiraProjectBindingInput;

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

    #[test]
    fn worker_cannot_create_assign_or_approve_completion() {
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

        assert!(matches!(
            service.create_task(
                worker_principal,
                "Nope",
                "",
                TaskPriority::Normal,
                &worker.workspace
            ),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(matches!(
            service.assign_task(worker_principal, task.id, worker.id),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(matches!(
            service.transition_task(worker_principal, task.id, TaskState::Completed, ""),
            Err(ApplicationError::NotAuthorized)
        ));
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
                &DecisionRequestInput {
                    task_id: Some(assigned.id),
                    kind: DecisionRequestKind::Input,
                    urgency: DecisionUrgency::Normal,
                    title: "Choose the safer path".into(),
                    reason: "Two valid implementations remain".into(),
                    risk: "The wrong choice adds migration work".into(),
                    evidence: "Both prototypes pass".into(),
                    suggested_action: "Use the durable variant".into(),
                    allowed_actions: vec!["durable".into(), "minimal".into()],
                    questions: Vec::new(),
                    deadline: None,
                },
            )
            .unwrap();
        service
            .create_decision(
                queen_principal,
                &DecisionRequestInput {
                    task_id: None,
                    kind: DecisionRequestKind::Approval,
                    urgency: DecisionUrgency::TimeSensitive,
                    title: "Approve release".into(),
                    reason: "The release candidate is ready".into(),
                    risk: String::new(),
                    evidence: "All checks pass".into(),
                    suggested_action: "Ship".into(),
                    allowed_actions: vec!["ship".into(), "hold".into()],
                    questions: Vec::new(),
                    deadline: None,
                },
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
                    reason: "Should remain private".into(),
                    risk: String::new(),
                    evidence: String::new(),
                    suggested_action: "Do not allow".into(),
                    allowed_actions: vec!["acknowledge".into()],
                    questions: Vec::new(),
                    deadline: None,
                },
            ),
            Err(ApplicationError::NotAuthorized)
        ));
    }
}
