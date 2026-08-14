mod agent;
mod attach;
mod attachments;
mod jira;
mod jira_oauth;
mod notifications;
mod terminal_socket;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path as FilePath, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, patch, post, put},
};
use base64ct::{Base64UrlUnpadded, Encoding};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use swarm_application::{ApiaryInvitationOverview, ApiaryService, ApplicationError, TaskService};
use swarm_domain::{
    Apiary, ApiaryInvitation, ApiaryInvitationId, ApiaryJoinReadiness, ControlRoomEventKind,
    DecisionRequestId, HiveIdentity, JiraConnectionState, JiraProjectBindingId, JiraProjectScope,
    JiraStatusMapping, LocalApiaryContext, NotificationPolicy, PresenceDeviceClass,
    PresenceDeviceId, PresenceMode, PresenceObservationState, ProviderConversationId, ProviderKind,
    QueenAutonomyLevel, QueenAutonomyPolicy, SharedWorkBackend, TaskDetailsUpdate, TaskId,
    TaskPriority, TaskState, WorkerAttentionState, WorkerId, WorkerProfile, WorkerSessionId,
};
use swarm_persistence::{
    DecisionDeliveryFailure, DecisionDispatch, JiraIssueSnapshot, JiraProjectBindingInput,
    JiraTransitionFailure, MAX_OPEN_TASKS_PER_ORDER, MAX_TASK_ACTIVITY_NOTE_BYTES,
    MAX_TASK_ACTIVITY_PAGE, NotificationSettings, PresentationColorTheme, PresentationDeviceClass,
    PresentationPreferences, PushSubscriptionInput, TaskDispatch, TaskDispatchFailure,
    TaskOutcomeDispatch, TaskOutcomeFailure, TaskStore, TaskStoreError,
};
use swarm_terminal::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, ClaudeConversationStart,
    CodexConversationStart, HistoryCursor, HostClient, HostRequest, HostResponse, JournalLimits,
    MAX_CANONICAL_SNAPSHOT_BYTES, MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS,
    MIN_TERMINAL_COLUMNS, MIN_TERMINAL_ROWS, ProcessResourceSample, ProviderActivity, TerminalSize,
    classify_provider_activity, sample_current_process,
};
use tokio::{
    sync::{Mutex, Notify, RwLock, Semaphore},
    time::{sleep, timeout},
};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use attach::{ATTACH_GRANT_TTL, AttachGrantError, AttachGrantStore, MAX_ATTACH_GRANTS};
use attachments::{AttachmentError, AttachmentStore, MAX_ATTACHMENT_BYTES};
use terminal_socket::{
    MAX_WEBSOCKET_MESSAGE_BYTES, TERMINAL_GRANT_PROTOCOL_PREFIX, TERMINAL_WEBSOCKET_PROTOCOL,
    serve_terminal_socket,
};

const MAX_TERMINAL_WEBSOCKETS: usize = 32;
const RESOURCE_ADVISORY_BYTES: u64 = 256 * 1024 * 1024;
const RESOURCE_CRITICAL_BYTES: u64 = 512 * 1024 * 1024;
const OPERATOR_SESSION_COOKIE: &str = "swarm_next_operator_session";
const OPERATOR_SESSION_MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;
const WORKER_RECOVERY_STABILITY_SECONDS: i64 = 5 * 60;

#[derive(Clone)]
pub struct AppState {
    terminal_limits: JournalLimits,
    terminal_host: Option<HostClient>,
    operator_token: Option<Arc<str>>,
    attach_grants: Arc<AttachGrantStore>,
    websocket_limit: Arc<Semaphore>,
    task_store: Option<TaskStore>,
    agent_bridge: Option<agent::AgentBridge>,
    worker_lifecycle: Arc<Mutex<()>>,
    coordination_delivery: Arc<Mutex<()>>,
    jira_delivery: Arc<Mutex<()>>,
    worker_errors: Arc<RwLock<HashMap<WorkerId, String>>>,
    worker_recovery_attempts: Arc<RwLock<HashMap<WorkerId, i64>>>,
    provider_activity: Arc<RwLock<HashMap<WorkerSessionId, ProviderActivity>>>,
    control_room_notify: Arc<Notify>,
    notification_sender: Option<notifications::NotificationSender>,
    attachment_store: Option<AttachmentStore>,
    workspace_roots: Arc<Vec<PathBuf>>,
    maintenance_request_path: Option<Arc<PathBuf>>,
    maintenance_timeout: Duration,
    jira_readiness: jira::JiraReadinessProbe,
}

impl AppState {
    #[must_use]
    pub fn new(terminal_limits: JournalLimits) -> Self {
        Self {
            terminal_limits,
            terminal_host: None,
            operator_token: None,
            attach_grants: Arc::new(AttachGrantStore::default()),
            websocket_limit: Arc::new(Semaphore::new(MAX_TERMINAL_WEBSOCKETS)),
            task_store: None,
            agent_bridge: None,
            worker_lifecycle: Arc::new(Mutex::new(())),
            coordination_delivery: Arc::new(Mutex::new(())),
            jira_delivery: Arc::new(Mutex::new(())),
            worker_errors: Arc::new(RwLock::new(HashMap::new())),
            worker_recovery_attempts: Arc::new(RwLock::new(HashMap::new())),
            provider_activity: Arc::new(RwLock::new(HashMap::new())),
            control_room_notify: Arc::new(Notify::new()),
            notification_sender: None,
            attachment_store: None,
            workspace_roots: Arc::new(Vec::new()),
            maintenance_request_path: None,
            maintenance_timeout: Duration::from_secs(45),
            jira_readiness: jira::JiraReadinessProbe::default(),
        }
    }

    /// Enables a bounded Jira Cloud identity probe using operator-owned credentials.
    ///
    /// # Errors
    /// Rejects invalid or insecure Jira base URLs.
    pub fn with_jira_configuration(
        mut self,
        base_url: &str,
        email: impl Into<Arc<str>>,
        api_token: impl Into<Arc<str>>,
    ) -> Result<Self, String> {
        self.jira_readiness = jira::JiraReadinessProbe::configured(base_url, email, api_token)?;
        Ok(self)
    }

    /// Enables operator-driven Atlassian OAuth with host-owned durable tokens.
    ///
    /// # Errors
    /// Rejects invalid public callback URLs or unreadable token storage.
    pub fn with_jira_oauth(
        mut self,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
    ) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| format!("Jira OAuth client could not start: {error}"))?;
        let oauth = jira_oauth::JiraOAuthClient::new(
            client,
            client_id,
            client_secret,
            public_base_url,
            token_path,
        )?;
        self.jira_readiness = jira::JiraReadinessProbe::oauth(oauth);
        Ok(self)
    }

    #[must_use]
    pub fn with_task_store(mut self, task_store: TaskStore) -> Self {
        self.task_store = Some(task_store);
        self
    }

    #[must_use]
    pub fn with_attachment_store(mut self, root: PathBuf) -> Self {
        self.attachment_store = Some(AttachmentStore::new(root));
        self
    }

    #[must_use]
    pub fn with_workspace_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.workspace_roots = Arc::new(roots);
        self
    }

    #[must_use]
    pub fn with_maintenance_request_path(mut self, path: PathBuf) -> Self {
        self.maintenance_request_path = Some(Arc::new(path));
        self
    }

    #[cfg(test)]
    #[must_use]
    fn with_maintenance_timeout(mut self, timeout: Duration) -> Self {
        self.maintenance_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_agent_configuration(
        mut self,
        config_root: PathBuf,
        mcp_url: impl Into<Arc<str>>,
    ) -> Self {
        if let Some(store) = self.task_store.clone() {
            self.agent_bridge = Some(
                agent::AgentBridge::new(
                    store,
                    config_root,
                    mcp_url,
                    self.control_room_notify.clone(),
                )
                .with_jira(self.jira_readiness.clone()),
            );
        }
        self
    }
    /// Enables durable encrypted Web Push delivery for this installation.
    ///
    /// # Errors
    /// Returns key generation, storage, or HTTPS client initialization failures.
    pub fn with_notifications(mut self, subject: impl Into<Arc<str>>) -> Result<Self, String> {
        let store = self
            .task_store
            .clone()
            .ok_or_else(|| "notification setup requires the task store".to_owned())?;
        self.notification_sender = Some(notifications::NotificationSender::initialize(
            store, subject,
        )?);
        Ok(self)
    }

    /// Recovers idempotent tagged notification sends after an API interruption.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn recover_notification_deliveries(&self) -> Result<usize, TaskStoreError> {
        self.task_store.as_ref().map_or(Ok(0), |store| {
            store.recover_notification_deliveries(unix_timestamp())
        })
    }

    /// Sends one bounded batch of currently eligible durable notifications.
    pub async fn deliver_notifications(&self) {
        if let Some(sender) = &self.notification_sender {
            sender.deliver().await;
        }
    }

    /// Automatically imports open Jira issues assigned to the operator when enabled,
    /// then refreshes every Jira issue already owned by this Hive.
    pub async fn reconcile_jira(&self) {
        self.deliver_jira_transitions().await;
        self.deliver_jira_comments().await;
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let bindings = match store.list_jira_project_bindings() {
            Ok(bindings) => bindings,
            Err(error) => {
                tracing::warn!(%error, "Jira reconciliation could not list project bindings");
                return;
            }
        };
        for binding in bindings {
            let links = match store.list_jira_issue_links(binding.id) {
                Ok(links) => links,
                Err(error) => {
                    tracing::warn!(%error, project = %binding.project_key, "Jira reconciliation could not read imported work");
                    continue;
                }
            };
            if binding.auto_sync_assigned && binding.workflow_mapped {
                match self
                    .jira_readiness
                    .assigned_open_issues(&binding.project_id)
                    .await
                {
                    Ok(issues) => {
                        for batch in issues.chunks(100) {
                            let snapshots =
                                batch.iter().map(jira_issue_snapshot).collect::<Vec<_>>();
                            match store.sync_jira_issues(binding.id, &snapshots) {
                                Ok(_) => self.control_room_notify.notify_waiters(),
                                Err(error) => {
                                    tracing::warn!(%error, project = %binding.project_key, "assigned Jira work could not synchronize automatically");
                                    break;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, project = %binding.project_key, "assigned Jira work is temporarily unavailable");
                    }
                }
            }
            if links.is_empty() {
                continue;
            }
            let imported = links
                .iter()
                .map(|link| link.issue_id.clone())
                .collect::<HashSet<_>>();
            let imported_ids = imported.iter().cloned().collect::<Vec<_>>();
            let issues = match self.jira_readiness.linked_issues(&imported_ids).await {
                Ok(issues) => issues,
                Err(error) => {
                    tracing::warn!(%error, project = %binding.project_key, "Jira reconciliation is temporarily unavailable");
                    continue;
                }
            };
            let snapshots = issues
                .iter()
                .filter(|issue| imported.contains(&issue.id))
                .map(jira_issue_snapshot)
                .collect::<Vec<_>>();
            if snapshots.is_empty() {
                continue;
            }
            match store.sync_jira_issues(binding.id, &snapshots) {
                Ok(_) => self.control_room_notify.notify_waiters(),
                Err(error) => {
                    tracing::warn!(%error, project = %binding.project_key, "Jira reconciliation was rejected");
                }
            }
        }
    }

    /// Delivers one bounded batch of durable Jira workflow updates.
    pub async fn deliver_jira_transitions(&self) {
        let _guard = self.jira_delivery.lock().await;
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        deliver_jira_transition_batch(
            store,
            &self.jira_readiness,
            self.control_room_notify.as_ref(),
        )
        .await;
    }

    pub async fn deliver_jira_comments(&self) {
        let _guard = self.jira_delivery.lock().await;
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        deliver_jira_comment_batch(
            store,
            &self.jira_readiness,
            self.control_room_notify.as_ref(),
        )
        .await;
    }

    /// Recovers a crash-interrupted Jira write as explicit uncertainty.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn recover_jira_transition_deliveries(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_jira_transitions)
    }

    /// Recovers crash-interrupted Jira comments as explicit uncertainty.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn recover_jira_comment_deliveries(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_jira_comments)
    }
    #[must_use]
    pub fn with_terminal_host(
        mut self,
        terminal_host: HostClient,
        operator_token: impl Into<Arc<str>>,
    ) -> Self {
        self.terminal_host = Some(terminal_host);
        self.operator_token = Some(operator_token.into());
        self
    }

    /// Reconciles durable worker identities with the terminal host and starts autostart workers.
    pub async fn supervise_workers(&self) {
        let live = match reconcile_worker_bindings(self).await {
            Ok(live) => live,
            Err(error) => {
                tracing::warn!(message = %error.message, "worker supervisor could not inspect the terminal host");
                return;
            }
        };
        let Ok(profiles) = task_store(self).and_then(|store| {
            store
                .list_worker_profiles()
                .map_err(|error| task_store_error(&error))
        }) else {
            tracing::warn!("worker supervisor could not load the durable roster");
            return;
        };
        refresh_provider_activity(self, &profiles, &live).await;
        let now = unix_timestamp();
        {
            let mut attempts = self.worker_recovery_attempts.write().await;
            attempts.retain(|worker_id, attempted_at| {
                !profiles.iter().any(|profile| {
                    profile.id == *worker_id
                        && profile.active_session_id.is_some()
                        && now.saturating_sub(*attempted_at) >= WORKER_RECOVERY_STABILITY_SECONDS
                })
            });
        }
        for profile in profiles
            .into_iter()
            .filter(|profile| profile.autostart && profile.active_session_id.is_none())
        {
            if self.worker_errors.read().await.contains_key(&profile.id) {
                continue;
            }
            let attempted_recovery = self
                .worker_recovery_attempts
                .write()
                .await
                .insert(profile.id, now)
                .is_some();
            if attempted_recovery {
                self.worker_errors.write().await.insert(
                    profile.id,
                    "Worker exited again before recovery was stable. Retry when ready.".to_owned(),
                );
                self.control_room_notify.notify_waiters();
                tracing::warn!(worker_id = %profile.id, worker_name = %profile.name, "autostart worker recovery circuit opened");
                continue;
            }
            if let Err(error) =
                start_worker_process(self, profile.id, TerminalSize::default()).await
            {
                self.worker_errors
                    .write()
                    .await
                    .insert(profile.id, error.message.clone());
                tracing::warn!(worker_id = %profile.id, worker_name = %profile.name, message = %error.message, "autostart worker could not be started");
            }
        }
        self.deliver_coordination().await;
        self.deliver_notifications().await;
    }

    /// Delivers durable coordination only to running workers without a live operator lease.
    pub async fn deliver_coordination(&self) {
        let _guard = self.coordination_delivery.lock().await;
        let (Some(store), Some(client)) = (&self.task_store, &self.terminal_host) else {
            return;
        };
        self.deliver_decision_outcomes(store, client).await;
        self.deliver_task_briefs(store, client).await;
        self.deliver_task_outcomes(store, client).await;
    }

    async fn deliver_decision_outcomes(&self, store: &TaskStore, client: &HostClient) {
        let deliveries = match store.claim_decision_deliveries(unix_timestamp()) {
            Ok(deliveries) => deliveries,
            Err(error) => {
                tracing::warn!(message = %error, "decision delivery queue could not be claimed");
                return;
            }
        };
        for delivery in deliveries {
            let outcome = match submit_terminal_message(
                client,
                delivery.session_id,
                decision_delivery_message(&delivery),
                &delivery_marker(delivery.decision_id),
            )
            .await
            {
                Ok(TerminalSubmission::Acknowledged) => {
                    store.complete_decision_delivery(delivery.decision_id, unix_timestamp())
                }
                Ok(TerminalSubmission::Rejected { code, message }) => {
                    tracing::warn!(decision_id = %delivery.decision_id, worker_id = %delivery.worker_id, %code, %message, "decision delivery was rejected by terminal host");
                    store.fail_decision_delivery(
                        delivery.decision_id,
                        unix_timestamp(),
                        DecisionDeliveryFailure::Retryable,
                    )
                }
                Ok(TerminalSubmission::Uncertain) => store.fail_decision_delivery(
                    delivery.decision_id,
                    unix_timestamp(),
                    DecisionDeliveryFailure::Uncertain,
                ),
                Err(error) => {
                    tracing::warn!(decision_id = %delivery.decision_id, worker_id = %delivery.worker_id, message = %error, "decision delivery result is uncertain");
                    store.fail_decision_delivery(
                        delivery.decision_id,
                        unix_timestamp(),
                        DecisionDeliveryFailure::Uncertain,
                    )
                }
            };
            match outcome {
                Ok(true) => self.control_room_notify.notify_waiters(),
                Ok(false) => {
                    tracing::warn!(decision_id = %delivery.decision_id, "decision delivery claim was no longer active");
                }
                Err(error) => {
                    tracing::warn!(decision_id = %delivery.decision_id, message = %error, "decision delivery outcome could not be persisted");
                }
            }
        }
    }

    async fn deliver_task_briefs(&self, store: &TaskStore, client: &HostClient) {
        let deliveries = match store.claim_task_dispatches(unix_timestamp()) {
            Ok(deliveries) => deliveries,
            Err(error) => {
                tracing::warn!(message = %error, "task dispatch queue could not be claimed");
                return;
            }
        };
        for delivery in deliveries {
            let outcome = match submit_terminal_message(
                client,
                delivery.session_id,
                task_dispatch_message(&delivery),
                &delivery_marker(delivery.task_id),
            )
            .await
            {
                Ok(TerminalSubmission::Acknowledged) => {
                    store.complete_task_dispatch(&delivery.assignment_id, unix_timestamp())
                }
                Ok(TerminalSubmission::Rejected { code, message }) => {
                    tracing::warn!(task_id = %delivery.task_id, worker_id = %delivery.worker_id, %code, %message, "task briefing was rejected by terminal host");
                    store.fail_task_dispatch(
                        &delivery.assignment_id,
                        unix_timestamp(),
                        TaskDispatchFailure::Retryable,
                    )
                }
                Ok(TerminalSubmission::Uncertain) => store.fail_task_dispatch(
                    &delivery.assignment_id,
                    unix_timestamp(),
                    TaskDispatchFailure::Uncertain,
                ),
                Err(error) => {
                    tracing::warn!(task_id = %delivery.task_id, worker_id = %delivery.worker_id, message = %error, "task briefing result is uncertain");
                    store.fail_task_dispatch(
                        &delivery.assignment_id,
                        unix_timestamp(),
                        TaskDispatchFailure::Uncertain,
                    )
                }
            };
            match outcome {
                Ok(true) => self.control_room_notify.notify_waiters(),
                Ok(false) => {
                    tracing::warn!(task_id = %delivery.task_id, "task briefing claim was no longer active");
                }
                Err(error) => {
                    tracing::warn!(task_id = %delivery.task_id, message = %error, "task briefing outcome could not be persisted");
                }
            }
        }
    }
    async fn deliver_task_outcomes(&self, store: &TaskStore, client: &HostClient) {
        let outcomes = match store.claim_task_outcomes(unix_timestamp()) {
            Ok(outcomes) => outcomes,
            Err(error) => {
                tracing::warn!(message = %error, "task outcome queue could not be claimed");
                return;
            }
        };
        for outcome in outcomes {
            let result = match submit_terminal_message(
                client,
                outcome.session_id,
                task_outcome_message(&outcome),
                &delivery_marker(outcome.task_id),
            )
            .await
            {
                Ok(TerminalSubmission::Acknowledged) => {
                    store.complete_task_outcome(&outcome.id, unix_timestamp())
                }
                Ok(TerminalSubmission::Rejected { code, message }) => {
                    tracing::warn!(task_id = %outcome.task_id, reporter_id = %outcome.reporting_worker_id, recipient_id = %outcome.recipient_worker_id, %code, %message, "task outcome was rejected by terminal host");
                    store.fail_task_outcome(
                        &outcome.id,
                        unix_timestamp(),
                        TaskOutcomeFailure::Retryable,
                    )
                }
                Ok(TerminalSubmission::Uncertain) => store.fail_task_outcome(
                    &outcome.id,
                    unix_timestamp(),
                    TaskOutcomeFailure::Uncertain,
                ),
                Err(error) => {
                    tracing::warn!(task_id = %outcome.task_id, reporter_id = %outcome.reporting_worker_id, message = %error, "task outcome result is uncertain");
                    store.fail_task_outcome(
                        &outcome.id,
                        unix_timestamp(),
                        TaskOutcomeFailure::Uncertain,
                    )
                }
            };
            match result {
                Ok(true) => self.control_room_notify.notify_waiters(),
                Ok(false) => {
                    tracing::warn!(task_id = %outcome.task_id, "task outcome claim was no longer active");
                }
                Err(error) => {
                    tracing::warn!(task_id = %outcome.task_id, message = %error, "task outcome could not be persisted");
                }
            }
        }
    }
    /// Makes crash-interrupted delivery explicit before any new dispatch is attempted.
    ///
    /// # Errors
    /// Returns a persistence error when recovery cannot be recorded.
    pub fn recover_decision_deliveries(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_decision_deliveries)
    }
    /// Makes crash-interrupted task briefings explicit before new dispatch.
    ///
    /// # Errors
    /// Returns a persistence error when recovery cannot be recorded.
    pub fn recover_task_dispatches(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_task_dispatches)
    }
    /// Makes crash-interrupted Queen handoffs explicit before new dispatch.
    ///
    /// # Errors
    /// Returns a persistence error when recovery cannot be recorded.
    pub fn recover_task_outcomes(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_task_outcomes)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum TerminalSubmission {
    Acknowledged,
    Rejected { code: String, message: String },
    Uncertain,
}

/// Submits a coordination prompt after observing it in host-owned output.
///
/// Claude's interactive input can render a carriage return that arrives in the
/// same PTY read as a long prompt without accepting the prompt. This waits for
/// the host's output sequence to advance and confirms a bounded delivery marker
/// in the canonical snapshot before sending Enter. Ordering therefore depends
/// on observed terminal state, never an arbitrary delay.
async fn submit_terminal_message(
    client: &HostClient,
    session_id: WorkerSessionId,
    mut bytes: Vec<u8>,
    marker: &[u8],
) -> Result<TerminalSubmission, swarm_terminal::IpcError> {
    let submit = bytes.last() == Some(&b'\r');
    if submit {
        bytes.pop();
    }
    let baseline = match client
        .request(&HostRequest::Read {
            session_id,
            after_sequence: None,
        })
        .await?
    {
        HostResponse::Output { resume, .. } => resume_sequence(&resume),
        HostResponse::Error { code, message } => {
            return Ok(TerminalSubmission::Rejected { code, message });
        }
        _ => return Ok(TerminalSubmission::Uncertain),
    };
    let response = client
        .request(&HostRequest::Write { session_id, bytes })
        .await?;
    match response {
        HostResponse::Acknowledged if submit => {}
        HostResponse::Acknowledged => return Ok(TerminalSubmission::Acknowledged),
        HostResponse::Error { code, message } => {
            return Ok(TerminalSubmission::Rejected { code, message });
        }
        _ => return Ok(TerminalSubmission::Uncertain),
    }
    let mut after_sequence = baseline;
    let mut rendered = false;
    for _ in 0..64 {
        let observed_sequence = match client
            .request(&HostRequest::Wait {
                session_id,
                after_sequence: Some(after_sequence),
            })
            .await?
        {
            HostResponse::Output { resume, .. } => resume_sequence(&resume),
            _ => return Ok(TerminalSubmission::Uncertain),
        };
        if observed_sequence <= after_sequence {
            return Ok(TerminalSubmission::Uncertain);
        }
        after_sequence = observed_sequence;
        rendered = matches!(
            client
                .request(&HostRequest::Read {
                    session_id,
                    after_sequence: None,
                })
                .await?,
            HostResponse::Output {
                resume: swarm_terminal::Resume::Snapshot { snapshot },
                ..
            } if snapshot.bytes.windows(marker.len()).any(|part| part == marker)
        );
        if rendered {
            break;
        }
    }
    if !rendered {
        return Ok(TerminalSubmission::Uncertain);
    }
    match client
        .request(&HostRequest::Write {
            session_id,
            bytes: vec![b'\r'],
        })
        .await
    {
        Ok(HostResponse::Acknowledged) => Ok(TerminalSubmission::Acknowledged),
        Ok(_) | Err(_) => Ok(TerminalSubmission::Uncertain),
    }
}

fn resume_sequence(resume: &swarm_terminal::Resume) -> u64 {
    match resume {
        swarm_terminal::Resume::Snapshot { snapshot } => snapshot.sequence,
        swarm_terminal::Resume::Deltas { frames } => {
            frames.last().map_or(0, |frame| frame.sequence)
        }
    }
}

fn delivery_marker(id: impl std::fmt::Display) -> Vec<u8> {
    id.to_string().bytes().take(8).collect()
}

fn decision_delivery_message(delivery: &DecisionDispatch) -> Vec<u8> {
    let action = terminal_safe_text(&delivery.action);
    let note = if delivery.note.is_empty() {
        "No additional note.".into()
    } else {
        terminal_safe_text(&delivery.note)
    };
    format!(
        "[Swarm decision {} resolved] Action: {}. Operator note: {} Use swarm_list_decisions for the full request context.\r",
        delivery.decision_id, action, note,
    )
    .into_bytes()
}

fn task_dispatch_message(delivery: &TaskDispatch) -> Vec<u8> {
    let title = terminal_safe_text(&delivery.title);
    let description = if delivery.description.is_empty() {
        "No additional brief.".into()
    } else {
        terminal_safe_text(&delivery.description)
    };
    let workspace = terminal_safe_text(&delivery.workspace);
    format!(
        "[Swarm task {} assigned] {}. Priority: {}. Workspace: {}. Brief: {} Use swarm_list_tasks for the authoritative current assignment; if it is not visible, the assignment changed.\r",
        delivery.task_id, title, delivery.priority, workspace, description,
    )
    .into_bytes()
}
fn task_outcome_message(outcome: &TaskOutcomeDispatch) -> Vec<u8> {
    let reporter = terminal_safe_text(&outcome.reporting_worker_name);
    let title = terminal_safe_text(&outcome.title);
    let note = if outcome.note.is_empty() {
        "No additional handoff note.".into()
    } else {
        terminal_safe_text(&outcome.note)
    };
    format!(
        "[Swarm worker outcome] {} moved task {} \"{}\" to {}. Handoff: {} Use swarm_list_tasks and task history for authoritative context.\r",
        reporter, outcome.task_id, title, outcome.target_state, note,
    )
    .into_bytes()
}
fn terminal_safe_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}
pub(crate) async fn deliver_jira_transition_batch(
    store: &TaskStore,
    jira: &jira::JiraReadinessProbe,
    control_room_notify: &Notify,
) {
    let deliveries = match store.claim_jira_transitions(unix_timestamp()) {
        Ok(deliveries) => deliveries,
        Err(error) => {
            tracing::warn!(%error, "Jira transition queue could not be claimed");
            return;
        }
    };
    for delivery in deliveries {
        let outcome = match jira
            .transition_issue(&delivery.issue_key, &delivery.target_status_ids)
            .await
        {
            Ok(transitioned) => store.complete_jira_transition(
                &delivery.id,
                &transitioned.status_id,
                &transitioned.status_name,
                unix_timestamp(),
            ),
            Err(error) => {
                let failure = if error == jira::JiraAdapterError::NetworkUnavailable {
                    JiraTransitionFailure::Retryable
                } else {
                    JiraTransitionFailure::Conflict
                };
                tracing::warn!(
                    task_id = %delivery.task_id,
                    issue = %delivery.issue_key,
                    %error,
                    "durable Jira transition was not acknowledged"
                );
                store.fail_jira_transition(
                    &delivery.id,
                    unix_timestamp(),
                    failure,
                    jira_adapter_error_code(error),
                )
            }
        };
        match outcome {
            Ok(true) => control_room_notify.notify_waiters(),
            Ok(false) => tracing::warn!(
                task_id = %delivery.task_id,
                issue = %delivery.issue_key,
                "Jira transition claim was no longer active"
            ),
            Err(error) => tracing::warn!(
                task_id = %delivery.task_id,
                issue = %delivery.issue_key,
                %error,
                "Jira transition outcome could not be persisted"
            ),
        }
    }
}

pub(crate) async fn deliver_jira_comment_batch(
    store: &TaskStore,
    jira: &jira::JiraReadinessProbe,
    control_room_notify: &Notify,
) {
    let deliveries = match store.claim_jira_comments(unix_timestamp()) {
        Ok(deliveries) => deliveries,
        Err(error) => {
            tracing::warn!(%error, "Jira comment queue could not be claimed");
            return;
        }
    };
    for delivery in deliveries {
        let outcome = match jira.add_comment(&delivery.issue_key, &delivery.body).await {
            Ok(()) => store.complete_jira_comment(&delivery.id, unix_timestamp()),
            Err(error) => {
                let retryable = error == jira::JiraAdapterError::NetworkUnavailable;
                tracing::warn!(
                    task_id = %delivery.task_id,
                    issue = %delivery.issue_key,
                    %error,
                    "durable Jira comment was not acknowledged"
                );
                store.fail_jira_comment(
                    &delivery.id,
                    unix_timestamp(),
                    retryable,
                    jira_adapter_error_code(error),
                )
            }
        };
        match outcome {
            Ok(true) => control_room_notify.notify_waiters(),
            Ok(false) => tracing::warn!(
                task_id = %delivery.task_id,
                issue = %delivery.issue_key,
                "Jira comment claim was no longer active"
            ),
            Err(error) => tracing::warn!(
                task_id = %delivery.task_id,
                issue = %delivery.issue_key,
                %error,
                "Jira comment outcome could not be persisted"
            ),
        }
    }
}

fn jira_adapter_error_code(error: jira::JiraAdapterError) -> &'static str {
    match error {
        jira::JiraAdapterError::NotConfigured => "not_configured",
        jira::JiraAdapterError::CredentialsInvalid => "credentials_invalid",
        jira::JiraAdapterError::PermissionDenied => "permission_denied",
        jira::JiraAdapterError::NetworkUnavailable => "network_unavailable",
        jira::JiraAdapterError::InvalidResponse => "invalid_response",
        jira::JiraAdapterError::ResponseLimitExceeded => "response_limit_exceeded",
        jira::JiraAdapterError::TransitionUnavailable => "transition_unavailable",
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(JournalLimits::default())
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeLimitsResponse {
    terminal: TerminalRuntimeLimits,
}

#[derive(Debug, Serialize)]
struct TerminalRuntimeLimits {
    journal_max_bytes: usize,
    journal_max_frames: usize,
    attach_grant_max_active: usize,
    websocket_max_active: usize,
    canonical_scrollback_rows: usize,
    canonical_compaction_input_bytes: usize,
    canonical_snapshot_max_bytes: usize,
    max_rows: u16,
    max_columns: u16,
    max_cells: usize,
}

#[derive(Debug, Serialize)]
struct WorkerEngineMaintenanceResponse {
    previous_version: String,
    current_version: String,
    stopped_sessions: usize,
    restarted_workers: usize,
}

#[derive(Debug, Serialize)]
struct RuntimeResourcesResponse {
    sampled_at: i64,
    policy: ResourcePolicyResponse,
    api: ProcessResourceResponse,
    terminal_host: ProcessResourceResponse,
    machine: MachineResourceResponse,
}

#[derive(Debug, Serialize)]
struct MachineResourceResponse {
    memory_total_bytes: Option<u64>,
    memory_available_bytes: Option<u64>,
    memory_used_percent: Option<f64>,
    swap_total_bytes: Option<u64>,
    swap_used_bytes: Option<u64>,
    swap_used_percent: Option<f64>,
    load_average: Option<[f64; 3]>,
    logical_cpus: Option<usize>,
    memory_pressure_avg10: Option<f64>,
    cpu_pressure_avg10: Option<f64>,
    io_pressure_avg10: Option<f64>,
    pressure: ResourcePressure,
}

#[derive(Debug, Serialize)]
struct ResourcePolicyResponse {
    mode: &'static str,
    advisory_bytes: u64,
    critical_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResourcePressure {
    Normal,
    Advisory,
    Critical,
    Unavailable,
}

#[derive(Debug, Serialize)]
struct ProcessResourceResponse {
    resident_memory_bytes: Option<u64>,
    process_tree_resident_memory_bytes: Option<u64>,
    process_tree_process_count: Option<u32>,
    pressure: ResourcePressure,
}

#[derive(Debug, Serialize)]
struct AttachGrantResponse {
    grant: String,
    protocol: &'static str,
    websocket_path: String,
    expires_in_ms: u64,
}

#[derive(Debug, Deserialize)]
struct SetPresenceRequest {
    manual_mode: Option<PresenceMode>,
}

#[derive(Debug, Deserialize)]
struct PresenceObservationRequest {
    device_class: PresenceDeviceClass,
    state: PresenceObservationState,
}
#[derive(Debug, Deserialize)]
struct SetNotificationPolicyRequest {
    policy: NotificationPolicy,
}

#[derive(Debug, Deserialize)]
struct SetQueenAutonomyPolicyRequest {
    at_hive: QueenAutonomyLevel,
    away: QueenAutonomyLevel,
    night_watch: QueenAutonomyLevel,
}

#[derive(Debug, Deserialize)]
struct SetPresentationPreferencesRequest {
    color_theme: PresentationColorTheme,
    terminal_keys_visible: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ProviderCapabilitiesView {
    claude_code: bool,
    codex: bool,
}

#[derive(Debug, Deserialize)]
struct SaveNotificationSubscriptionRequest {
    device_class: PresenceDeviceClass,
    endpoint: String,
    keys: NotificationSubscriptionKeys,
}

#[derive(Debug, Deserialize)]
struct NotificationSubscriptionKeys {
    p256dh: String,
    auth: String,
}
#[derive(Debug, Deserialize)]
struct StartSessionRequest {
    workspace: PathBuf,
    rows: u16,
    columns: u16,
}

#[derive(Debug, Deserialize)]
struct CreateWorkerRequest {
    name: String,
    #[serde(default = "default_provider")]
    provider: ProviderKind,
    workspace: String,
    #[serde(default)]
    autostart: bool,
    #[serde(default)]
    allow_outside_roots: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateWorkerRequest {
    name: Option<String>,
    autostart: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StartWorkerRequest {
    #[serde(default = "default_terminal_rows")]
    rows: u16,
    #[serde(default = "default_terminal_columns")]
    columns: u16,
}

#[derive(Debug, Serialize)]
struct WorkerView {
    #[serde(flatten)]
    profile: WorkerProfile,
    running: bool,
    attention_state: WorkerAttentionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    engagement_expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct WorkspaceView {
    name: String,
    path: String,
    kind: &'static str,
    configured_worker_id: Option<WorkerId>,
}

#[derive(Debug, Serialize)]
struct LocalHiveView {
    #[serde(flatten)]
    identity: HiveIdentity,
    apiary_context: LocalApiaryContext,
}

#[derive(Debug, Serialize)]
struct ApiaryInvitationView {
    invitation: ApiaryInvitation,
    apiary: Apiary,
    readiness: ApiaryJoinReadiness,
    jira_connection: JiraConnectionState,
}

impl From<ApiaryInvitationOverview> for ApiaryInvitationView {
    fn from(value: ApiaryInvitationOverview) -> Self {
        Self {
            invitation: value.invitation,
            apiary: value.apiary,
            readiness: value.readiness,
            jira_connection: value.jira_connection,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateApiaryRequest {
    name: String,
    shared_work_backend: SharedWorkBackend,
}

#[derive(Debug, Deserialize)]
struct AcceptApiaryPolicyRequest {
    policy_revision: u64,
}

fn worker_view(
    profile: WorkerProfile,
    running: bool,
    awaiting_operator: bool,
    runtime_error: Option<String>,
    provider_activity: ProviderActivity,
) -> WorkerView {
    let engagement_expires_at = profile.engagement_expires_at;
    let attention_state = if runtime_error.is_some() {
        WorkerAttentionState::Blocked
    } else if !running {
        WorkerAttentionState::Sleeping
    } else if engagement_expires_at.is_some() {
        WorkerAttentionState::WithOperator
    } else if awaiting_operator || provider_activity == ProviderActivity::AwaitingOperator {
        WorkerAttentionState::AwaitingOperator
    } else if provider_activity == ProviderActivity::Resting {
        WorkerAttentionState::Resting
    } else {
        WorkerAttentionState::Buzzing
    };
    WorkerView {
        profile,
        running,
        attention_state,
        engagement_expires_at,
        runtime_error,
    }
}

fn default_provider() -> ProviderKind {
    ProviderKind::ClaudeCode
}

fn default_terminal_rows() -> u16 {
    TerminalSize::default().rows
}

fn default_terminal_columns() -> u16 {
    TerminalSize::default().columns
}

#[derive(Debug, Deserialize)]
struct InputRequest {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ResizeRequest {
    rows: u16,
    columns: u16,
}

#[derive(Debug, Deserialize)]
struct CreateTaskRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
    workspace: String,
}

#[derive(Debug, Deserialize)]
struct TransitionTaskRequest {
    state: TaskState,
    #[serde(default)]
    note: String,
}
#[derive(Debug, Deserialize)]
struct JiraCommentRequest {
    body: String,
}
#[derive(Debug, Deserialize)]
struct ResolveDecisionRequest {
    action: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct AssignTaskRequest {
    worker_id: Option<WorkerId>,
}

#[derive(Debug, Deserialize)]
struct OutputQuery {
    after: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ControlRoomEventsQuery {
    after: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TaskActivityQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ReorderTasksRequest {
    task_ids: Vec<TaskId>,
}

#[derive(Debug, Deserialize)]
struct ReorderWorkersRequest {
    worker_ids: Vec<WorkerId>,
}

#[derive(Debug, Deserialize)]
struct JiraProjectsQuery {
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateJiraProjectBindingRequest {
    #[serde(rename = "project_id")]
    id: String,
    #[serde(rename = "project_key")]
    key: String,
    #[serde(rename = "project_name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct JiraAssignedSyncRequest {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ReplaceJiraMappingsRequest {
    mappings: Vec<JiraStatusMapping>,
}

#[derive(Debug, Deserialize)]
struct SyncJiraBindingRequest {
    issue_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct JiraTaskLinkView {
    issue_id: String,
    issue_key: String,
    issue_url: Option<String>,
    binding_id: JiraProjectBindingId,
    project_key: String,
    project_name: String,
    task_id: TaskId,
    jira_status_id: String,
    jira_status_name: String,
    jira_assignee_account_id: Option<String>,
    jira_assignee_name: Option<String>,
    remote_updated_at: String,
    last_synced_at: i64,
    outbound_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    segment: Option<u64>,
    record: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DogfoodReportsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CreateDogfoodReportRequest {
    expectation: String,
    observation: String,
    diagnostic_bundle: String,
    attachment_name: Option<String>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: &self.message,
            }),
        )
            .into_response()
    }
}

pub fn router(state: AppState) -> Router {
    api_router(state)
}

/// Builds the API and serves a compiled browser application from `web_root`.
/// Unknown files remain 404s; API misses are never rewritten to HTML.
pub fn router_with_web_root(state: AppState, web_root: impl AsRef<FilePath>) -> Router {
    router_with_optional_asset_root(state, web_root.as_ref().to_path_buf(), None)
}

/// Builds the API with a stable hashed-asset library retained across releases.
pub fn router_with_asset_root(
    state: AppState,
    web_root: impl AsRef<FilePath>,
    asset_root: impl AsRef<FilePath>,
) -> Router {
    router_with_optional_asset_root(
        state,
        web_root.as_ref().to_path_buf(),
        Some(asset_root.as_ref().to_path_buf()),
    )
}

fn router_with_optional_asset_root(
    state: AppState,
    web_root: PathBuf,
    asset_root: Option<PathBuf>,
) -> Router {
    let app = match asset_root {
        Some(asset_root) => api_router(state).nest_service(
            "/assets",
            ServeDir::new(asset_root).fallback(ServeDir::new(web_root.join("assets"))),
        ),
        None => api_router(state),
    };
    app.fallback_service(ServeDir::new(web_root).append_index_html_on_directories(true))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; manifest-src 'self'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "camera=(), geolocation=(), microphone=(), payment=(), usb=()",
            ),
        ))
}

#[allow(clippy::too_many_lines)]
fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/mcp", post(mcp))
        .route("/health", get(health))
        .route(
            "/api/v1/auth/session",
            get(get_browser_session)
                .post(create_browser_session)
                .delete(delete_browser_session),
        )
        .route("/api/v1/hive", get(local_hive))
        .route("/api/v1/apiary", post(create_apiary))
        .route("/api/v1/apiary/invitations", get(apiary_invitations))
        .route(
            "/api/v1/apiary/invitations/{invitation_id}/policy-acceptance",
            post(accept_apiary_policy),
        )
        .route(
            "/api/v1/apiary/invitations/{invitation_id}/join",
            post(join_apiary),
        )
        .route(
            "/api/v1/presence",
            get(operator_presence).put(set_operator_presence),
        )
        .route(
            "/api/v1/presence/devices/{device_id}",
            put(observe_presence_device),
        )
        .route(
            "/api/v1/notifications/settings",
            get(notification_settings).put(set_notification_policy),
        )
        .route(
            "/api/v1/orchestration/queen-policy",
            get(queen_autonomy_policy).put(set_queen_autonomy_policy),
        )
        .route(
            "/api/v1/preferences/presentation/{device_class}",
            get(presentation_preferences).put(set_presentation_preferences),
        )
        .route(
            "/api/v1/notifications/subscriptions/{device_id}",
            put(save_notification_subscription).delete(remove_notification_subscription),
        )
        .route(
            "/api/v1/notifications/subscriptions/{device_id}/test",
            post(test_notification),
        )
        .route("/api/v1/control-room/events", get(control_room_events))
        .route("/api/v1/runtime/limits", get(runtime_limits))
        .route("/api/v1/runtime/resources", get(runtime_resources))
        .route("/api/v1/runtime/terminal-host", get(terminal_host_status))
        .route(
            "/api/v1/runtime/terminal-host/maintenance",
            post(maintain_terminal_host),
        )
        .route("/api/v1/backups/database", get(download_database_backup))
        .route(
            "/api/v1/feedback/reports",
            get(list_dogfood_reports).post(create_dogfood_report),
        )
        .route(
            "/api/v1/feedback/attachments",
            post(upload_dogfood_attachment).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BYTES)),
        )
        .route(
            "/api/v1/feedback/attachments/{name}",
            get(download_dogfood_attachment),
        )
        .route("/api/v1/tasks", get(list_tasks).post(create_task))
        .route("/api/v1/decisions", get(list_decisions))
        .route(
            "/api/v1/decisions/{decision_id}/resolution",
            patch(resolve_decision),
        )
        .route("/api/v1/tasks/order", put(reorder_tasks))
        .route("/api/v1/tasks/{task_id}", patch(update_task))
        .route("/api/v1/tasks/{task_id}/activity", get(task_activity))
        .route("/api/v1/tasks/{task_id}/state", patch(transition_task))
        .route("/api/v1/tasks/{task_id}/assignment", put(assign_task))
        .route("/api/v1/workers", get(list_workers).post(create_worker))
        .route("/api/v1/providers", get(list_provider_capabilities))
        .route("/api/v1/integrations/jira/readiness", get(jira_readiness))
        .route(
            "/api/v1/integrations/jira/auth/start",
            post(jira_auth_start),
        )
        .route("/api/v1/integrations/jira/auth", delete(jira_disconnect))
        .route("/auth/jira/callback", get(jira_auth_callback))
        .route("/api/v1/integrations/jira/projects", get(jira_projects))
        .route(
            "/api/v1/integrations/jira/projects/{project_id_or_key}/statuses",
            get(jira_project_statuses),
        )
        .route(
            "/api/v1/integrations/jira/bindings",
            get(jira_bindings).post(create_jira_binding),
        )
        .route("/api/v1/integrations/jira/task-links", get(jira_task_links))
        .route(
            "/api/v1/integrations/jira/task-links/{task_id}/comments",
            get(jira_task_comments).post(create_jira_task_comment),
        )
        .route(
            "/api/v1/integrations/jira/task-links/{task_id}/retry",
            post(retry_jira_task_link),
        )
        .route(
            "/api/v1/integrations/jira/bindings/{binding_id}/mappings",
            get(jira_mappings).put(replace_jira_mappings),
        )
        .route(
            "/api/v1/integrations/jira/bindings/{binding_id}/assigned-sync",
            put(set_jira_assigned_sync),
        )
        .route(
            "/api/v1/integrations/jira/bindings/{binding_id}/issues",
            get(jira_binding_issues),
        )
        .route(
            "/api/v1/integrations/jira/bindings/{binding_id}/sync",
            post(sync_jira_binding),
        )
        .route(
            "/api/v1/integrations/jira/reconcile",
            post(reconcile_jira_now),
        )
        .route("/api/v1/workers/order", put(reorder_workers))
        .route("/api/v1/workers/{worker_id}", patch(update_worker))
        .route("/api/v1/workspaces", get(list_workspaces))
        .route("/api/v1/workers/{worker_id}/start", post(start_worker))
        .route("/api/v1/workers/{worker_id}/session", delete(stop_worker))
        .route(
            "/api/v1/terminal/sessions",
            get(list_sessions).post(start_session),
        )
        .route(
            "/api/v1/terminal/history/diagnostics",
            get(history_diagnostics),
        )
        .route(
            "/api/v1/terminal/history/sessions",
            get(list_history_sessions),
        )
        .route(
            "/api/v1/terminal/history/sessions/{session_id}",
            get(read_history),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}",
            delete(stop_session),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/output",
            get(read_output),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/input",
            post(write_input),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/engagements/{device_id}",
            delete(release_terminal_engagement),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attachments",
            post(upload_terminal_attachment).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BYTES)),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/size",
            put(resize_terminal),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attach-grants",
            post(issue_attach_grant),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attach",
            get(attach_terminal),
        )
        .with_state(Arc::new(state))
}

async fn mcp(State(state): State<Arc<AppState>>, request: axum::extract::Request) -> Response {
    let Some(bridge) = state.agent_bridge.clone() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let response = agent::handle(bridge, request).await;
    state.deliver_coordination().await;
    response
}
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: build_version(),
    })
}

fn build_version() -> &'static str {
    option_env!("SWARM_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

async fn get_browser_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

async fn create_browser_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let cookie = browser_session_set_cookie(&state, &headers)?;
    Ok((
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (header::SET_COOKIE, cookie),
        ],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

async fn delete_browser_session() -> Response {
    (
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::SET_COOKIE,
                HeaderValue::from_static(
                    "swarm_next_operator_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
                ),
            ),
        ],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

async fn local_hive(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let identity = task_store(&state)?
        .local_hive_identity()
        .map_err(|error| task_store_error(&error))?;
    let apiary_context = task_store(&state)?
        .local_apiary_context()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(LocalHiveView {
            identity,
            apiary_context,
        }),
    )
        .into_response())
}

async fn apiary_invitations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let views = apiary_service(&state)?
        .pending_invitations(jira.connection, unix_timestamp())
        .map_err(application_error)?
        .into_iter()
        .map(ApiaryInvitationView::from)
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(views)).into_response())
}

async fn create_apiary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateApiaryRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let context = apiary_service(&state)?
        .create_from_personal_hive(&request.name, request.shared_work_backend, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(context),
    )
        .into_response())
}

async fn accept_apiary_policy(
    State(state): State<Arc<AppState>>,
    Path(invitation_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<AcceptApiaryPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let overview = apiary_service(&state)?
        .accept_policy(
            parse_apiary_invitation_id(&invitation_id)?,
            request.policy_revision,
            jira.connection,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ApiaryInvitationView::from(overview)),
    )
        .into_response())
}

async fn join_apiary(
    State(state): State<Arc<AppState>>,
    Path(invitation_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let context = apiary_service(&state)?
        .join(
            parse_apiary_invitation_id(&invitation_id)?,
            jira.connection,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(context)).into_response())
}

async fn download_database_backup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let backup = tempfile::NamedTempFile::new().map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("a temporary backup could not be created: {error}"),
        )
    })?;
    task_store(&state)?
        .backup_to(backup.path())
        .map_err(|error| task_store_error(&error))?;
    let bytes = std::fs::read(backup.path()).map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("the completed backup could not be read: {error}"),
        )
    })?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-hive.sqlite3"),
    );
    Ok((response_headers, bytes).into_response())
}

async fn list_dogfood_reports(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DogfoodReportsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reports = task_store(&state)?
        .list_dogfood_reports(query.limit.unwrap_or(20))
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(reports)).into_response())
}

async fn create_dogfood_report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDogfoodReportRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let report = task_store(&state)?
        .create_dogfood_report(
            &request.expectation,
            &request.observation,
            &request.diagnostic_bundle,
            request.attachment_name.as_deref(),
        )
        .map_err(|error| task_store_error(&error))?;
    Ok((StatusCode::CREATED, Json(report)).into_response())
}

#[derive(Serialize)]
struct DogfoodAttachmentResponse {
    name: String,
}

async fn upload_dogfood_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let path = store
        .save(media_type, &body)
        .await
        .map_err(attachment_error)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attachment_path_unavailable",
                "private attachment name is not valid UTF-8",
            )
        })?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(DogfoodAttachmentResponse { name: name.into() }),
    )
        .into_response())
}

async fn download_dogfood_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let referenced = task_store(&state)?
        .dogfood_attachment_is_referenced(&name)
        .map_err(|error| task_store_error(&error))?;
    if !referenced {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "feedback_attachment_not_found",
            "the private report attachment was not found",
        ));
    }
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let (bytes, media_type) = store.read(&name).await.map_err(attachment_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type)
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename={name}"))
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    Ok((response_headers, bytes).into_response())
}

async fn operator_presence(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let presence = task_service(&state)?
        .operator_presence(unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}

async fn set_operator_presence(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetPresenceRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let (presence, changed) = task_service(&state)?
        .set_operator_presence(request.manual_mode, unix_timestamp())
        .map_err(application_error)?;
    if changed {
        state.control_room_notify.notify_waiters();
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}

async fn observe_presence_device(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    Json(request): Json<PresenceObservationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "presence device ID must be a UUID",
        )
    })?;
    let (presence, changed) = task_service(&state)?
        .observe_operator_device(
            device_id,
            request.device_class,
            request.state,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    if changed {
        state.control_room_notify.notify_waiters();
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}
async fn notification_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let sender = state.notification_sender.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notifications_unavailable",
            "notification transport is unavailable",
        )
    })?;
    let settings = task_store(&state)?
        .notification_settings()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(notifications::NotificationSettingsResponse {
            settings,
            vapid_public_key: sender.public_key(),
        }),
    )
        .into_response())
}

async fn queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = task_store(&state)?
        .queen_autonomy_policy()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

async fn set_queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetQueenAutonomyPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = QueenAutonomyPolicy {
        at_hive: request.at_hive,
        away: request.away,
        night_watch: request.night_watch,
    };
    let policy = task_store(&state)?
        .set_queen_autonomy_policy(policy, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

async fn presentation_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_class): Path<PresentationDeviceClass>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preferences = task_store(&state)?
        .presentation_preferences(device_class)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(preferences)).into_response())
}

async fn set_presentation_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_class): Path<PresentationDeviceClass>,
    Json(request): Json<SetPresentationPreferencesRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preferences = task_store(&state)?
        .set_presentation_preferences(
            PresentationPreferences {
                device_class,
                color_theme: request.color_theme,
                terminal_keys_visible: request.terminal_keys_visible,
                configured: true,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(preferences)).into_response())
}

async fn set_notification_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetNotificationPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let sender = state.notification_sender.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notifications_unavailable",
            "notification transport is unavailable",
        )
    })?;
    let settings = task_store(&state)?
        .set_notification_policy(request.policy, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    schedule_notification_delivery(&state);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(notifications::NotificationSettingsResponse {
            settings,
            vapid_public_key: sender.public_key(),
        }),
    )
        .into_response())
}

async fn save_notification_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    Json(request): Json<SaveNotificationSubscriptionRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    notifications::validate_push_endpoint(&request.endpoint).map_err(|message| {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_endpoint", message)
    })?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "notification device ID must be a UUID",
        )
    })?;
    let p256dh = notifications::decode_subscription_key(&request.keys.p256dh, 65)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    notifications::validate_subscription_public_key(&p256dh)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    let auth = notifications::decode_subscription_key(&request.keys.auth, 16)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    let settings = task_store(&state)?
        .save_notification_subscription(
            &PushSubscriptionInput {
                device_id,
                device_class: request.device_class,
                endpoint: request.endpoint,
                p256dh,
                auth,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    notification_settings_response(&state, settings)
}

async fn remove_notification_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "notification device ID must be a UUID",
        )
    })?;
    let settings = task_store(&state)?
        .remove_notification_subscription(device_id)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    notification_settings_response(&state, settings)
}

async fn test_notification(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "notification device ID must be a UUID",
        )
    })?;
    let queued = task_store(&state)?
        .enqueue_device_test_notification(device_id, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    if !queued {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "notification_device_not_found",
            "this browser is not registered for notifications",
        ));
    }
    schedule_notification_delivery(&state);
    let settings = task_store(&state)?
        .notification_settings()
        .map_err(|error| task_store_error(&error))?;
    notification_settings_response(&state, settings)
}

fn schedule_notification_delivery(state: &Arc<AppState>) {
    let sender = state.notification_sender.clone();
    tokio::spawn(async move {
        if let Some(sender) = sender {
            sender.deliver().await;
        }
    });
}
fn notification_settings_response(
    state: &AppState,
    settings: NotificationSettings,
) -> Result<Response, ApiError> {
    let sender = state.notification_sender.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notifications_unavailable",
            "notification transport is unavailable",
        )
    })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(notifications::NotificationSettingsResponse {
            settings,
            vapid_public_key: sender.public_key(),
        }),
    )
        .into_response())
}
async fn control_room_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ControlRoomEventsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let after = query.after.unwrap_or(0).max(0);
    let notified = state.control_room_notify.notified();
    let mut page = task_store(&state)?
        .list_control_room_events(after)
        .map_err(|error| task_store_error(&error))?;
    if page.events.is_empty() && !page.reset_required {
        let _ = timeout(Duration::from_secs(20), notified).await;
        page = task_store(&state)?
            .list_control_room_events(after)
            .map_err(|error| task_store_error(&error))?;
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(page)).into_response())
}

async fn runtime_limits(State(state): State<Arc<AppState>>) -> Json<RuntimeLimitsResponse> {
    Json(RuntimeLimitsResponse {
        terminal: TerminalRuntimeLimits {
            journal_max_bytes: state.terminal_limits.max_bytes,
            journal_max_frames: state.terminal_limits.max_frames,
            attach_grant_max_active: MAX_ATTACH_GRANTS,
            websocket_max_active: MAX_TERMINAL_WEBSOCKETS,
            canonical_scrollback_rows: CANONICAL_SCROLLBACK_ROWS,
            canonical_compaction_input_bytes: CANONICAL_COMPACTION_INPUT_BYTES,
            canonical_snapshot_max_bytes: MAX_CANONICAL_SNAPSHOT_BYTES,
            max_rows: MAX_TERMINAL_ROWS,
            max_columns: MAX_TERMINAL_COLUMNS,
            max_cells: MAX_TERMINAL_CELLS,
        },
    })
}

async fn terminal_host_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::HostStatus).await
}

async fn maintain_terminal_host(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let guard = state.worker_lifecycle.lock().await;
    let result = maintain_terminal_host_locked(&state).await;
    if let Ok(maintenance) = &result
        && maintenance.previous_version != maintenance.current_version
    {
        if let Err(error) = task_store(&state).and_then(|store| {
            store
                .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
                .map(|_| ())
                .map_err(|error| task_store_error(&error))
        }) {
            tracing::warn!(message = %error.message, "worker-engine update could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    drop(guard);

    // This runs on both success and failure. A failed package trigger therefore
    // revives autostart workers on the still-current host instead of leaving a
    // partially stopped Hive behind.
    state.supervise_workers().await;
    let mut response = result?;
    response.restarted_workers = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .filter(|worker| worker.active_session_id.is_some())
        .count();
    Ok(Json(response).into_response())
}

async fn maintain_terminal_host_locked(
    state: &AppState,
) -> Result<WorkerEngineMaintenanceResponse, ApiError> {
    let request_path = state.maintenance_request_path.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_engine_maintenance_unavailable",
            "this installation does not expose managed worker-engine maintenance",
        )
    })?;
    let previous = host_status_snapshot(state).await?;
    if previous.host_version == build_version() {
        return Ok(WorkerEngineMaintenanceResponse {
            previous_version: previous.host_version.clone(),
            current_version: previous.host_version,
            stopped_sessions: 0,
            restarted_workers: 0,
        });
    }
    let HostResponse::Sessions { sessions } =
        request_host(state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected session response",
        ));
    };
    let running = sessions
        .into_iter()
        .filter(|session| session.running)
        .collect::<Vec<_>>();
    for session in &running {
        request_host(
            state,
            HostRequest::Stop {
                session_id: session.session_id,
            },
        )
        .await?;
        task_store(state)?
            .release_worker_session(session.session_id)
            .map_err(|error| task_store_error(&error))?;
        task_store(state)?
            .release_session_assignments(session.session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    state.control_room_notify.notify_waiters();
    std::fs::write(
        request_path.as_ref(),
        format!(
            "requested_at={}\ntarget_version={}\n",
            unix_timestamp(),
            build_version()
        ),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_engine_maintenance_unavailable",
            format!("the managed maintenance request could not be recorded: {error}"),
        )
    })?;

    let updated = timeout(state.maintenance_timeout, async {
        loop {
            sleep(Duration::from_millis(200)).await;
            if let Ok(status) = host_status_snapshot(state).await
                && status.host_version == build_version()
                && !status.draining
            {
                return status;
            }
        }
    })
    .await;
    let _ = std::fs::remove_file(request_path.as_ref());
    let current = updated.map_err(|_| {
        ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "worker_engine_maintenance_timed_out",
            "the worker engine did not report the expected release; configured workers were revived on the available host",
        )
    })?;
    Ok(WorkerEngineMaintenanceResponse {
        previous_version: previous.host_version,
        current_version: current.host_version,
        stopped_sessions: running.len(),
        restarted_workers: 0,
    })
}

async fn host_status_snapshot(
    state: &AppState,
) -> Result<swarm_terminal::TerminalHostStatus, ApiError> {
    let HostResponse::HostStatus { status } = request_host(state, HostRequest::HostStatus).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected status response",
        ));
    };
    Ok(status)
}

async fn runtime_resources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let terminal_host = if let Some(client) = &state.terminal_host {
        match client.request(&HostRequest::HostStatus).await {
            Ok(HostResponse::HostStatus { status }) => resource_response(status.resources),
            Ok(_) | Err(_) => resource_response(None),
        }
    } else {
        resource_response(None)
    };
    let response = RuntimeResourcesResponse {
        sampled_at: unix_timestamp(),
        policy: ResourcePolicyResponse {
            mode: "observe_only",
            advisory_bytes: RESOURCE_ADVISORY_BYTES,
            critical_bytes: RESOURCE_CRITICAL_BYTES,
        },
        api: resource_response(Some(sample_current_process())),
        terminal_host,
        machine: sample_machine_resources(),
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

fn sample_machine_resources() -> MachineResourceResponse {
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
        let fields = meminfo.as_deref().map(parse_meminfo).unwrap_or_default();
        let bytes = |name: &str| fields.get(name).and_then(|value| value.checked_mul(1024));
        let memory_total_bytes = bytes("MemTotal");
        let memory_available_bytes = bytes("MemAvailable");
        let swap_total_bytes = bytes("SwapTotal");
        let swap_free_bytes = bytes("SwapFree");
        let swap_used_bytes = swap_total_bytes
            .zip(swap_free_bytes)
            .map(|(total, free)| total.saturating_sub(free));
        let percent = |used: u64, total: u64| {
            (total > 0).then(|| {
                let basis_points = used.saturating_mul(10_000) / total;
                f64::from(u32::try_from(basis_points).unwrap_or(u32::MAX)) / 100.0
            })
        };
        let memory_used_percent = memory_total_bytes
            .zip(memory_available_bytes)
            .and_then(|(total, available)| percent(total.saturating_sub(available), total));
        let swap_used_percent = swap_used_bytes
            .zip(swap_total_bytes)
            .and_then(|(used, total)| percent(used, total));
        let load_average = std::fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|value| parse_load_average(&value));
        let memory_pressure_avg10 = read_psi_avg10("/proc/pressure/memory");
        let cpu_pressure_avg10 = read_psi_avg10("/proc/pressure/cpu");
        let io_pressure_avg10 = read_psi_avg10("/proc/pressure/io");
        let pressure = match (memory_used_percent, memory_pressure_avg10) {
            (_, Some(psi)) if psi >= 10.0 => ResourcePressure::Critical,
            (Some(used), _) if used >= 95.0 => ResourcePressure::Critical,
            (_, Some(psi)) if psi >= 2.0 => ResourcePressure::Advisory,
            (Some(used), _) if used >= 85.0 => ResourcePressure::Advisory,
            (Some(_), _) => ResourcePressure::Normal,
            _ => ResourcePressure::Unavailable,
        };
        MachineResourceResponse {
            memory_total_bytes,
            memory_available_bytes,
            memory_used_percent,
            swap_total_bytes,
            swap_used_bytes,
            swap_used_percent,
            load_average,
            logical_cpus: std::thread::available_parallelism().ok().map(usize::from),
            memory_pressure_avg10,
            cpu_pressure_avg10,
            io_pressure_avg10,
            pressure,
        }
    }
    #[cfg(not(target_os = "linux"))]
    MachineResourceResponse {
        memory_total_bytes: None,
        memory_available_bytes: None,
        memory_used_percent: None,
        swap_total_bytes: None,
        swap_used_bytes: None,
        swap_used_percent: None,
        load_average: None,
        logical_cpus: std::thread::available_parallelism().ok().map(usize::from),
        memory_pressure_avg10: None,
        cpu_pressure_avg10: None,
        io_pressure_avg10: None,
        pressure: ResourcePressure::Unavailable,
    }
}

#[cfg(target_os = "linux")]
fn parse_meminfo(value: &str) -> std::collections::HashMap<&str, u64> {
    value
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let kib = rest.split_whitespace().next()?.parse().ok()?;
            Some((name, kib))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_load_average(value: &str) -> Option<[f64; 3]> {
    let mut values = value.split_whitespace().take(3).map(str::parse::<f64>);
    Some([
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
    ])
}

#[cfg(target_os = "linux")]
fn read_psi_avg10(path: &str) -> Option<f64> {
    let value = std::fs::read_to_string(path).ok()?;
    value.lines().find_map(|line| {
        let rest = line.strip_prefix("some ")?;
        rest.split_whitespace()
            .find_map(|field| field.strip_prefix("avg10=")?.parse().ok())
    })
}

fn resource_response(sample: Option<ProcessResourceSample>) -> ProcessResourceResponse {
    let resident_memory_bytes = sample.and_then(|sample| sample.resident_memory_bytes);
    let process_tree_resident_memory_bytes =
        sample.and_then(|sample| sample.process_tree_resident_memory_bytes);
    let process_tree_process_count = sample.and_then(|sample| sample.process_tree_process_count);
    let pressure = match process_tree_resident_memory_bytes.or(resident_memory_bytes) {
        Some(bytes) if bytes >= RESOURCE_CRITICAL_BYTES => ResourcePressure::Critical,
        Some(bytes) if bytes >= RESOURCE_ADVISORY_BYTES => ResourcePressure::Advisory,
        Some(_) => ResourcePressure::Normal,
        None => ResourcePressure::Unavailable,
    };
    ProcessResourceResponse {
        resident_memory_bytes,
        process_tree_resident_memory_bytes,
        process_tree_process_count,
        pressure,
    }
}

async fn list_decisions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decisions = task_service(&state)?
        .list_visible_decisions(None)
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(decisions)).into_response())
}

async fn resolve_decision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
    Json(request): Json<ResolveDecisionRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    task_service(&state)?
        .resolve_operator_decision(decision_id, &request.action, &request.note)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let decision = task_store(&state)?
        .get_decision_request(decision_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(decision).into_response())
}
async fn list_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks = task_service(&state)?
        .list_tasks()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_service(&state)?
        .create_operator_task(
            &request.title,
            &request.description,
            request.priority,
            &request.workspace,
        )
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(task)).into_response())
}

async fn task_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(query): Query<TaskActivityQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let limit = query.limit.unwrap_or(30);
    if !(1..=MAX_TASK_ACTIVITY_PAGE).contains(&limit) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_activity_limit",
            format!("task activity limit must be between 1 and {MAX_TASK_ACTIVITY_PAGE}"),
        ));
    }
    let activity = task_store(&state)?
        .list_task_activity(parse_task_id(&task_id)?, limit)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(activity)).into_response())
}

async fn reorder_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReorderTasksRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    if request.task_ids.len() > MAX_OPEN_TASKS_PER_ORDER {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_order",
            format!("task order cannot exceed {MAX_OPEN_TASKS_PER_ORDER} entries"),
        ));
    }
    let tasks = task_store(&state)?
        .reorder_open_tasks(&request.task_ids)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(tasks).into_response())
}

async fn update_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TaskDetailsUpdate>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_service(&state)?
        .update_operator_task(parse_task_id(&task_id)?, &request)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(Json(task).into_response())
}

async fn transition_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TransitionTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let current = store
        .get_task(task_id)
        .map_err(|error| task_store_error(&error))?;
    if !current.state.can_transition_to(request.state) {
        return Err(task_store_error(&TaskStoreError::InvalidTransition {
            from: current.state,
            to: request.state,
        }));
    }
    if request.note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
        return Err(task_store_error(&TaskStoreError::InvalidTaskActivityNote));
    }
    let task = task_service(&state)?
        .transition_operator_task_with_note(task_id, request.state, &request.note)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_jira_transitions().await;
    Ok(Json(task).into_response())
}

async fn assign_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<AssignTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let task = match request.worker_id {
        Some(worker_id) => task_service(&state)?.assign_operator_task(task_id, worker_id),
        None => task_service(&state)?.unassign_operator_task(task_id),
    }
    .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let task = task_store(&state)?
        .get_task(task.id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(task).into_response())
}

async fn list_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let live = reconcile_worker_bindings(&state).await?;
    let profiles = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    let provider_activity = refresh_provider_activity(&state, &profiles, &live).await;
    let awaiting_operator = task_store(&state)?
        .workers_awaiting_operator()
        .map_err(|error| task_store_error(&error))?;
    let errors = state.worker_errors.read().await;
    let workers = profiles
        .into_iter()
        .map(|profile| {
            let running = profile
                .active_session_id
                .is_some_and(|session_id| live.contains(&session_id));
            let runtime_error = errors.get(&profile.id).cloned();
            let needs_operator = awaiting_operator.contains(&profile.id);
            let activity = profile
                .active_session_id
                .and_then(|session_id| provider_activity.get(&session_id).copied())
                .unwrap_or(ProviderActivity::Unknown);
            worker_view(profile, running, needs_operator, runtime_error, activity)
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(workers)).into_response())
}

async fn observe_provider_activity(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderActivity> {
    let observations = profiles
        .iter()
        .filter_map(|profile| {
            let session_id = profile.active_session_id?;
            live.contains(&session_id)
                .then_some((session_id, profile.provider))
        })
        .collect::<Vec<_>>();
    stream::iter(observations)
        .map(|(session_id, provider)| async move {
            match request_host(
                state,
                HostRequest::Read {
                    session_id,
                    after_sequence: None,
                },
            )
            .await
            {
                Ok(HostResponse::Output {
                    resume: swarm_terminal::Resume::Snapshot { snapshot },
                    running: true,
                    ..
                }) => Some((session_id, classify_provider_activity(provider, &snapshot))),
                _ => None,
            }
        })
        .buffer_unordered(8)
        .filter_map(async move |observation| observation)
        .collect()
        .await
}

async fn refresh_provider_activity(
    state: &AppState,
    profiles: &[WorkerProfile],
    live: &HashSet<WorkerSessionId>,
) -> HashMap<WorkerSessionId, ProviderActivity> {
    let observed = observe_provider_activity(state, profiles, live).await;
    let changed = {
        let mut previous = state.provider_activity.write().await;
        if *previous == observed {
            false
        } else {
            previous.clone_from(&observed);
            true
        }
    };
    if changed {
        if let Some(store) = &state.task_store
            && let Err(error) =
                store.record_control_room_event(ControlRoomEventKind::RuntimeChanged)
        {
            tracing::warn!(%error, "provider activity change could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    observed
}

async fn list_provider_capabilities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let capabilities = match request_host(&state, HostRequest::ProviderCapabilities).await {
        Ok(HostResponse::ProviderCapabilities { claude_code, codex }) => {
            ProviderCapabilitiesView { claude_code, codex }
        }
        _ => ProviderCapabilitiesView {
            claude_code: true,
            codex: false,
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(capabilities)).into_response())
}

async fn jira_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.jira_readiness.readiness().await),
    )
        .into_response())
}

#[derive(Serialize)]
struct JiraAuthorizationStart {
    authorization_url: String,
}

#[derive(Deserialize)]
struct JiraAuthorizationCallback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

async fn jira_auth_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let oauth = state.jira_readiness.oauth_client().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "jira_oauth_unavailable",
            "Atlassian OAuth is not configured on this Swarm host",
        )
    })?;
    let url = oauth.authorization_url().await.map_err(jira_oauth_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(JiraAuthorizationStart {
            authorization_url: url.to_string(),
        }),
    )
        .into_response())
}

async fn jira_auth_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<JiraAuthorizationCallback>,
) -> Response {
    let Some(oauth) = state.jira_readiness.oauth_client() else {
        return Redirect::to("/?jira=unavailable#settings-integrations").into_response();
    };
    if query.error.is_some() {
        return Redirect::to("/?jira=denied#settings-integrations").into_response();
    }
    let result = match (query.state.as_deref(), query.code.as_deref()) {
        (Some(auth_state), Some(code)) => oauth.exchange_code(auth_state, code).await,
        _ => Err(jira_oauth::OAuthError::InvalidState),
    };
    let location = if result.is_ok() {
        "/?jira=connected#settings-integrations"
    } else {
        "/?jira=failed#settings-integrations"
    };
    Redirect::to(location).into_response()
}

async fn jira_disconnect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let oauth = state.jira_readiness.oauth_client().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "jira_oauth_unavailable",
            "Atlassian OAuth is not configured on this Swarm host",
        )
    })?;
    oauth.disconnect().await.map_err(jira_oauth_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn jira_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<JiraProjectsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let projects = state
        .jira_readiness
        .projects(query.query.as_deref())
        .await
        .map_err(jira_adapter_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(projects)).into_response())
}

async fn jira_project_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(project_id_or_key): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let statuses = state
        .jira_readiness
        .project_statuses(&project_id_or_key)
        .await
        .map_err(jira_adapter_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(statuses)).into_response())
}

async fn jira_bindings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let bindings = task_store(&state)?
        .list_jira_project_bindings()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(bindings)).into_response())
}

async fn jira_task_links(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = task_store(&state)?;
    let bindings = store
        .list_jira_project_bindings()
        .map_err(|error| task_store_error(&error))?;
    let browser_base_url = state.jira_readiness.browser_base_url().await;
    let mut links = Vec::new();
    for binding in bindings {
        for link in store
            .list_jira_issue_links(binding.id)
            .map_err(|error| task_store_error(&error))?
        {
            let outbound_state = store
                .jira_transition_state_for_task(link.task_id)
                .map_err(|error| task_store_error(&error))?
                .or(store
                    .jira_comment_state_for_task(link.task_id)
                    .map_err(|error| task_store_error(&error))?);
            let issue_url = browser_base_url
                .as_ref()
                .and_then(|base_url| jira::issue_url(base_url, &link.issue_key));
            links.push(JiraTaskLinkView {
                issue_id: link.issue_id,
                issue_key: link.issue_key,
                issue_url,
                binding_id: link.binding_id,
                project_key: binding.project_key.clone(),
                project_name: binding.project_name.clone(),
                task_id: link.task_id,
                jira_status_id: link.jira_status_id,
                jira_status_name: link.jira_status_name,
                jira_assignee_account_id: link.jira_assignee_account_id,
                jira_assignee_name: link.jira_assignee_name,
                remote_updated_at: link.remote_updated_at,
                last_synced_at: link.last_synced_at,
                outbound_state,
            });
        }
    }
    links.sort_by(|left, right| left.issue_key.cmp(&right.issue_key));
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(links)).into_response())
}

async fn jira_task_comments(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let link = task_store(&state)?
        .jira_issue_link_for_task(parse_task_id(&task_id)?)
        .map_err(|error| task_store_error(&error))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "jira_link_not_found",
                "this task is not linked to Jira",
            )
        })?;
    let comments = state
        .jira_readiness
        .comments(&link.issue_key)
        .await
        .map_err(jira_adapter_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(comments)).into_response())
}

async fn create_jira_task_comment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<JiraCommentRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    store
        .queue_jira_comment(task_id, &request.body)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_jira_comments().await;
    let state_name = store
        .jira_comment_state_for_task(task_id)
        .map_err(|error| task_store_error(&error))?
        .unwrap_or_else(|| "delivered".into());
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "state": state_name })),
    )
        .into_response())
}

async fn retry_jira_task_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let retried_transition = store
        .retry_jira_transition(task_id)
        .map_err(|error| task_store_error(&error))?;
    let retried_comments = store
        .retry_jira_comments(task_id)
        .map_err(|error| task_store_error(&error))?;
    if !retried_transition && !retried_comments {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "jira_transition_not_retryable",
            "This Jira task does not have a conflicting or uncertain update to retry",
        ));
    }
    state.control_room_notify.notify_waiters();
    state.deliver_jira_transitions().await;
    state.deliver_jira_comments().await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn create_jira_binding(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateJiraProjectBindingRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let binding = task_store(&state)?
        .upsert_jira_project_binding(&JiraProjectBindingInput {
            project_id: &request.id,
            project_key: &request.key,
            project_name: &request.name,
            scope: JiraProjectScope::Hive,
            apiary_id: None,
        })
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(binding),
    )
        .into_response())
}

async fn jira_mappings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(binding_id): Path<JiraProjectBindingId>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let mappings = task_store(&state)?
        .list_jira_status_mappings(binding_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(mappings)).into_response())
}

async fn replace_jira_mappings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(binding_id): Path<JiraProjectBindingId>,
    Json(request): Json<ReplaceJiraMappingsRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let mappings = task_store(&state)?
        .replace_jira_status_mappings(binding_id, &request.mappings)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(mappings)).into_response())
}

async fn set_jira_assigned_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(binding_id): Path<JiraProjectBindingId>,
    Json(request): Json<JiraAssignedSyncRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let binding = task_store(&state)?
        .set_jira_auto_sync_assigned(binding_id, request.enabled)
        .map_err(|error| task_store_error(&error))?;
    if request.enabled {
        state.reconcile_jira().await;
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(binding)).into_response())
}

async fn jira_binding_issues(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(binding_id): Path<JiraProjectBindingId>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let binding = task_store(&state)?
        .get_jira_project_binding(binding_id)
        .map_err(|error| task_store_error(&error))?;
    let issues = state
        .jira_readiness
        .hive_intake_issues(&binding.project_id)
        .await
        .map_err(jira_adapter_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(issues)).into_response())
}

async fn sync_jira_binding(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(binding_id): Path<JiraProjectBindingId>,
    Json(request): Json<SyncJiraBindingRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = task_store(&state)?;
    let binding = store
        .get_jira_project_binding(binding_id)
        .map_err(|error| task_store_error(&error))?;
    let issues = state
        .jira_readiness
        .hive_intake_issues(&binding.project_id)
        .await
        .map_err(jira_adapter_error)?;
    let selected_ids = request
        .issue_ids
        .into_iter()
        .map(|id| id.trim().to_owned())
        .collect::<HashSet<_>>();
    if selected_ids.is_empty()
        || selected_ids.len() > 100
        || selected_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_jira_selection",
            "choose between 1 and 100 Jira issues to import",
        ));
    }
    let mut selected_issues = issues
        .into_iter()
        .filter(|issue| selected_ids.contains(&issue.id))
        .collect::<Vec<_>>();
    if selected_ids.len() != selected_issues.len() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_jira_selection",
            "one or more selected Jira issues are no longer available",
        ));
    }
    state
        .jira_readiness
        .claim_unassigned_issues(&mut selected_issues)
        .await
        .map_err(jira_adapter_error)?;
    let snapshots = selected_issues
        .iter()
        .map(jira_issue_snapshot)
        .collect::<Vec<_>>();
    let tasks = store
        .sync_jira_issues(binding_id, &snapshots)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

async fn reconcile_jira_now(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    state.reconcile_jira().await;
    Ok(StatusCode::NO_CONTENT)
}

fn jira_issue_snapshot(issue: &jira::JiraIssue) -> JiraIssueSnapshot<'_> {
    JiraIssueSnapshot {
        issue_id: &issue.id,
        issue_key: &issue.key,
        summary: &issue.summary,
        description: &issue.description,
        status_id: &issue.status_id,
        status_name: &issue.status_name,
        assignee_account_id: issue.assignee_account_id.as_deref(),
        assignee_name: issue.assignee_name.as_deref(),
        remote_updated_at: &issue.updated_at,
    }
}

async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let profiles = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    let workspaces = workspace_catalog(&state, &profiles).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(workspaces)).into_response())
}

async fn workspace_catalog(
    state: &AppState,
    profiles: &[WorkerProfile],
) -> Result<Vec<WorkspaceView>, ApiError> {
    const MAX_WORKSPACES: usize = 256;
    const MAX_FOLDER_DEPTH: usize = 6;
    let mut workspaces = Vec::new();
    for root in state.workspace_roots.iter() {
        let entries = tokio::fs::read_dir(root).await.map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "workspace_catalog_unavailable",
                "configured repository catalog is unavailable",
            )
        })?;
        let mut pending = VecDeque::from([(entries, 0_usize)]);
        while let Some((mut entries, depth)) = pending.pop_front() {
            while let Some(entry) = entries.next_entry().await.map_err(|_| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "workspace_catalog_unavailable",
                    "configured repository catalog could not be read",
                )
            })? {
                if workspaces.len() >= MAX_WORKSPACES {
                    break;
                }
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if !file_type.is_dir() || file_type.is_symlink() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || name == "queen" {
                    continue;
                }
                let path = entry.path();
                let path_text = path.to_string_lossy().into_owned();
                let configured_worker_id = profiles
                    .iter()
                    .find(|profile| profile.workspace == path_text)
                    .map(|profile| profile.id);
                let repository = tokio::fs::try_exists(path.join(".git"))
                    .await
                    .unwrap_or(false);
                workspaces.push(WorkspaceView {
                    name,
                    path: path_text,
                    kind: if repository { "repository" } else { "folder" },
                    configured_worker_id,
                });
                if !repository
                    && depth < MAX_FOLDER_DEPTH
                    && let Ok(children) = tokio::fs::read_dir(&path).await
                {
                    pending.push_back((children, depth + 1));
                }
            }
            if workspaces.len() >= MAX_WORKSPACES {
                break;
            }
        }
        if workspaces.len() >= MAX_WORKSPACES {
            break;
        }
    }
    workspaces.sort_by_key(|workspace| workspace.path.to_lowercase());
    Ok(workspaces)
}

async fn resolve_workspace_path(
    state: &AppState,
    requested: &str,
    allow_outside_roots: bool,
) -> Result<PathBuf, ApiError> {
    let requested = FilePath::new(requested.trim());
    if !requested.is_absolute() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_workspace",
            "enter an absolute path inside a configured workspace root",
        ));
    }
    let metadata = tokio::fs::symlink_metadata(requested).await.map_err(|_| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_workspace",
            "that workspace folder does not exist",
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_workspace",
            "choose a real folder rather than a file or symbolic link",
        ));
    }
    let canonical = tokio::fs::canonicalize(requested).await.map_err(|_| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_workspace",
            "that workspace folder could not be resolved",
        )
    })?;
    if canonical.parent().is_none() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsafe_workspace",
            "a filesystem root cannot be used as a worker repository",
        ));
    }
    for root in state.workspace_roots.iter() {
        if let Ok(root) = tokio::fs::canonicalize(root).await
            && canonical.starts_with(root)
        {
            return Ok(canonical);
        }
    }
    if allow_outside_roots {
        return Ok(canonical);
    }
    Err(ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown_workspace",
        "that folder is outside the configured workspace roots",
    ))
}

async fn create_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let profiles = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    let workspace =
        resolve_workspace_path(&state, &request.workspace, request.allow_outside_roots).await?;
    let workspace = workspace.to_string_lossy().into_owned();
    if profiles
        .iter()
        .any(|profile| profile.workspace == workspace)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "workspace_already_assigned",
            "that repository already belongs to a worker",
        ));
    }
    let position = profiles
        .iter()
        .map(|profile| profile.position)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let profile = task_store(&state)?
        .create_worker(
            &request.name,
            request.provider,
            &workspace,
            request.autostart,
            position,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::CREATED,
        Json(worker_view(
            profile,
            false,
            false,
            None,
            ProviderActivity::Unknown,
        )),
    )
        .into_response())
}

async fn reorder_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReorderWorkersRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .reorder_workers(&request.worker_ids)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn update_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
    Json(request): Json<UpdateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .update_worker_profile(worker_id, request.name.as_deref(), request.autostart)
        .map_err(|error| task_store_error(&error))?;
    if request.autostart.is_some() {
        state.worker_errors.write().await.remove(&worker_id);
        state
            .worker_recovery_attempts
            .write()
            .await
            .remove(&worker_id);
    }
    let running = profile.active_session_id.is_some();
    state.control_room_notify.notify_waiters();
    Ok(Json(worker_view(
        profile,
        running,
        false,
        None,
        ProviderActivity::Unknown,
    ))
    .into_response())
}

async fn start_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
    Json(request): Json<StartWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    require_valid_size(request.rows, request.columns)?;
    let worker_id = parse_worker_id(&worker_id)?;
    state.worker_errors.write().await.remove(&worker_id);
    state
        .worker_recovery_attempts
        .write()
        .await
        .remove(&worker_id);
    let worker = start_worker_process(
        &state,
        worker_id,
        TerminalSize::new(request.rows, request.columns),
    )
    .await?;
    Ok(Json(worker).into_response())
}

async fn stop_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _guard = state.worker_lifecycle.lock().await;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    if let Some(session_id) = profile.active_session_id {
        request_host(&state, HostRequest::Stop { session_id }).await?;
        task_store(&state)?
            .release_worker_session(session_id)
            .map_err(|error| task_store_error(&error))?;
        task_store(&state)?
            .release_session_assignments(session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    state.worker_errors.write().await.remove(&worker_id);
    state
        .worker_recovery_attempts
        .write()
        .await
        .remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(worker_view(
        profile,
        false,
        false,
        None,
        ProviderActivity::Unknown,
    ))
    .into_response())
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HostResponse>, ApiError> {
    authorize(&state, &headers)?;
    let response = request_host(&state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    Ok(Json(HostResponse::Sessions {
        sessions: sessions
            .into_iter()
            .filter(|session| session.running)
            .collect(),
    }))
}

async fn history_diagnostics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::HistoryDiagnostics).await
}

async fn list_history_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::ListHistorySessions).await
}

async fn read_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Response, ApiError> {
    let cursor = match (query.segment, query.record) {
        (None, None) => None,
        (Some(segment), Some(record)) => Some(HistoryCursor { segment, record }),
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_history_cursor",
                "history cursor requires both segment and record",
            ));
        }
    };
    authorized_no_store_request(
        &state,
        &headers,
        HostRequest::ReadHistory {
            session_id: parse_session_id(&session_id)?,
            cursor,
        },
    )
    .await
}

async fn start_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<StartSessionRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    require_valid_size(request.rows, request.columns)?;
    let response = authorized_request(
        &state,
        &headers,
        HostRequest::StartClaude {
            workspace: request.workspace,
            size: TerminalSize::new(request.rows, request.columns),
            conversation: ClaudeConversationStart::New {
                session_id: ProviderConversationId::new(),
            },
            mcp_config: None,
            allow_outside_roots: false,
        },
    )
    .await?;
    record_session_event(&state)?;
    Ok(response)
}

async fn read_output(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(
        &state,
        &headers,
        HostRequest::Read {
            session_id: parse_session_id(&session_id)?,
            after_sequence: query.after,
        },
    )
    .await
}

async fn write_input(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<InputRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(
        &state,
        &headers,
        HostRequest::Write {
            session_id: parse_session_id(&session_id)?,
            bytes: request.text.into_bytes(),
        },
    )
    .await
}

async fn resize_terminal(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<ResizeRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    require_valid_size(request.rows, request.columns)?;
    authorized_request(
        &state,
        &headers,
        HostRequest::Resize {
            session_id: parse_session_id(&session_id)?,
            size: TerminalSize::new(request.rows, request.columns),
        },
    )
    .await
}

async fn stop_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<HostResponse>, ApiError> {
    let session_id = parse_session_id(&session_id)?;
    let response = authorized_request(&state, &headers, HostRequest::Stop { session_id }).await?;
    if let Some(store) = &state.task_store {
        store
            .release_worker_session(session_id)
            .map_err(|error| task_store_error(&error))?;
        store
            .release_session_assignments(session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    record_session_event(&state)?;
    Ok(response)
}

async fn start_worker_process(
    state: &AppState,
    worker_id: WorkerId,
    size: TerminalSize,
) -> Result<WorkerView, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    let live = reconcile_worker_bindings_unlocked(state).await?;
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    if let Some(session_id) = profile.active_session_id
        && live.contains(&session_id)
    {
        return Ok(worker_view(
            profile,
            true,
            false,
            None,
            ProviderActivity::Unknown,
        ));
    }
    let mcp_config = if profile.provider == ProviderKind::ClaudeCode {
        state
            .agent_bridge
            .as_ref()
            .map(|bridge| bridge.ensure_worker_config(worker_id))
            .transpose()
            .map_err(|error| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "agent_config_unavailable",
                    error.to_string(),
                )
            })?
    } else {
        None
    };

    let worker_workspace = PathBuf::from(&profile.workspace);
    let allow_outside_roots = !state
        .workspace_roots
        .iter()
        .any(|root| worker_workspace.starts_with(root));
    let request = match profile.provider {
        ProviderKind::ClaudeCode => HostRequest::StartClaude {
            workspace: worker_workspace.clone(),
            size,
            conversation: match (
                profile.provider_conversation_id,
                profile.has_session_history,
            ) {
                (Some(session_id), false) => ClaudeConversationStart::New { session_id },
                (Some(session_id), true) => ClaudeConversationStart::Resume { session_id },
                (None, true) => ClaudeConversationStart::Continue,
                (None, false) => {
                    let session_id = task_store(state)?
                        .assign_provider_conversation(worker_id)
                        .map_err(|error| task_store_error(&error))?;
                    ClaudeConversationStart::New { session_id }
                }
            },
            mcp_config,
            allow_outside_roots,
        },
        ProviderKind::Codex => HostRequest::StartCodex {
            workspace: worker_workspace,
            size,
            conversation: if profile.has_session_history {
                CodexConversationStart::Continue
            } else {
                CodexConversationStart::New
            },
            allow_outside_roots,
        },
    };
    let response = request_host(state, request).await?;
    let HostResponse::SessionStarted { session_id } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    if let Err(error) = task_store(state)?.bind_worker_session(worker_id, session_id) {
        let _ = request_host(state, HostRequest::Stop { session_id }).await;
        return Err(task_store_error(&error));
    }
    state.worker_errors.write().await.remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(worker_view(
        profile,
        true,
        false,
        None,
        ProviderActivity::Unknown,
    ))
}

async fn reconcile_worker_bindings(state: &AppState) -> Result<HashSet<WorkerSessionId>, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    reconcile_worker_bindings_unlocked(state).await
}

async fn reconcile_worker_bindings_unlocked(
    state: &AppState,
) -> Result<HashSet<WorkerSessionId>, ApiError> {
    let response = request_host(state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    let live = sessions
        .into_iter()
        .filter(|session| session.running)
        .map(|session| session.session_id)
        .collect::<HashSet<_>>();
    let released = task_store(state)?
        .release_missing_worker_sessions(&live)
        .map_err(|error| task_store_error(&error))?;
    if released > 0 {
        state.control_room_notify.notify_waiters();
    }
    Ok(live)
}

async fn request_host(state: &AppState, request: HostRequest) -> Result<HostResponse, ApiError> {
    let response = terminal_client(state)?
        .request(&request)
        .await
        .map_err(|error| host_unavailable(&error))?;
    if let HostResponse::Error { message, .. } = &response {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "terminal_operation_failed",
            message,
        ));
    }
    Ok(response)
}

async fn issue_attach_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let client = terminal_client(&state)?;
    match client
        .request(&HostRequest::Read {
            session_id,
            after_sequence: Some(u64::MAX),
        })
        .await
        .map_err(|error| host_unavailable(&error))?
    {
        HostResponse::Output { .. } => {}
        HostResponse::Error { message, .. } => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "terminal_operation_failed",
                message,
            ));
        }
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "unexpected_host_response",
                "terminal host returned an unexpected response",
            ));
        }
    }
    let grant = state
        .attach_grants
        .issue(session_id)
        .map_err(|error| attach_grant_error(&error))?;
    let response = AttachGrantResponse {
        grant,
        protocol: TERMINAL_WEBSOCKET_PROTOCOL,
        websocket_path: format!("/api/v1/terminal/sessions/{session_id}/attach"),
        expires_in_ms: u64::try_from(ATTACH_GRANT_TTL.as_millis()).unwrap_or(u64::MAX),
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

async fn attach_terminal(
    websocket: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let session_id = parse_session_id(&session_id)?;
    let grant = websocket_grant(&headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "attach_grant_required",
            "a short-lived terminal attach grant is required",
        )
    })?;
    let permit = Arc::clone(&state.websocket_limit)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "terminal_websocket_limit_reached",
                "terminal WebSocket capacity is exhausted",
            )
        })?;
    if !state.attach_grants.consume(grant, session_id) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_attach_grant",
            "the terminal attach grant is invalid, expired, or already used",
        ));
    }
    let client = terminal_client(&state)?.clone();
    let store = task_store(&state)?.clone();
    let control_room_notify = Arc::clone(&state.control_room_notify);
    Ok(websocket
        .protocols([TERMINAL_WEBSOCKET_PROTOCOL])
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .max_frame_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            serve_terminal_socket(socket, client, session_id, store, control_room_notify).await;
        }))
}

async fn release_terminal_engagement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_id, device_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "device_id must be a UUID",
        )
    })?;
    let released = task_store(&state)?
        .release_worker_engagement(session_id, device_id)
        .map_err(|error| task_store_error(&error))?;
    if released {
        state.control_room_notify.notify_waiters();
        state.deliver_coordination().await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct TerminalAttachmentResponse {
    path: String,
}

async fn upload_terminal_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let HostResponse::Sessions { sessions } =
        request_host(&state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "terminal_protocol_error",
            "terminal host returned an unexpected response",
        ));
    };
    if !sessions
        .iter()
        .any(|session| session.session_id == session_id)
    {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "terminal_session_not_found",
            "terminal session does not exist",
        ));
    }
    let path = store
        .save(media_type, &body)
        .await
        .map_err(attachment_error)?;
    let path = path.to_str().ok_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attachment_path_unavailable",
            "private attachment path is not valid UTF-8",
        )
    })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TerminalAttachmentResponse { path: path.into() }),
    )
        .into_response())
}

fn attachment_error(error: AttachmentError) -> ApiError {
    match error {
        AttachmentError::UnsupportedType | AttachmentError::InvalidSignature => ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_attachment_type",
            error.to_string(),
        ),
        AttachmentError::InvalidSize => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "invalid_attachment_size",
            error.to_string(),
        ),
        AttachmentError::Capacity => ApiError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "attachment_capacity_reached",
            error.to_string(),
        ),
        AttachmentError::Unavailable => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attachment_store_unavailable",
            error.to_string(),
        ),
        AttachmentError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "feedback_attachment_not_found",
            error.to_string(),
        ),
    }
}

async fn authorized_request(
    state: &AppState,
    headers: &HeaderMap,
    request: HostRequest,
) -> Result<Json<HostResponse>, ApiError> {
    authorize(state, headers)?;
    let client = terminal_client(state)?;
    let response = client
        .request(&request)
        .await
        .map_err(|error| host_unavailable(&error))?;
    if let HostResponse::Error { message, .. } = &response {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "terminal_operation_failed",
            message,
        ));
    }
    Ok(Json(response))
}

async fn authorized_no_store_request(
    state: &AppState,
    headers: &HeaderMap,
    request: HostRequest,
) -> Result<Response, ApiError> {
    let Json(response) = authorized_request(state, headers, request).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

fn terminal_client(state: &AppState) -> Result<&HostClient, ApiError> {
    state.terminal_host.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_unconfigured",
            "terminal host is not configured",
        )
    })
}

fn record_session_event(state: &AppState) -> Result<(), ApiError> {
    if let Some(store) = &state.task_store {
        store
            .record_control_room_event(ControlRoomEventKind::SessionsChanged)
            .map_err(|error| task_store_error(&error))?;
    }
    state.control_room_notify.notify_waiters();
    Ok(())
}

fn task_store(state: &AppState) -> Result<&TaskStore, ApiError> {
    state.task_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "task_store_unconfigured",
            "task persistence is not configured",
        )
    })
}

fn task_service(state: &AppState) -> Result<TaskService, ApiError> {
    task_store(state).map(|store| TaskService::new(store.clone()))
}

fn apiary_service(state: &AppState) -> Result<ApiaryService, ApiError> {
    task_store(state).map(|store| ApiaryService::new(store.clone()))
}

fn application_error(error: ApplicationError) -> ApiError {
    match error {
        ApplicationError::NotAuthorized => ApiError::new(
            StatusCode::FORBIDDEN,
            "task_outcome_not_authorized",
            error.to_string(),
        ),
        ApplicationError::WorkerNotRunning => ApiError::new(
            StatusCode::CONFLICT,
            "worker_session_not_active",
            error.to_string(),
        ),
        ApplicationError::IntegrationUnavailable(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "integration_unavailable",
            error.to_string(),
        ),
        ApplicationError::Store(error) => task_store_error(&error),
    }
}

fn jira_adapter_error(error: jira::JiraAdapterError) -> ApiError {
    match error {
        jira::JiraAdapterError::NotConfigured => ApiError::new(
            StatusCode::CONFLICT,
            "jira_not_configured",
            "connect Jira before browsing projects",
        ),
        jira::JiraAdapterError::CredentialsInvalid => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "jira_credentials_invalid",
            "Jira rejected this operator's credentials",
        ),
        jira::JiraAdapterError::PermissionDenied => ApiError::new(
            StatusCode::FORBIDDEN,
            "jira_access_denied",
            "Jira denied access to this project",
        ),
        jira::JiraAdapterError::NetworkUnavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "jira_network_unavailable",
            "Jira is temporarily unavailable",
        ),
        jira::JiraAdapterError::InvalidResponse => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "jira_response_invalid",
            "Jira returned an invalid response",
        ),
        jira::JiraAdapterError::ResponseLimitExceeded => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "jira_response_limit_exceeded",
            "Jira returned more data than this bounded operation permits",
        ),
        jira::JiraAdapterError::TransitionUnavailable => ApiError::new(
            StatusCode::CONFLICT,
            "jira_transition_unavailable",
            "Jira does not offer a workflow transition mapped to that Swarm state",
        ),
    }
}

fn jira_oauth_error(error: jira_oauth::OAuthError) -> ApiError {
    match error {
        jira_oauth::OAuthError::NotConnected => ApiError::new(
            StatusCode::CONFLICT,
            "jira_not_connected",
            "connect Jira before continuing",
        ),
        jira_oauth::OAuthError::CredentialsInvalid => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "jira_oauth_invalid",
            "Atlassian authorization needs to be renewed",
        ),
        jira_oauth::OAuthError::PermissionDenied => ApiError::new(
            StatusCode::FORBIDDEN,
            "jira_oauth_denied",
            "Atlassian did not grant the required Jira access",
        ),
        jira_oauth::OAuthError::NetworkUnavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "jira_oauth_unavailable",
            "Atlassian authorization is temporarily unavailable",
        ),
        jira_oauth::OAuthError::InvalidState => ApiError::new(
            StatusCode::BAD_REQUEST,
            "jira_oauth_state_invalid",
            "This Jira connection attempt expired or was already used",
        ),
        jira_oauth::OAuthError::InvalidResponse | jira_oauth::OAuthError::Storage => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "jira_oauth_failed",
            "Jira authorization could not be stored safely",
        ),
    }
}
#[allow(clippy::too_many_lines)]
fn task_store_error(error: &TaskStoreError) -> ApiError {
    match error {
        TaskStoreError::NotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "task_not_found", error.to_string())
        }
        TaskStoreError::InvalidApiaryInvitation => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_apiary_invitation",
            error.to_string(),
        ),
        TaskStoreError::InvalidApiary => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_apiary", error.to_string())
        }
        TaskStoreError::ApiaryMembershipConflict => ApiError::new(
            StatusCode::CONFLICT,
            "apiary_membership_conflict",
            error.to_string(),
        ),
        TaskStoreError::ApiaryNotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "apiary_not_found", error.to_string())
        }
        TaskStoreError::ApiaryInvitationNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "apiary_invitation_not_found",
            error.to_string(),
        ),
        TaskStoreError::ApiaryInvitationResolved | TaskStoreError::ApiaryJoinNotReady => {
            ApiError::new(
                StatusCode::CONFLICT,
                "apiary_join_not_ready",
                error.to_string(),
            )
        }
        TaskStoreError::DecisionNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "decision_not_found",
            error.to_string(),
        ),
        TaskStoreError::InvalidDecisionContent
        | TaskStoreError::InvalidDecisionActions
        | TaskStoreError::InvalidDecisionDeadline
        | TaskStoreError::InvalidDecisionResolution => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_decision",
            error.to_string(),
        ),
        TaskStoreError::DecisionAlreadyResolved | TaskStoreError::DecisionInboxFull => {
            ApiError::new(StatusCode::CONFLICT, "decision_conflict", error.to_string())
        }
        TaskStoreError::InvalidTitle
        | TaskStoreError::InvalidDescription
        | TaskStoreError::InvalidWorkspace
        | TaskStoreError::EmptyTaskDetailsUpdate
        | TaskStoreError::InvalidTaskActivityNote => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_task", error.to_string())
        }
        TaskStoreError::InvalidTransition { .. } | TaskStoreError::CompletedTask => ApiError::new(
            StatusCode::CONFLICT,
            "task_transition_rejected",
            error.to_string(),
        ),
        TaskStoreError::TaskDispatchQueueFull => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "task_dispatch_queue_full",
            error.to_string(),
        ),
        TaskStoreError::PresenceDeviceLimit => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "presence_device_limit",
            error.to_string(),
        ),
        TaskStoreError::InvalidNotificationSubscription
        | TaskStoreError::InvalidVapidKey
        | TaskStoreError::NotificationSubscriptionLimit
        | TaskStoreError::NotificationQueueFull => notification_store_error(error),
        TaskStoreError::TaskOutcomeQueueFull => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "task_outcome_queue_full",
            error.to_string(),
        ),
        TaskStoreError::InvalidTaskOrder | TaskStoreError::InvalidWorkerOrder => ApiError::new(
            StatusCode::CONFLICT,
            "task_order_conflict",
            error.to_string(),
        ),
        TaskStoreError::InvalidDogfoodReport
        | TaskStoreError::InvalidDogfoodAttachment
        | TaskStoreError::InvalidDogfoodReportLimit => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_dogfood_report",
            error.to_string(),
        ),
        TaskStoreError::InvalidJiraProject | TaskStoreError::InvalidJiraWorkflowMapping => {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_jira_configuration",
                error.to_string(),
            )
        }
        TaskStoreError::JiraProjectBindingNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "jira_project_binding_not_found",
            error.to_string(),
        ),
        TaskStoreError::JiraTransitionPending => ApiError::new(
            StatusCode::CONFLICT,
            "jira_transition_pending",
            error.to_string(),
        ),
        TaskStoreError::JiraTransitionQueueFull => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "jira_transition_queue_full",
            error.to_string(),
        ),
        TaskStoreError::InvalidJiraComment => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_jira_comment",
            error.to_string(),
        ),
        TaskStoreError::JiraCommentQueueFull => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "jira_comment_queue_full",
            error.to_string(),
        ),
        TaskStoreError::WorkerNotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "worker_not_found", error.to_string())
        }
        TaskStoreError::InvalidWorkerName | TaskStoreError::EmptyWorkerUpdate => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_worker", error.to_string())
        }
        TaskStoreError::DuplicateWorkerName
        | TaskStoreError::QueenAlreadyExists
        | TaskStoreError::QueenProfileImmutable => {
            ApiError::new(StatusCode::CONFLICT, "worker_conflict", error.to_string())
        }
        TaskStoreError::WorkerAlreadyRunning => ApiError::new(
            StatusCode::CONFLICT,
            "worker_already_running",
            error.to_string(),
        ),
        TaskStoreError::WorkerSessionNotActive => ApiError::new(
            StatusCode::CONFLICT,
            "worker_session_not_active",
            error.to_string(),
        ),
        TaskStoreError::ProviderConversationUnavailable => ApiError::new(
            StatusCode::CONFLICT,
            "provider_conversation_unavailable",
            error.to_string(),
        ),
        TaskStoreError::Io(_)
        | TaskStoreError::Sql(_)
        | TaskStoreError::LockPoisoned
        | TaskStoreError::InvalidAgentCredentialDigest
        | TaskStoreError::UnsupportedSchemaVersion { .. }
        | TaskStoreError::IntegrityFailure(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "task_store_unavailable",
            "task persistence is temporarily unavailable",
        ),
    }
}

fn notification_store_error(error: &TaskStoreError) -> ApiError {
    match error {
        TaskStoreError::InvalidNotificationSubscription | TaskStoreError::InvalidVapidKey => {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_notification_configuration",
                error.to_string(),
            )
        }
        TaskStoreError::NotificationSubscriptionLimit | TaskStoreError::NotificationQueueFull => {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "notification_capacity_reached",
                error.to_string(),
            )
        }
        _ => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notification_persistence_unavailable",
            "notification persistence is temporarily unavailable",
        ),
    }
}
fn host_unavailable(error: &swarm_terminal::IpcError) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "terminal_host_unavailable",
        error.to_string(),
    )
}

fn attach_grant_error(error: &AttachGrantError) -> ApiError {
    match error {
        AttachGrantError::CapacityReached { .. } => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "attach_grant_limit_reached",
            error.to_string(),
        ),
        AttachGrantError::RandomnessUnavailable | AttachGrantError::LockPoisoned => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attach_grant_unavailable",
            error.to_string(),
        ),
    }
}

fn websocket_grant(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(TERMINAL_GRANT_PROTOCOL_PREFIX))
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = state.operator_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let presented_bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let expected_session = browser_session_value(expected);
    let presented_session = cookie_value(headers, OPERATOR_SESSION_COOKIE).unwrap_or_default();
    let bearer_matches = presented_bearer.len() == expected.len()
        && bool::from(presented_bearer.as_bytes().ct_eq(expected.as_bytes()));
    let session_matches = presented_session.len() == expected_session.len()
        && bool::from(
            presented_session
                .as_bytes()
                .ct_eq(expected_session.as_bytes()),
        );
    if !bearer_matches && !session_matches {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_operator_token",
            "a valid operator session is required",
        ));
    }
    Ok(())
}

fn browser_session_set_cookie(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<HeaderValue, ApiError> {
    let expected = state.operator_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_unconfigured",
            "operator authentication is not configured",
        )
    })?;
    let secure = if request_is_secure(headers) {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{OPERATOR_SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={OPERATOR_SESSION_MAX_AGE_SECONDS}{secure}",
        browser_session_value(expected),
    ))
    .map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "operator_session_unavailable",
            "browser session could not be created",
        )
    })
}

fn browser_session_value(operator_token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"swarm-next.operator-session.v1\0");
    digest.update(operator_token.as_bytes());
    Base64UrlUnpadded::encode_string(&digest.finalize())
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(name)?.strip_prefix('='))
}

fn request_is_secure(headers: &HeaderMap) -> bool {
    if headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
    {
        return true;
    }
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|host| {
            !host.starts_with("localhost")
                && !host.starts_with("127.0.0.1")
                && !host.starts_with("[::1]")
        })
}

fn parse_session_id(value: &str) -> Result<WorkerSessionId, ApiError> {
    WorkerSessionId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_session_id",
            "session ID must be a UUID",
        )
    })
}

fn parse_worker_id(value: &str) -> Result<WorkerId, ApiError> {
    WorkerId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_worker_id",
            "worker ID must be a UUID",
        )
    })
}

fn parse_task_id(value: &str) -> Result<TaskId, ApiError> {
    TaskId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_id",
            "task ID must be a UUID",
        )
    })
}
fn parse_decision_id(value: &str) -> Result<DecisionRequestId, ApiError> {
    DecisionRequestId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_decision_id",
            "decision ID must be a UUID",
        )
    })
}

fn parse_apiary_invitation_id(value: &str) -> Result<ApiaryInvitationId, ApiError> {
    ApiaryInvitationId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_apiary_invitation_id",
            "Apiary invitation ID must be a UUID",
        )
    })
}

fn require_valid_size(rows: u16, columns: u16) -> Result<(), ApiError> {
    let cells = usize::from(rows) * usize::from(columns);
    if rows < MIN_TERMINAL_ROWS
        || columns < MIN_TERMINAL_COLUMNS
        || rows > MAX_TERMINAL_ROWS
        || columns > MAX_TERMINAL_COLUMNS
        || cells > MAX_TERMINAL_CELLS
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_terminal_size",
            format!(
                "terminal dimensions must be at least {MIN_TERMINAL_ROWS} rows and \
                 {MIN_TERMINAL_COLUMNS} columns, and within {MAX_TERMINAL_ROWS} rows, \
                 {MAX_TERMINAL_COLUMNS} columns, and {MAX_TERMINAL_CELLS} cells"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::{body::Body, http::Request};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use swarm_domain::WorkerSessionId;
    use swarm_terminal::{
        HistoryLimits, HistoryStore, JournalLimits, PROTOCOL_VERSION, ProviderCommand,
        SessionRegistry, TerminalSnapshot,
    };
    use swarm_terminal_host::HostServer;
    use tempfile::TempDir;
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async,
        tungstenite::{Message as ClientMessage, client::IntoClientRequest},
    };
    use tower::ServiceExt;

    use super::*;

    #[test]
    fn decision_delivery_is_one_sanitized_terminal_submission() {
        let delivery = DecisionDispatch {
            decision_id: DecisionRequestId::new(),
            worker_id: WorkerId::new(),
            session_id: WorkerSessionId::new(),
            action: "ship\nnow".into(),
            note: "green\u{1b}[31m\rchecks".into(),
        };
        let message = decision_delivery_message(&delivery);
        assert_eq!(message.last(), Some(&b'\r'));
        assert!(!message[..message.len() - 1].contains(&b'\n'));
        assert!(!message.contains(&0x1b));
        assert!(String::from_utf8_lossy(&message).contains("ship now"));
    }
    #[test]
    fn task_dispatch_is_one_sanitized_terminal_submission() {
        let dispatch = TaskDispatch {
            assignment_id: "assignment-1".into(),
            task_id: TaskId::new(),
            worker_id: WorkerId::new(),
            session_id: WorkerSessionId::new(),
            title: "polish\nmobile".into(),
            description: "keep\u{1b}[31m context\rstable".into(),
            priority: TaskPriority::High,
            workspace: "/workspace/petal".into(),
        };
        let message = task_dispatch_message(&dispatch);
        assert_eq!(message.last(), Some(&b'\r'));
        assert!(!message[..message.len() - 1].contains(&b'\n'));
        assert!(!message.contains(&0x1b));
        assert!(String::from_utf8_lossy(&message).contains("polish mobile"));
    }
    #[test]
    fn task_outcome_is_one_sanitized_terminal_submission() {
        let outcome = TaskOutcomeDispatch {
            id: "outcome-1".into(),
            task_id: TaskId::new(),
            reporting_worker_id: WorkerId::new(),
            reporting_worker_name: "Petal\nBee".into(),
            recipient_worker_id: WorkerId::new(),
            session_id: WorkerSessionId::new(),
            title: "Mobile\u{1b}[31m controls".into(),
            target_state: TaskState::Review,
            note: "Shipped\rand verified".into(),
        };
        let message = task_outcome_message(&outcome);
        assert_eq!(message.last(), Some(&b'\r'));
        assert!(!message[..message.len() - 1].contains(&b'\n'));
        assert!(!message.contains(&0x1b));
        assert!(String::from_utf8_lossy(&message).contains("Petal Bee"));
    }
    #[tokio::test]
    async fn health_is_versioned() {
        let response = router(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        let json = response_json(response).await;
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], build_version());
    }

    #[test]
    fn resource_pressure_classification_is_explicit_at_each_boundary() {
        let sample = |resident_memory_bytes| {
            resource_response(Some(ProcessResourceSample {
                resident_memory_bytes,
                process_tree_resident_memory_bytes: resident_memory_bytes,
                process_tree_process_count: resident_memory_bytes.map(|_| 1),
            }))
        };
        assert_eq!(
            sample(Some(RESOURCE_ADVISORY_BYTES - 1)).pressure,
            ResourcePressure::Normal
        );
        assert_eq!(
            sample(Some(RESOURCE_ADVISORY_BYTES)).pressure,
            ResourcePressure::Advisory
        );
        assert_eq!(
            sample(Some(RESOURCE_CRITICAL_BYTES)).pressure,
            ResourcePressure::Critical
        );
        assert_eq!(sample(None).pressure, ResourcePressure::Unavailable);
        assert_eq!(
            resource_response(None).pressure,
            ResourcePressure::Unavailable
        );
    }

    #[tokio::test]
    async fn local_hive_identity_is_private_and_stable() {
        let store = TaskStore::in_memory().unwrap();
        let expected = store.local_hive_identity().unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let app = router(state);

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/hive")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/hive").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json["operator"]["id"], expected.operator.id.to_string());
        assert_eq!(json["operator"]["display_name"], "Operator");
        assert_eq!(json["hive"]["id"], expected.hive.id.to_string());
        assert_eq!(json["hive"]["name"], "My Hive");
        assert!(json["hive"]["apiary_id"].is_null());
        assert_eq!(json["apiary_context"]["mode"], "personal");
    }

    #[tokio::test]
    async fn apiary_invitation_overview_is_private_and_empty_for_a_personal_hive() {
        let store = TaskStore::in_memory().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/apiary/invitations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/apiary/invitations").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(response).await, serde_json::json!([]));
    }

    #[tokio::test]
    async fn personal_hive_can_found_one_apiary_only_through_private_application_command() {
        let store = TaskStore::in_memory().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );
        let body = serde_json::json!({
            "name": "Wildflower Garden",
            "shared_work_backend": "jira"
        })
        .to_string();
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(created).await;
        assert_eq!(json["mode"], "federated");
        assert_eq!(json["apiary"]["name"], "Wildflower Garden");
        assert_eq!(json["apiary"]["shared_work_backend"], "jira");
        assert_eq!(json["local_role"], "keeper");

        let duplicate = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(duplicate).await["code"],
            "apiary_membership_conflict"
        );
    }

    #[tokio::test]
    async fn invitation_acceptance_commands_never_bypass_operator_authentication() {
        let invitation_id = ApiaryInvitationId::new();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(TaskStore::in_memory().unwrap()),
        );
        let policy = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/invitations/{invitation_id}/policy-acceptance"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"policy_revision":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(policy.status(), StatusCode::UNAUTHORIZED);

        let join = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/apiary/invitations/{invitation_id}/join"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(join.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn provider_capabilities_are_private_and_degrade_for_an_older_host() {
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret"),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/providers").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json["claude_code"], true);
        assert_eq!(json["codex"], false);
    }

    #[test]
    fn explicit_decisions_drive_attention_without_overriding_operator_engagement() {
        let store = TaskStore::in_memory().unwrap();
        let profile = store.ensure_queen("/workspace/queen").unwrap();
        assert_eq!(
            worker_view(profile.clone(), true, true, None, ProviderActivity::Resting)
                .attention_state,
            WorkerAttentionState::AwaitingOperator
        );
        let mut engaged = profile;
        engaged.engagement_expires_at = Some(400);
        assert_eq!(
            worker_view(engaged, true, true, None, ProviderActivity::Resting).attention_state,
            WorkerAttentionState::WithOperator
        );
    }

    #[test]
    fn provider_activity_distinguishes_loaded_idle_from_active_and_unloaded() {
        let store = TaskStore::in_memory().unwrap();
        let profile = store.ensure_queen("/workspace/queen").unwrap();
        assert_eq!(
            worker_view(
                profile.clone(),
                true,
                false,
                None,
                ProviderActivity::Resting,
            )
            .attention_state,
            WorkerAttentionState::Resting
        );
        assert_eq!(
            worker_view(profile.clone(), true, false, None, ProviderActivity::Active,)
                .attention_state,
            WorkerAttentionState::Buzzing
        );
        assert_eq!(
            worker_view(profile, false, false, None, ProviderActivity::Active).attention_state,
            WorkerAttentionState::Sleeping
        );
    }

    #[tokio::test]
    async fn jira_readiness_is_private_and_explicit_when_not_configured() {
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret"),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/integrations/jira/readiness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/integrations/jira/readiness").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json["configured"], false);
        assert_eq!(json["connection"], "not_connected");
        assert!(json["account_name"].is_null());
    }

    #[tokio::test]
    async fn jira_project_binding_and_workflow_mapping_are_private_and_durable() {
        let store = TaskStore::in_memory().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/integrations/jira/bindings")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": "10001",
                            "project_key": "WEB",
                            "project_name": "Website Services",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let create = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/integrations/jira/bindings")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": "10001",
                            "project_key": "WEB",
                            "project_name": "Website Services",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
        assert_eq!(create.headers()[header::CACHE_CONTROL], "no-store");
        let binding = response_json(create).await;
        let binding_id = binding["id"].as_str().unwrap();
        assert!(binding.get("default_worker_id").is_none());

        let mapped = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/integrations/jira/bindings/{binding_id}/mappings"
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "mappings": [
                            { "jira_status_id": "1", "jira_status_name": "To Do", "task_state": "ready" },
                            { "jira_status_id": "3", "jira_status_name": "In Progress", "task_state": "active" }
                        ]})
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mapped.status(), StatusCode::OK);
        let listed =
            response_json(authorized_get(app, "/api/v1/integrations/jira/bindings").await).await;
        assert_eq!(listed[0]["project_key"], "WEB");
        assert_eq!(listed[0]["workflow_mapped"], true);
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn jira_sync_composes_remote_search_mapping_and_idempotent_task_intake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let assignment_writes = Arc::new(AtomicUsize::new(0));
        let written = assignment_writes.clone();
        let comment_writes = Arc::new(AtomicUsize::new(0));
        let comment_written = comment_writes.clone();
        let jira_server = axum::Router::new()
            .route(
                "/rest/api/3/myself",
                get(|| async {
                    Json(serde_json::json!({
                        "accountId": "account-1",
                        "displayName": "Bea"
                    }))
                }),
            )
            .route(
                "/rest/api/3/search/jql",
                get(|| async {
                    Json(serde_json::json!({
                        "isLast": true,
                        "issues": [{
                            "id": "20001",
                            "key": "WEB-42",
                            "fields": {
                                "summary": "Polish the launch page",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": { "accountId": "account-1", "displayName": "Bea" },
                                "updated": "2026-08-13T13:00:00.000+0000"
                            }
                        }, {
                            "id": "20002",
                            "key": "WEB-43",
                            "fields": {
                                "summary": "Keep the unselected issue remote",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": null,
                                "updated": "2026-08-13T13:01:00.000+0000"
                            }
                        }]
                    }))
                }),
            )
            .route(
                "/rest/api/3/issue/20002/assignee",
                axum::routing::put(move |Json(body): Json<serde_json::Value>| {
                    let written = written.clone();
                    async move {
                        assert_eq!(body["accountId"], "account-1");
                        written.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    }
                }),
            )
            .route(
                "/rest/api/3/issue/WEB-42/comment",
                get(|| async {
                    Json(serde_json::json!({
                        "comments": [{
                            "id": "comment-1",
                            "author": { "accountId": "account-1", "displayName": "Bea" },
                            "body": { "type": "doc", "version": 1, "content": [{
                                "type": "paragraph",
                                "content": [{ "type": "text", "text": "Ready for review" }]
                            }]},
                            "created": "2026-08-13T13:00:00.000+0000",
                            "updated": "2026-08-13T13:00:00.000+0000"
                        }]
                    }))
                })
                .post(move |Json(body): Json<serde_json::Value>| {
                    let comment_written = comment_written.clone();
                    async move {
                        assert_eq!(
                            body["body"]["content"][0]["content"][0]["text"],
                            "Shipped cleanly"
                        );
                        comment_written.fetch_add(1, Ordering::SeqCst);
                        StatusCode::CREATED
                    }
                }),
            )
            .route(
                "/rest/api/3/issue/WEB-42/transitions",
                get(|| async {
                    Json(serde_json::json!({ "transitions": [
                        { "id": "41", "to": { "id": "4", "name": "In Review" } }
                    ] }))
                })
                .post(|| async { StatusCode::NO_CONTENT }),
            );
        tokio::spawn(async move { axum::serve(listener, jira_server).await.unwrap() });

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
                &[
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                    JiraStatusMapping {
                        jira_status_id: "4".into(),
                        jira_status_name: "In Review".into(),
                        task_state: TaskState::Review,
                    },
                ],
            )
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store)
                .with_jira_configuration(
                    &format!("http://{address}"),
                    "operator@example.test",
                    "api-token",
                )
                .unwrap(),
        );
        for expected_count in [1, 1] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!(
                            "/api/v1/integrations/jira/bindings/{}/sync",
                            binding.id
                        ))
                        .header("authorization", "Bearer secret")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"issue_ids":["20001"]}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response_json(response).await.as_array().unwrap().len(),
                expected_count
            );
        }
        let claimed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/integrations/jira/bindings/{}/sync",
                        binding.id
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"issue_ids":["20002"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claimed.status(), StatusCode::OK);
        assert_eq!(assignment_writes.load(Ordering::SeqCst), 1);
        let tasks = response_json(authorized_get(app.clone(), "/api/v1/tasks").await).await;
        assert_eq!(tasks.as_array().unwrap().len(), 2);
        assert_eq!(tasks[0]["title"], "Polish the launch page");
        assert_eq!(tasks[0]["state"], "active");
        let links = response_json(
            authorized_get(app.clone(), "/api/v1/integrations/jira/task-links").await,
        )
        .await;
        assert_eq!(links.as_array().unwrap().len(), 2);
        assert_eq!(links[0]["issue_key"], "WEB-42");
        assert_eq!(
            links[0]["issue_url"],
            format!("http://{address}/browse/WEB-42")
        );
        assert_eq!(links[0]["project_name"], "Website Services");
        assert_eq!(links[0]["jira_status_name"], "In Progress");
        assert_eq!(links[0]["jira_assignee_name"], "Bea");
        assert_eq!(links[0]["task_id"], tasks[0]["id"]);
        assert_eq!(links[1]["issue_key"], "WEB-43");
        assert_eq!(links[1]["jira_assignee_name"], "Bea");

        let comments = response_json(
            authorized_get(
                app.clone(),
                &format!(
                    "/api/v1/integrations/jira/task-links/{}/comments",
                    tasks[0]["id"].as_str().unwrap()
                ),
            )
            .await,
        )
        .await;
        assert_eq!(comments[0]["body"], "Ready for review");
        let commented = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/integrations/jira/task-links/{}/comments",
                        tasks[0]["id"].as_str().unwrap()
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"body":"Shipped cleanly"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(commented.status(), StatusCode::ACCEPTED);
        assert_eq!(response_json(commented).await["state"], "delivered");
        assert_eq!(comment_writes.load(Ordering::SeqCst), 1);

        let transitioned = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!(
                        "/api/v1/tasks/{}/state",
                        tasks[0]["id"].as_str().unwrap()
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"state":"review"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(transitioned.status(), StatusCode::OK);
        assert_eq!(response_json(transitioned).await["state"], "review");
        let transitioned_links =
            response_json(authorized_get(app, "/api/v1/integrations/jira/task-links").await).await;
        assert_eq!(transitioned_links[0]["jira_status_id"], "4");
        assert_eq!(transitioned_links[0]["jira_status_name"], "In Review");
    }

    #[tokio::test]
    async fn jira_reconciliation_refreshes_only_work_already_in_the_hive() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let jira_server = axum::Router::new().route(
            "/rest/api/3/search/jql",
            get(
                |axum::extract::Query(query): axum::extract::Query<
                    std::collections::HashMap<String, String>,
                >| async move {
                    let jql = query.get("jql").map(String::as_str).unwrap_or_default();
                    assert!(jql.contains("id in (\"20001\")"));
                    assert!(!jql.contains("statusCategory != Done"));
                    Json(serde_json::json!({
                        "isLast": true,
                        "issues": [{
                            "id": "20001",
                            "key": "WEB-42",
                            "fields": {
                                "summary": "Closed remotely",
                                "status": { "id": "5", "name": "Done" },
                                "assignee": { "accountId": "account-2", "displayName": "Fern" },
                                "updated": "2026-08-13T14:00:00.000+0000"
                            }
                        }, {
                            "id": "20002",
                            "key": "WEB-43",
                            "fields": {
                                "summary": "Never implicitly import this issue",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": null,
                                "updated": "2026-08-13T14:01:00.000+0000"
                            }
                        }]
                    }))
                },
            ),
        );
        tokio::spawn(async move { axum::serve(listener, jira_server).await.unwrap() });

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
                &[
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                    JiraStatusMapping {
                        jira_status_id: "5".into(),
                        jira_status_name: "Done".into(),
                        task_state: TaskState::Completed,
                    },
                ],
            )
            .unwrap();
        store
            .set_jira_auto_sync_assigned(binding.id, false)
            .unwrap();
        store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Original title",
                    description: "Original Jira context",
                    status_id: "3",
                    status_name: "In Progress",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T13:00:00.000+0000",
                }],
            )
            .unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_jira_configuration(
                &format!("http://{address}"),
                "operator@example.test",
                "api-token",
            )
            .unwrap();

        state.reconcile_jira().await;

        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Closed remotely");
        assert_eq!(tasks[0].state, TaskState::Completed);
        let links = store.list_jira_issue_links(binding.id).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].jira_assignee_name.as_deref(), Some("Fern"));
        assert_eq!(links[0].jira_status_id, "5");
    }

    #[tokio::test]
    async fn enabled_jira_assigned_sync_imports_open_operator_work() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let jira_server = axum::Router::new().route(
            "/rest/api/3/search/jql",
            get(|| async {
                Json(serde_json::json!({
                    "isLast": true,
                    "issues": [{
                        "id": "20003",
                        "key": "WEB-44",
                        "fields": {
                            "summary": "Assigned operator work",
                            "status": { "id": "3", "name": "In Progress" },
                            "assignee": { "accountId": "account-1", "displayName": "Bea" },
                            "updated": "2026-08-13T15:00:00.000+0000"
                        }
                    }]
                }))
            }),
        );
        tokio::spawn(async move { axum::serve(listener, jira_server).await.unwrap() });

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
                    jira_status_id: "3".into(),
                    jira_status_name: "In Progress".into(),
                    task_state: TaskState::Active,
                }],
            )
            .unwrap();
        store.set_jira_auto_sync_assigned(binding.id, true).unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_jira_configuration(
                &format!("http://{address}"),
                "operator@example.test",
                "api-token",
            )
            .unwrap();

        state.reconcile_jira().await;

        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Assigned operator work");
        assert_eq!(tasks[0].state, TaskState::Active);
        assert_eq!(store.list_jira_issue_links(binding.id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn jira_unassigned_intake_fails_closed_when_remote_claim_is_denied() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let jira_server = axum::Router::new()
            .route(
                "/rest/api/3/myself",
                get(|| async {
                    Json(serde_json::json!({
                        "accountId": "account-1",
                        "displayName": "Bea"
                    }))
                }),
            )
            .route(
                "/rest/api/3/search/jql",
                get(|| async {
                    Json(serde_json::json!({
                        "isLast": true,
                        "issues": [{
                            "id": "20002",
                            "key": "WEB-43",
                            "fields": {
                                "summary": "Do not import without ownership",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": null,
                                "updated": "2026-08-13T13:01:00.000+0000"
                            }
                        }]
                    }))
                }),
            )
            .route(
                "/rest/api/3/issue/20002/assignee",
                axum::routing::put(|| async { StatusCode::FORBIDDEN }),
            );
        tokio::spawn(async move { axum::serve(listener, jira_server).await.unwrap() });

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
                    jira_status_id: "3".into(),
                    jira_status_name: "In Progress".into(),
                    task_state: TaskState::Active,
                }],
            )
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone())
                .with_jira_configuration(
                    &format!("http://{address}"),
                    "operator@example.test",
                    "api-token",
                )
                .unwrap(),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/integrations/jira/bindings/{}/sync",
                        binding.id
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"issue_ids":["20002"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(store.list_tasks().unwrap().is_empty());
        assert!(store.list_jira_issue_links(binding.id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn jira_outage_keeps_the_local_transition_and_queues_remote_delivery() {
        let unavailable = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = unavailable.local_addr().unwrap();
        drop(unavailable);
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
                &[
                    JiraStatusMapping {
                        jira_status_id: "1".into(),
                        jira_status_name: "To Do".into(),
                        task_state: TaskState::Ready,
                    },
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                ],
            )
            .unwrap();
        let task = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "",
                    status_id: "1",
                    status_name: "To Do",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T12:00:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone())
                .with_jira_configuration(
                    &format!("http://{address}"),
                    "operator@example.test",
                    "api-token",
                )
                .unwrap(),
        );

        let transitioned = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/tasks/{}/state", task.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"state":"active"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(transitioned.status(), StatusCode::OK);
        assert_eq!(response_json(transitioned).await["state"], "active");
        assert_eq!(store.get_task(task.id).unwrap().state, TaskState::Active);
        let links =
            response_json(authorized_get(app, "/api/v1/integrations/jira/task-links").await).await;
        assert_eq!(links[0]["jira_status_name"], "To Do");
        assert_eq!(links[0]["outbound_state"], "queued");
    }

    #[tokio::test]
    async fn database_backup_is_private_no_store_and_reopenable() {
        let store = TaskStore::in_memory().unwrap();
        let expected = store.local_hive_identity().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/backups/database")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/backups/database").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/vnd.sqlite3"
        );
        assert!(
            response.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .contains("attachment")
        );
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("restored.sqlite3");
        std::fs::write(&path, bytes).unwrap();
        let restored = TaskStore::open(path).unwrap();
        restored.verify_integrity().unwrap();
        assert_eq!(
            restored.local_hive_identity().unwrap().hive.id,
            expected.hive.id
        );
    }

    #[tokio::test]
    async fn dogfood_report_queue_keeps_reviewed_notes_and_a_private_screenshot() {
        let runtime = TempDir::new().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_attachment_store(runtime.path().join("attachments"))
                .with_task_store(TaskStore::in_memory().unwrap()),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/feedback/reports")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let attachment = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/feedback/attachments")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "image/png")
                    .body(Body::from(b"\x89PNG\r\n\x1a\nprivate-screen".as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(attachment.status(), StatusCode::CREATED);
        let attachment = response_json(attachment).await;
        let attachment_name = attachment["name"].as_str().unwrap();
        assert!(!attachment_name.contains("private"));

        let unreferenced = feedback_attachment_get(app.clone(), attachment_name, true).await;
        assert_eq!(unreferenced.status(), StatusCode::NOT_FOUND);

        let saved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/feedback/reports")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "expectation": "Terminal should redraw",
                            "observation": "It stayed blank",
                            "diagnostic_bundle": "operator-reviewed sanitized evidence",
                            "attachment_name": attachment_name,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::CREATED);

        let reports = authorized_get(app.clone(), "/api/v1/feedback/reports?limit=1").await;
        assert_eq!(reports.status(), StatusCode::OK);
        assert_eq!(reports.headers()[header::CACHE_CONTROL], "no-store");
        let reports = response_json(reports).await;
        assert_eq!(reports[0]["observation"], "It stayed blank");
        assert_eq!(reports[0]["attachment_name"], attachment_name);

        let unauthorized_attachment =
            feedback_attachment_get(app.clone(), attachment_name, false).await;
        assert_eq!(unauthorized_attachment.status(), StatusCode::UNAUTHORIZED);
        let downloaded = feedback_attachment_get(app.clone(), attachment_name, true).await;
        assert_eq!(downloaded.status(), StatusCode::OK);
        assert_eq!(downloaded.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(downloaded.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            downloaded.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert!(
            downloaded.headers()[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .contains(attachment_name)
        );
        let downloaded = axum::body::to_bytes(downloaded.into_body(), MAX_ATTACHMENT_BYTES)
            .await
            .unwrap();
        assert_eq!(downloaded.as_ref(), b"\x89PNG\r\n\x1a\nprivate-screen");
        assert_eq!(
            std::fs::read_dir(runtime.path().join("attachments"))
                .unwrap()
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn presence_routes_are_private_typed_and_do_not_churn_events() {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store.clone());
        let app = router(state);
        let device_id = PresenceDeviceId::new();

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/presence")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let observe = || {
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/presence/devices/{device_id}"))
                .header("authorization", "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"device_class":"desktop","state":"active"}"#))
                .unwrap()
        };
        let observed = app.clone().oneshot(observe()).await.unwrap();
        assert_eq!(observed.status(), StatusCode::OK);
        assert_eq!(observed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(observed).await["mode"], "at_hive");
        let cursor = store.list_control_room_events(0).unwrap().next_cursor;

        assert_eq!(
            app.clone().oneshot(observe()).await.unwrap().status(),
            StatusCode::OK
        );
        assert!(
            store
                .list_control_room_events(cursor)
                .unwrap()
                .events
                .is_empty()
        );

        let manual = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/presence")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"manual_mode":"night_watch"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response_json(manual).await["mode"], "night_watch");
        assert_eq!(
            response_json(authorized_get(app, "/api/v1/presence").await).await["source"],
            "manual"
        );
    }

    #[tokio::test]
    async fn queen_policy_route_is_private_and_persists_all_presence_tiers() {
        let store = TaskStore::in_memory().unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/orchestration/queen-policy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let updated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/orchestration/queen-policy")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"at_hive":"local_execution","away":"advisory","night_watch":"coordinate"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        assert_eq!(updated.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(updated).await["away"], "advisory");
        let fetched =
            response_json(authorized_get(app, "/api/v1/orchestration/queen-policy").await).await;
        assert_eq!(fetched["at_hive"], "local_execution");
        assert_eq!(fetched["night_watch"], "coordinate");
    }

    #[tokio::test]
    async fn presentation_preferences_are_private_device_scoped_and_persisted() {
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(TaskStore::in_memory().unwrap()),
        );
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/preferences/presentation/mobile")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let initial = response_json(
            authorized_get(app.clone(), "/api/v1/preferences/presentation/mobile").await,
        )
        .await;
        assert_eq!(initial["configured"], false);
        assert_eq!(initial["color_theme"], "light");

        let updated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/preferences/presentation/mobile")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"color_theme":"dark","terminal_keys_visible":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated = response_json(updated).await;
        assert_eq!(updated["configured"], true);
        assert_eq!(updated["terminal_keys_visible"], false);

        let desktop =
            response_json(authorized_get(app, "/api/v1/preferences/presentation/desktop").await)
                .await;
        assert_eq!(desktop["configured"], false);
        assert_eq!(desktop["color_theme"], "light");
    }

    #[tokio::test]
    async fn notification_routes_are_private_bounded_and_reject_arbitrary_endpoints() {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store)
            .with_notifications("mailto:test@swarm-next.local")
            .unwrap();
        let app = router(state);

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let settings = authorized_get(app.clone(), "/api/v1/notifications/settings").await;
        assert_eq!(settings.status(), StatusCode::OK);
        assert_eq!(settings.headers()[header::CACHE_CONTROL], "no-store");
        let settings = response_json(settings).await;
        assert_eq!(settings["policy"], "important_only");
        assert_eq!(settings["subscription_count"], 0);
        assert!(settings["vapid_public_key"].as_str().unwrap().len() > 40);

        let malicious = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/notifications/subscriptions/{}",
                        PresenceDeviceId::new()
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"device_class":"mobile","endpoint":"https://127.0.0.1/internal","keys":{"p256dh":"AA","auth":"AA"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malicious.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(malicious).await["code"],
            "invalid_push_endpoint"
        );
    }
    #[tokio::test]
    async fn control_room_event_feed_is_private_resumable_and_content_free() {
        let store = TaskStore::in_memory().unwrap();
        store
            .create_task("Sensitive title", "/private/workspace")
            .unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let app = router(state);

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/control-room/events?after=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/control-room/events?after=0").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json["events"][0]["kind"], "tasks_changed");
        assert!(json["next_cursor"].as_i64().unwrap() > 0);
        assert_eq!(json["reset_required"], false);
        let serialized = json.to_string();
        assert!(!serialized.contains("Sensitive title"));
        assert!(!serialized.contains("private/workspace"));
    }
    #[tokio::test]
    async fn packaged_router_serves_only_existing_browser_assets() {
        let web_root = TempDir::new().unwrap();
        std::fs::write(
            web_root.path().join("index.html"),
            "<!doctype html><title>Swarm Next</title>",
        )
        .unwrap();
        std::fs::write(web_root.path().join("app.js"), "export {};").unwrap();
        let app = router_with_web_root(AppState::default(), web_root.path());

        let index = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(index.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(index.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(index.headers()[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(index.headers()[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(
            index.headers()["content-security-policy"],
            "default-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; manifest-src 'self'"
        );
        assert_eq!(
            index.headers()["permissions-policy"],
            "camera=(), geolocation=(), microphone=(), payment=(), usb=()"
        );

        let asset = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);

        let missing_api = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/not-a-route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_api.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn packaged_router_retains_hashed_assets_for_open_tabs() {
        let web_root = TempDir::new().unwrap();
        let asset_root = TempDir::new().unwrap();
        std::fs::write(web_root.path().join("index.html"), "<!doctype html>").unwrap();
        std::fs::create_dir(web_root.path().join("assets")).unwrap();
        std::fs::write(
            web_root.path().join("assets/current.js"),
            "export const current = true;",
        )
        .unwrap();
        std::fs::write(
            asset_root.path().join("previous.js"),
            "export const previous = true;",
        )
        .unwrap();
        let app = router_with_asset_root(AppState::default(), web_root.path(), asset_root.path());

        let retained = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/previous.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retained.status(), StatusCode::OK);

        let current = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/current.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(current.status(), StatusCode::OK);

        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/assets/missing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn limits_are_observable() {
        let response = router(AppState::new(JournalLimits::new(2048, 64)))
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/limits")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = response_json(response).await;
        assert_eq!(json["terminal"]["journal_max_bytes"], 2048);
        assert_eq!(json["terminal"]["journal_max_frames"], 64);
        assert_eq!(json["terminal"]["canonical_scrollback_rows"], 1_000);
        assert_eq!(
            json["terminal"]["canonical_compaction_input_bytes"],
            CANONICAL_COMPACTION_INPUT_BYTES
        );
        assert_eq!(
            json["terminal"]["canonical_snapshot_max_bytes"],
            MAX_CANONICAL_SNAPSHOT_BYTES
        );
        assert_eq!(json["terminal"]["max_rows"], MAX_TERMINAL_ROWS);
        assert_eq!(json["terminal"]["max_columns"], MAX_TERMINAL_COLUMNS);
        assert_eq!(json["terminal"]["max_cells"], MAX_TERMINAL_CELLS);
    }

    #[tokio::test]
    async fn terminal_routes_fail_closed_without_auth() {
        let response = router(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/terminal/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn terminal_routes_reject_an_invalid_operator_token() {
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret");
        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/terminal/sessions")
                    .header("authorization", "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn worker_preferences_update_without_changing_repository_or_conversation() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                "/workspace/daisy",
                false,
                1,
            )
            .unwrap();
        let conversation = worker.provider_conversation_id;
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/workers/{}", worker.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"Clover","autostart":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response = response_json(response).await;
        assert_eq!(response["name"], "Clover");
        assert_eq!(response["autostart"], true);
        assert_eq!(response["workspace"], worker.workspace);
        assert_eq!(
            store
                .get_worker_profile(worker.id)
                .unwrap()
                .provider_conversation_id,
            conversation
        );
    }

    #[tokio::test]
    async fn task_routes_persist_the_minimal_lifecycle() {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let app = router(state);

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tasks")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"title":"Recover exact terminal","description":"Survive reloads","priority":"high","workspace":"/workspace"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = response_json(created).await;
        assert_eq!(created["state"], "draft");
        assert_eq!(created["description"], "Survive reloads");
        assert_eq!(created["priority"], "high");

        let updated = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/tasks/{}", created["id"].as_str().unwrap()))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"title":"Recover every terminal","priority":"urgent"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated = response_json(updated).await;
        assert_eq!(updated["title"], "Recover every terminal");
        assert_eq!(updated["description"], "Survive reloads");
        assert_eq!(updated["priority"], "urgent");

        let ready = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!(
                        "/api/v1/tasks/{}/state",
                        created["id"].as_str().unwrap()
                    ))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"state":"ready"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ready.status(), StatusCode::OK);
        assert_eq!(response_json(ready).await["state"], "ready");

        let activity = authorized_get(
            app.clone(),
            &format!(
                "/api/v1/tasks/{}/activity?limit=10",
                created["id"].as_str().unwrap()
            ),
        )
        .await;
        assert_eq!(activity.status(), StatusCode::OK);
        assert_eq!(
            activity.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let activity = response_json(activity).await;
        assert!(!activity["truncated"].as_bool().unwrap());
        assert_eq!(activity["events"].as_array().unwrap().len(), 3);
        assert_eq!(activity["events"][0]["kind"], "created");
        assert_eq!(activity["events"][2]["from_state"], "draft");
        assert_eq!(activity["events"][2]["to_state"], "ready");

        let listed = authorized_get(app, "/api/v1/tasks").await;
        let listed = response_json(listed).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["title"], "Recover every terminal");
    }

    #[tokio::test]
    async fn decision_routes_are_operator_only_and_resolve_allowed_actions() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let actions = vec!["ship".to_string(), "hold".to_string()];
        let decision = store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: None,
                kind: swarm_domain::DecisionRequestKind::Approval,
                urgency: swarm_domain::DecisionUrgency::TimeSensitive,
                title: "Approve the release",
                reason: "The candidate is ready",
                risk: "Users wait if held",
                evidence: "All checks pass",
                suggested_action: "Ship",
                allowed_actions: &actions,
                deadline: None,
            })
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/decisions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let listed = response_json(authorized_get(app.clone(), "/api/v1/decisions").await).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["state"], "pending");

        let resolved = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/decisions/{}/resolution", decision.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"action":"ship","note":"Proceed"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resolved.status(), StatusCode::OK);
        let resolved = response_json(resolved).await;
        assert_eq!(resolved["state"], "resolved");
        assert_eq!(resolved["resolution_action"], "ship");
        assert_eq!(resolved["resolution_note"], "Proceed");
        assert!(resolved["resolved_by_operator_id"].is_string());
    }

    #[tokio::test]
    async fn resolving_a_decision_delivers_to_the_requesting_worker_terminal() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(16_384, 256), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-lc".into(),
                "while IFS= read -r line; do printf 'received:%s\n' \"$line\"; done".into(),
            ],
            working_directory: workspace.clone(),
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
        let queen = store
            .ensure_queen(workspace.to_string_lossy().as_ref())
            .unwrap();
        store.bind_worker_session(queen.id, session.id()).unwrap();
        let actions = vec!["ship".to_string(), "hold".to_string()];
        let decision = store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: None,
                kind: swarm_domain::DecisionRequestKind::Approval,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Approve the release",
                reason: "The candidate is ready",
                risk: "Users wait if held",
                evidence: "All checks pass",
                suggested_action: "Ship",
                allowed_actions: &actions,
                deadline: None,
            })
            .unwrap();
        let client = HostClient::new(&socket);
        let app = router(
            AppState::default()
                .with_terminal_host(client.clone(), "secret")
                .with_task_store(store),
        );

        let resolved = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/decisions/{}/resolution", decision.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"action":"ship","note":"Proceed after green checks"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resolved.status(), StatusCode::OK);
        assert_eq!(response_json(resolved).await["delivery_state"], "delivered");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut after_sequence = None;
        let mut output = Vec::new();
        loop {
            let response = client
                .request(&HostRequest::Read {
                    session_id: session.id(),
                    after_sequence,
                })
                .await
                .unwrap();
            let HostResponse::Output { resume, .. } = response else {
                panic!("terminal host should return worker output");
            };
            match resume {
                swarm_terminal::Resume::Deltas { frames } => {
                    for frame in frames {
                        after_sequence = Some(frame.sequence);
                        output.extend_from_slice(&frame.bytes);
                    }
                }
                swarm_terminal::Resume::Snapshot { snapshot } => {
                    after_sequence = Some(snapshot.sequence);
                    output = snapshot.bytes;
                }
            }
            let rendered = String::from_utf8_lossy(&output);
            if rendered.contains("[Swarm decision") && rendered.contains("Action: ship") {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        session.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }
    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn worker_review_handoff_reaches_the_quiet_queen_terminal() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-lc".into(),
                "read value; printf 'received:%s' \"$value\"; sleep 5".into(),
            ],
            working_directory: workspace.clone(),
        };
        let queen_terminal = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());

        let store = TaskStore::in_memory().unwrap();
        let queen = store
            .ensure_queen(workspace.to_string_lossy().as_ref())
            .unwrap();
        store
            .bind_worker_session(queen.id, queen_terminal.id())
            .unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let worker_session = WorkerSessionId::new();
        store
            .bind_worker_session(worker.id, worker_session)
            .unwrap();
        let task = store
            .create_task("Ship mobile controls", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, worker_session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .transition_worker_task(
                task.id,
                TaskState::Review,
                "Android voice and shortcuts verified.",
                worker_session,
            )
            .unwrap();

        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store.clone());
        state.deliver_coordination().await;
        assert_eq!(
            store.get_task(task.id).unwrap().outcome_delivery_state,
            Some(swarm_domain::TaskOutcomeDeliveryState::Delivered)
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut after_sequence = None;
        let mut output = Vec::new();
        loop {
            let response = HostClient::new(&socket)
                .request(&HostRequest::Read {
                    session_id: queen_terminal.id(),
                    after_sequence,
                })
                .await
                .unwrap();
            let HostResponse::Output { resume, .. } = response else {
                panic!("terminal host should return Queen output");
            };
            match resume {
                swarm_terminal::Resume::Deltas { frames } => {
                    for frame in frames {
                        after_sequence = Some(frame.sequence);
                        output.extend_from_slice(&frame.bytes);
                    }
                }
                swarm_terminal::Resume::Snapshot { snapshot } => {
                    after_sequence = Some(snapshot.sequence);
                    output = snapshot.bytes;
                }
            }
            let rendered = String::from_utf8_lossy(&output);
            if rendered.contains("[Swarm worker outcome]")
                && rendered.contains("Android voice and shortcuts verified")
            {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        queen_terminal.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }
    #[tokio::test]
    async fn task_activity_rejects_invalid_limits() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Bound history", "/workspace").unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let response = authorized_get(
            router(state),
            &format!("/api/v1/tasks/{}/activity?limit=101", task.id),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await["code"],
            "invalid_task_activity_limit"
        );
    }

    #[tokio::test]
    async fn task_order_requires_the_complete_open_set() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let app = router(state);
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tasks/order")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"task_ids":["{}"]}}"#, first.id)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        assert_eq!(response_json(rejected).await["code"], "task_order_conflict");

        let reordered = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tasks/order")
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"task_ids":["{}","{}"]}}"#,
                        second.id, first.id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reordered.status(), StatusCode::OK);
        let reordered = response_json(reordered).await;
        assert_eq!(reordered[0]["id"], second.id.to_string());
        assert_eq!(reordered[0]["position"], 0);
        assert_eq!(reordered[1]["id"], first.id.to_string());
        assert_eq!(reordered[1]["position"], 1);
    }

    #[tokio::test]
    async fn task_routes_reject_invalid_transitions() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Stateful work", "/workspace").unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(store);
        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/tasks/{}/state", task.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"state":"completed"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(response).await["code"],
            "task_transition_rejected"
        );
    }

    #[tokio::test]
    async fn history_diagnostics_are_authorized_and_content_free() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let history = Arc::new(
            HistoryStore::open(runtime.path().join("history"), HistoryLimits::default()).unwrap(),
        );
        let registry = Arc::new(
            SessionRegistry::new_with_history(
                JournalLimits::new(4096, 64),
                1,
                [workspace],
                Some(history),
            )
            .unwrap(),
        );
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let app =
            router(AppState::default().with_terminal_host(HostClient::new(&socket), "secret"));

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/terminal/history/diagnostics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app.clone(), "/api/v1/terminal/history/diagnostics").await;
        assert!(response.status().is_success());
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(
            json["diagnostics"]["limits"]["max_total_bytes"],
            HistoryLimits::default().max_total_bytes
        );
        assert_eq!(json["diagnostics"]["retained_bytes"], 0);
        assert!(json.get("bytes").is_none());
        let status_response = authorized_get(app.clone(), "/api/v1/runtime/terminal-host").await;
        assert_eq!(status_response.headers()[header::CACHE_CONTROL], "no-store");
        let status = response_json(status_response).await;
        assert_eq!(status["status"]["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(status["status"]["draining"], false);
        assert_eq!(status["status"]["running_sessions"], 0);

        let resources = authorized_get(app.clone(), "/api/v1/runtime/resources").await;
        assert_eq!(resources.status(), StatusCode::OK);
        assert_eq!(resources.headers()[header::CACHE_CONTROL], "no-store");
        let resources = response_json(resources).await;
        assert_eq!(resources["policy"]["mode"], "observe_only");
        assert_eq!(
            resources["policy"]["advisory_bytes"],
            RESOURCE_ADVISORY_BYTES
        );
        assert_eq!(
            resources["policy"]["critical_bytes"],
            RESOURCE_CRITICAL_BYTES
        );
        assert_eq!(resources["api"]["pressure"], "normal");
        assert_eq!(resources["terminal_host"]["pressure"], "normal");
        assert!(resources["api"]["resident_memory_bytes"].as_u64().is_some());
        assert!(
            resources["terminal_host"]["resident_memory_bytes"]
                .as_u64()
                .is_some()
        );

        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn managed_maintenance_stops_sessions_updates_the_host_and_cleans_its_request() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let session = registry
            .spawn(
                &ProviderCommand {
                    executable: PathBuf::from("/bin/sh"),
                    arguments: vec!["-lc".into(), "sleep 10".into()],
                    working_directory: workspace,
                },
                TerminalSize::default(),
            )
            .unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server =
            HostServer::bind_with_version(&socket, Arc::clone(&registry), "old-host").unwrap();
        let old_server_task = tokio::spawn(server.run());
        let maintenance_request = runtime.path().join("worker-engine-maintenance.request");
        let watched_request = maintenance_request.clone();
        let replacement_socket = socket.clone();
        let replacement_registry = Arc::clone(&registry);
        let (replacement_sender, replacement_receiver) = tokio::sync::oneshot::channel();
        let watcher = tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
            while !watched_request.exists() {
                assert!(tokio::time::Instant::now() < deadline);
                sleep(Duration::from_millis(10)).await;
            }
            old_server_task.abort();
            let _ = old_server_task.await;
            let replacement = HostServer::bind_with_version(
                replacement_socket,
                replacement_registry,
                build_version(),
            )
            .unwrap();
            let replacement_task = tokio::spawn(replacement.run());
            let _ = replacement_sender.send(replacement_task);
        });
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store.clone())
            .with_maintenance_request_path(maintenance_request.clone())
            .with_maintenance_timeout(Duration::from_secs(3));

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/terminal-host/maintenance")
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = response_json(response).await;
        assert_eq!(response["previous_version"], "old-host");
        assert_eq!(response["current_version"], build_version());
        assert_eq!(response["stopped_sessions"], 1);
        assert!(!maintenance_request.exists());
        assert!(!session.is_running().unwrap());
        assert!(
            store
                .list_control_room_events(0)
                .unwrap()
                .events
                .iter()
                .any(|event| event.kind == ControlRoomEventKind::RuntimeChanged)
        );

        watcher.await.unwrap();
        let replacement_task = replacement_receiver.await.unwrap();
        replacement_task.abort();
        let _ = replacement_task.await;
    }

    #[tokio::test]
    async fn runtime_resources_fail_closed_without_operator_authentication() {
        let response = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret"),
        )
        .oneshot(
            Request::builder()
                .uri("/api/v1/runtime/resources")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn durable_history_is_listed_and_paged_after_store_reopen() {
        let runtime = TempDir::new().unwrap();
        let history_root = runtime.path().join("history");
        let id = WorkerSessionId::new();
        {
            let history = HistoryStore::open(&history_root, HistoryLimits::default()).unwrap();
            history.start_session(id).unwrap();
            history
                .append_checkpoint(
                    id,
                    &TerminalSnapshot {
                        sequence: 0,
                        rows: 24,
                        columns: 80,
                        truncated: false,
                        bytes: Vec::new(),
                    },
                )
                .unwrap();
            history.append(id, 1, b"survived-restart").unwrap();
            history.finish_session(id).unwrap();
        }
        let history =
            Arc::new(HistoryStore::open(&history_root, HistoryLimits::default()).unwrap());
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new_with_history(
                JournalLimits::new(4096, 64),
                1,
                [workspace],
                Some(history),
            )
            .unwrap(),
        );
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let app =
            router(AppState::default().with_terminal_host(HostClient::new(&socket), "secret"));

        let sessions_response =
            authorized_get(app.clone(), "/api/v1/terminal/history/sessions").await;
        assert_eq!(
            sessions_response.headers()[header::CACHE_CONTROL],
            "no-store"
        );
        let sessions = response_json(sessions_response).await;
        assert_eq!(sessions["sessions"][0]["session_id"], id.to_string());
        assert_eq!(sessions["sessions"][0]["active"], false);

        let page_response =
            authorized_get(app, &format!("/api/v1/terminal/history/sessions/{id}")).await;
        assert_eq!(page_response.headers()[header::CACHE_CONTROL], "no-store");
        let page = response_json(page_response).await;
        assert_eq!(page["page"]["session_id"], id.to_string());
        let output = page["page"]["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|record| record["kind"] == "output")
            .flat_map(|record| record["bytes"].as_array().into_iter().flatten())
            .filter_map(Value::as_u64)
            .filter_map(|byte| u8::try_from(byte).ok())
            .collect::<Vec<_>>();
        assert_eq!(output, b"survived-restart");
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn api_recreation_reattaches_the_durable_queen_without_a_duplicate() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "sleep 5".into()],
            working_directory: workspace.clone(),
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
        let queen = store
            .ensure_queen(workspace.to_string_lossy().as_ref())
            .unwrap();
        store.bind_worker_session(queen.id, session.id()).unwrap();

        let first_state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store.clone());
        first_state.supervise_workers().await;
        let first_workers = authorized_get(router(first_state), "/api/v1/workers").await;
        assert_eq!(first_workers.status(), StatusCode::OK);
        assert_eq!(first_workers.headers()[header::CACHE_CONTROL], "no-store");
        let first_workers = response_json(first_workers).await;
        assert_eq!(first_workers[0]["name"], "Queen");
        assert_eq!(first_workers[0]["role"], "queen");
        assert_eq!(first_workers[0]["running"], true);
        assert_eq!(
            first_workers[0]["active_session_id"],
            session.id().to_string()
        );

        let replacement_state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store);
        replacement_state.supervise_workers().await;
        let response = HostClient::new(&socket)
            .request(&HostRequest::ListSessions)
            .await
            .unwrap();
        let HostResponse::Sessions { sessions } = response else {
            panic!("terminal host should return its sessions");
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session.id());
        assert!(sessions[0].running);

        let replacement_workers =
            authorized_get(router(replacement_state), "/api/v1/workers").await;
        let replacement_workers = response_json(replacement_workers).await;
        assert_eq!(
            replacement_workers[0]["active_session_id"],
            session.id().to_string()
        );

        session.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn repeated_queen_recovery_opens_a_visible_circuit_instead_of_respawning() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry.clone()).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
        let queen = store
            .ensure_queen(workspace.to_string_lossy().as_ref())
            .unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store);
        state
            .worker_recovery_attempts
            .write()
            .await
            .insert(queen.id, unix_timestamp());

        state.supervise_workers().await;
        state.supervise_workers().await;

        let response = HostClient::new(&socket)
            .request(&HostRequest::ListSessions)
            .await
            .unwrap();
        let HostResponse::Sessions { sessions } = response else {
            panic!("terminal host should return its sessions");
        };
        assert!(sessions.is_empty());
        let workers = response_json(authorized_get(router(state), "/api/v1/workers").await).await;
        assert_eq!(workers[0]["attention_state"], "blocked");
        assert_eq!(
            workers[0]["runtime_error"],
            "Worker exited again before recovery was stable. Retry when ready."
        );

        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn a_stable_queen_resets_the_recovery_circuit() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let session = registry
            .spawn(
                &ProviderCommand {
                    executable: PathBuf::from("/bin/sh"),
                    arguments: vec!["-lc".into(), "sleep 5".into()],
                    working_directory: workspace.clone(),
                },
                TerminalSize::default(),
            )
            .unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
        let queen = store
            .ensure_queen(workspace.to_string_lossy().as_ref())
            .unwrap();
        store.bind_worker_session(queen.id, session.id()).unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store);
        state.worker_recovery_attempts.write().await.insert(
            queen.id,
            unix_timestamp() - WORKER_RECOVERY_STABILITY_SECONDS,
        );

        state.supervise_workers().await;

        assert!(state.worker_recovery_attempts.read().await.is_empty());
        session.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn api_recreation_preserves_host_owned_session() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "printf durable; sleep 5".into()],
            working_directory: workspace.clone(),
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let state = AppState::default().with_terminal_host(HostClient::new(&socket), "secret");

        let first_api = router(state.clone());
        let first = authorized_get(first_api, "/api/v1/terminal/sessions").await;
        assert!(first.status().is_success());
        drop(first);

        let replacement_api = router(state);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let response = authorized_get(
                replacement_api.clone(),
                &format!("/api/v1/terminal/sessions/{}/output?after=0", session.id()),
            )
            .await;
            let json = response_json(response).await;
            let frames = json["resume"]["frames"]
                .as_array()
                .expect("delta response should contain frames");
            let bytes = frames
                .iter()
                .flat_map(|frame| frame["bytes"].as_array().into_iter().flatten())
                .filter_map(Value::as_u64)
                .filter_map(|byte| u8::try_from(byte).ok())
                .collect::<Vec<_>>();
            if String::from_utf8_lossy(&bytes).contains("durable") {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        session.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn active_session_list_excludes_a_completed_provider_session() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "exit 0".into()],
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while session.is_running().unwrap() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let state = AppState::default().with_terminal_host(HostClient::new(&socket), "secret");

        let response = authorized_get(router(state), "/api/v1/terminal/sessions").await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = response_json(response).await;
        assert_eq!(response["sessions"], serde_json::json!([]));

        server_task.abort();
        let _ = server_task.await;
    }

    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn task_assignment_accepts_a_sleeping_durable_worker() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Sleeping Clover",
                ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task("Wait for Clover", "/workspace/clover")
            .unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/missing/terminal.sock"), "secret")
            .with_task_store(store);

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{}/assignment", task.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"worker_id":"{}"}}"#, worker.id)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let assigned = response_json(response).await;
        assert_eq!(assigned["assigned_worker_id"], worker.id.to_string());
        assert!(assigned["assigned_session_id"].is_null());
        assert!(assigned["dispatch_state"].is_null());
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn task_assignment_binds_a_running_worker_and_retains_durable_ownership() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-lc".into(),
                "read value; printf 'received:%s' \"$value\"; sleep 5".into(),
            ],
            working_directory: workspace.clone(),
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                workspace.to_string_lossy().as_ref(),
                false,
                1,
            )
            .unwrap();
        store.bind_worker_session(worker.id, session.id()).unwrap();
        let task = store
            .create_task("Assigned through API", "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store.clone());
        let app = router(state);

        let assigned = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tasks/{}/assignment", task.id))
                    .header("authorization", "Bearer secret")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"worker_id":"{}"}}"#, worker.id)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(assigned.status(), StatusCode::OK);
        assert_eq!(
            response_json(assigned).await["assigned_session_id"],
            session.id().to_string()
        );
        assert_eq!(
            store.get_task(task.id).unwrap().dispatch_state,
            Some(swarm_domain::TaskDispatchState::Delivered)
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut after_sequence = None;
        let mut output = Vec::new();
        loop {
            let response = HostClient::new(&socket)
                .request(&HostRequest::Read {
                    session_id: session.id(),
                    after_sequence,
                })
                .await
                .unwrap();
            let HostResponse::Output { resume, .. } = response else {
                panic!("terminal host should return assigned worker output");
            };
            match resume {
                swarm_terminal::Resume::Deltas { frames } => {
                    for frame in frames {
                        after_sequence = Some(frame.sequence);
                        output.extend_from_slice(&frame.bytes);
                    }
                }
                swarm_terminal::Resume::Snapshot { snapshot } => {
                    after_sequence = Some(snapshot.sequence);
                    output = snapshot.bytes;
                }
            }
            if String::from_utf8_lossy(&output).contains("[Swarm task") {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let stopped = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/terminal/sessions/{}", session.id()))
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stopped.status(), StatusCode::OK);
        let stopped_task = store.get_task(task.id).unwrap();
        assert_eq!(stopped_task.assigned_worker_id, Some(worker.id));
        assert_eq!(stopped_task.assigned_session_id, None);

        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn websocket_replays_and_controls_a_host_owned_terminal() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-lc".into(),
                "printf socket-ready; read value; printf 'socket:%s' \"$value\"".into(),
            ],
            working_directory: workspace.clone(),
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let host_server = HostServer::bind(&socket, registry).unwrap();
        let host_task = tokio::spawn(host_server.run());
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Socket worker",
                ProviderKind::ClaudeCode,
                workspace.to_string_lossy().as_ref(),
                false,
                1,
            )
            .unwrap();
        store.bind_worker_session(worker.id, session.id()).unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new(&socket), "secret")
            .with_task_store(store.clone());
        let app = router(state);

        let grant = issue_terminal_grant(&app, session.id()).await;
        let second_grant = issue_terminal_grant(&app, session.id()).await;
        let resume_grant = issue_terminal_grant(&app, session.id()).await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let release_app = app.clone();
        let api_task = tokio::spawn(async move { axum::serve(listener, app).await });
        let websocket_url = format!(
            "ws://{address}/api/v1/terminal/sessions/{}/attach",
            session.id()
        );
        let mut websocket = connect_terminal(&websocket_url, &grant, 30, 100, None).await;

        let (initial, initial_dimensions, initial_sequence) =
            terminal_output_until(&mut websocket, "socket-ready").await;
        assert_eq!(initial_dimensions, Some((30, 100)));
        assert!(String::from_utf8_lossy(&initial).contains("socket-ready"));

        let mut resumed_websocket = connect_terminal(
            &websocket_url,
            &resume_grant,
            30,
            100,
            Some(initial_sequence),
        )
        .await;
        let resumed_state = tokio::time::timeout(Duration::from_secs(1), resumed_websocket.next())
            .await
            .expect("covered resume should be acknowledged without new output")
            .expect("covered resume WebSocket closed")
            .unwrap();
        let ClientMessage::Text(resumed_state) = resumed_state else {
            panic!("covered resume should receive an immediate state acknowledgement");
        };
        let resumed_state: Value = serde_json::from_str(&resumed_state).unwrap();
        assert_eq!(resumed_state["type"], "state");
        assert_eq!(resumed_state["running"], true);
        assert_eq!(resumed_state["latest_sequence"], initial_sequence);

        let mut second_websocket =
            connect_terminal(&websocket_url, &second_grant, 30, 100, None).await;
        let (_, second_initial_dimensions, _) =
            terminal_output_until(&mut second_websocket, "socket-ready").await;
        assert_eq!(second_initial_dimensions, Some((30, 100)));

        websocket
            .send(ClientMessage::Text(
                r#"{"type":"resize","rows":35,"columns":110}"#.into(),
            ))
            .await
            .unwrap();
        let (_, first_resized_dimensions, _) =
            terminal_output_until(&mut websocket, "socket-ready").await;
        let (_, second_resized_dimensions, _) =
            terminal_output_until(&mut second_websocket, "socket-ready").await;
        assert_eq!(first_resized_dimensions, Some((35, 110)));
        assert_eq!(second_resized_dimensions, Some((35, 110)));

        websocket
            .send(ClientMessage::Text(
                r#"{"type":"input","text":"hello\n"}"#.into(),
            ))
            .await
            .unwrap();
        let (after_input, _, _) = terminal_output_until(&mut websocket, "socket:hello").await;
        assert!(String::from_utf8_lossy(&after_input).contains("socket:hello"));
        assert!(!store.worker_accepts_injection(worker.id, i64::MIN).unwrap());

        let released = release_app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/terminal/sessions/{}/engagements/019fedfc-1c30-70e1-a5e2-9a3c94268093",
                        session.id()
                    ))
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(released.status(), StatusCode::NO_CONTENT);
        assert!(store.worker_accepts_injection(worker.id, i64::MIN).unwrap());

        let mut reused_request = websocket_url.into_client_request().unwrap();
        reused_request.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            format!("{TERMINAL_WEBSOCKET_PROTOCOL}, {TERMINAL_GRANT_PROTOCOL_PREFIX}{grant}")
                .parse()
                .unwrap(),
        );
        let reused = connect_async(reused_request).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = reused else {
            panic!("expected HTTP rejection for reused grant");
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        session.stop().unwrap();
        api_task.abort();
        let _ = api_task.await;
        host_task.abort();
        let _ = host_task.await;
    }

    async fn issue_terminal_grant(app: &Router, session_id: WorkerSessionId) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/terminal/sessions/{session_id}/attach-grants"
                    ))
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        response_json(response).await["grant"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn connect_terminal(
        websocket_url: &str,
        grant: &str,
        rows: u16,
        columns: u16,
        after_sequence: Option<u64>,
    ) -> WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>> {
        let mut request = websocket_url.into_client_request().unwrap();
        request.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            format!("{TERMINAL_WEBSOCKET_PROTOCOL}, {TERMINAL_GRANT_PROTOCOL_PREFIX}{grant}")
                .parse()
                .unwrap(),
        );
        let (mut websocket, response) = connect_async(request).await.unwrap();
        assert_eq!(
            response.headers()[header::SEC_WEBSOCKET_PROTOCOL],
            TERMINAL_WEBSOCKET_PROTOCOL
        );
        websocket
            .send(ClientMessage::Text(
                format!(
                    r#"{{"type":"resume","after_sequence":{},"rows":{rows},"columns":{columns},"device_id":"019fedfc-1c30-70e1-a5e2-9a3c94268093"}}"#,
                    after_sequence
                        .map_or_else(|| "null".to_owned(), |sequence| sequence.to_string())
                )
                .into(),
            ))
            .await
            .unwrap();
        websocket
    }

    async fn terminal_output_until<S>(
        websocket: &mut S,
        expected: &str,
    ) -> (Vec<u8>, Option<(u16, u16)>, u64)
    where
        S: futures_util::Stream<
                Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>,
            > + Unpin,
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        let mut snapshot_dimensions = None;
        loop {
            let message = tokio::time::timeout_at(deadline, websocket.next())
                .await
                .expect("timed out waiting for terminal WebSocket output")
                .expect("terminal WebSocket closed")
                .unwrap();
            if let ClientMessage::Binary(payload) = message {
                let latest_sequence = u64::from_be_bytes(payload[1..9].try_into().unwrap());
                match payload[0] {
                    1 => output.extend_from_slice(&payload[9..]),
                    2 => {
                        assert!(payload.len() >= 14);
                        snapshot_dimensions = Some((
                            u16::from_be_bytes(payload[9..11].try_into().unwrap()),
                            u16::from_be_bytes(payload[11..13].try_into().unwrap()),
                        ));
                        output.clear();
                        output.extend_from_slice(&payload[14..]);
                    }
                    frame_type => panic!("unexpected terminal frame type {frame_type}"),
                }
                if String::from_utf8_lossy(&output).contains(expected) {
                    return (output, snapshot_dimensions, latest_sequence);
                }
            }
        }
    }

    async fn authorized_get(app: Router, uri: &str) -> Response {
        app.oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    async fn feedback_attachment_get(app: Router, name: &str, authorized: bool) -> Response {
        let mut request = Request::builder().uri(format!("/api/v1/feedback/attachments/{name}"));
        if authorized {
            request = request.header("authorization", "Bearer secret");
        }
        app.oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn workspace_catalog_is_bounded_friendly_and_marks_owned_repositories() {
        let root = TempDir::new().unwrap();
        let daisy = root.path().join("daisy-repo");
        let open = root.path().join("open-repo");
        std::fs::create_dir_all(daisy.join(".git")).unwrap();
        std::fs::create_dir_all(&open).unwrap();
        std::fs::create_dir_all(root.path().join(".hidden")).unwrap();
        std::fs::create_dir_all(root.path().join("queen")).unwrap();
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                ProviderKind::ClaudeCode,
                daisy.to_string_lossy().as_ref(),
                false,
                1,
            )
            .unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_workspace_roots(vec![root.path().to_path_buf()]);

        let catalog = workspace_catalog(&state, &store.list_worker_profiles().unwrap())
            .await
            .unwrap();

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].name, "daisy-repo");
        assert_eq!(catalog[0].kind, "repository");
        assert_eq!(catalog[0].configured_worker_id, Some(worker.id));
        assert_eq!(catalog[1].name, "open-repo");
        assert_eq!(catalog[1].configured_worker_id, None);
    }

    #[tokio::test]
    async fn workspace_catalog_descends_through_folders_but_stops_at_repositories() {
        let root = TempDir::new().unwrap();
        let group = root.path().join("personal");
        let repository = group.join("swarm-next");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::create_dir_all(repository.join("node_modules").join("not-a-workspace")).unwrap();
        let state = AppState::default().with_workspace_roots(vec![root.path().to_path_buf()]);

        let catalog = workspace_catalog(&state, &[]).await.unwrap();

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].path, group.to_string_lossy());
        assert_eq!(catalog[0].kind, "folder");
        assert_eq!(catalog[1].path, repository.to_string_lossy());
        assert_eq!(catalog[1].kind, "repository");
    }

    #[tokio::test]
    async fn typed_workspace_paths_require_explicit_approval_outside_roots() {
        let root = TempDir::new().unwrap();
        let nested = root.path().join("personal").join("swarm-next");
        std::fs::create_dir_all(&nested).unwrap();
        let outside = TempDir::new().unwrap();
        let state = AppState::default().with_workspace_roots(vec![root.path().to_path_buf()]);

        assert_eq!(
            resolve_workspace_path(&state, nested.to_string_lossy().as_ref(), false)
                .await
                .unwrap(),
            nested.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_workspace_path(&state, outside.path().to_string_lossy().as_ref(), false)
                .await
                .unwrap_err()
                .code,
            "unknown_workspace"
        );
        assert_eq!(
            resolve_workspace_path(&state, outside.path().to_string_lossy().as_ref(), true)
                .await
                .unwrap(),
            outside.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn terminal_attachment_upload_never_bypasses_operator_authentication() {
        let attachments = TempDir::new().unwrap();
        let response = router(
            AppState::default()
                .with_terminal_host(
                    HostClient::new(attachments.path().join("absent.sock")),
                    "secret",
                )
                .with_attachment_store(attachments.path().to_path_buf()),
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/terminal/sessions/{}/attachments",
                    WorkerSessionId::new()
                ))
                .header(header::CONTENT_TYPE, "image/png")
                .body(Body::from(b"\x89PNG\r\n\x1a\nprivate".as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn browser_session_survives_without_exposing_the_operator_token_to_javascript() {
        let runtime = TempDir::new().unwrap();
        let state = AppState::default().with_terminal_host(
            HostClient::new(runtime.path().join("absent.sock")),
            "durable-secret",
        );
        let app = router(state);
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/session")
                    .header(header::AUTHORIZATION, "Bearer durable-secret")
                    .header("x-forwarded-proto", "https")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::NO_CONTENT);
        let set_cookie = created.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert!(set_cookie.contains("Max-Age=2592000"));
        assert!(set_cookie.contains("Secure"));
        assert!(!set_cookie.contains("durable-secret"));
        let cookie = set_cookie.split(';').next().unwrap();

        let restored = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/session")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(restored.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn locking_clears_the_browser_session_without_requiring_a_live_session() {
        let response = router(AppState::default())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/auth/session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
    }

    async fn response_json(response: Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
