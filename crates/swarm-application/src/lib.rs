use swarm_domain::{
    Apiary, ApiaryCollapseReadiness, ApiaryHiveCandidate, ApiaryInvitation, ApiaryInvitationId,
    ApiaryJiraProject, ApiaryJoinCheckState, ApiaryJoinChecks, ApiaryJoinReadiness,
    DecisionRequest, DecisionRequestId, DecisionRequestKind, DecisionUrgency, HiveConnectionCard,
    JiraConnectionState, JiraProjectBindingId, LocalApiaryContext, OperatorPresence,
    PresenceDeviceClass, PresenceDeviceId, PresenceMode, PresenceObservationState,
    SharedWorkBackend, Task, TaskId, TaskPriority, TaskState, WorkerId, WorkerProfile, WorkerRole,
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

impl ApiaryService {
    #[must_use]
    pub const fn new(store: TaskStore) -> Self {
        Self { store }
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
            .create_task_with_details(title, description, priority, workspace)
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
            .update_task_details(task_id, update)
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
            .assign_task_to_worker(task_id, worker_id)
            .map_err(Into::into)
    }

    /// Returns an operator task to the unassigned Hive queue.
    ///
    /// # Errors
    ///
    /// Propagates task validation and persistence failures.
    pub fn unassign_operator_task(&self, task_id: TaskId) -> Result<Task, ApplicationError> {
        self.store.unassign_task(task_id).map_err(Into::into)
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
        self.store
            .transition_task(task_id, target)
            .map_err(Into::into)
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
        self.store
            .transition_task_with_note(task_id, target, note)
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
        self.create_operator_task(title, description, priority, workspace)
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
        self.assign_operator_task(task_id, worker_id)
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
        self.transition_operator_task_with_note(task_id, target, note)
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
    ) -> Result<DecisionRequest, ApplicationError> {
        self.store
            .resolve_decision_request(id, action, note)
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
    pub deadline: Option<i64>,
}

fn require_queen(principal: AgentPrincipal) -> Result<(), ApplicationError> {
    if principal.role == WorkerRole::Queen {
        Ok(())
    } else {
        Err(ApplicationError::NotAuthorized)
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

        let current = service.store().get_worker_profile(worker.id).unwrap();
        assert_eq!(
            service
                .list_visible_tasks(AgentPrincipal::from(&current))
                .unwrap(),
            [service.store().get_task(mine.id).unwrap()]
        );
        assert!(service.store().get_task(other.id).is_ok());

        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            service.transition_operator_task(mine.id, state).unwrap();
        }
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
                    deadline: None,
                },
            ),
            Err(ApplicationError::NotAuthorized)
        ));
    }
}
