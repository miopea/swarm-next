mod agent;
mod attach;
mod attachments;
mod auth;
mod backups;
mod control_room;
mod decisions;
mod email_attachments;
pub mod federation_http;
mod feedback;
mod jira;
mod jira_oauth;
mod maintenance;
mod microsoft_oauth;
mod notifications;
mod orchestration;
mod outlook;
mod presence;
mod presentation;
mod provider_activity;
mod runtime;
mod session_history;
mod tasks;
mod terminal_attach;
mod terminal_control;
mod terminal_host;
mod terminal_socket;
mod worker_runtime;
mod workers;

#[cfg(test)]
use runtime::{
    ResourcePressure, deployed_source_revision, development_source_status_for, git_output,
    resource_response,
};
#[cfg(test)]
use std::process::Command;
#[cfg(test)]
use workers::{resolve_workspace_path, workspace_catalog};

use std::{
    collections::{HashMap, HashSet},
    path::{Path as FilePath, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, patch, post, put},
};
use serde::{Deserialize, Serialize};
use swarm_application::{
    ApiaryHiveCandidateOverview, ApiaryInvitationOverview, ApiaryService, ApplicationError,
    FederationJoinInvitationOverview, TaskService,
};
use swarm_domain::{
    Apiary, ApiaryInvitation, ApiaryInvitationBundle, ApiaryInvitationId, ApiaryJoinLink,
    ApiaryJoinLinkId, ApiaryJoinReadiness, ApiaryKeeperLink, ApiaryTask, ApiaryTaskId,
    DecisionRequestId, FederationCatalogSnapshot, FederationClaimId, FederationJoinSubmission,
    FederationSharedClaim, FederationSyncCondition, FederationTaskCommand,
    FederationTaskOutboxEntry, FederationTaskOutboxStatus, FederationTaskPage,
    FederationTaskSyncStatus, HiveConnectionCard, HiveId, HiveIdentity, JiraConnectionState,
    JiraProjectBindingId, JiraProjectScope, JiraStatusMapping, LocalApiaryContext, OperatorId,
    ProviderKind, SharedWorkBackend, StewardCapability, Stewardship, StewardshipId, TaskId,
    TaskPriority, TaskState, WorkerAttentionState, WorkerId, WorkerProfile, WorkerSessionId,
};
#[cfg(test)]
use swarm_domain::{ControlRoomEventKind, PresenceDeviceId};
use swarm_persistence::{
    DecisionDeliveryFailure, DecisionDispatch, JiraIssueSnapshot, JiraProjectBindingInput,
    JiraTransitionFailure, TaskDispatch, TaskDispatchFailure, TaskOutcomeDispatch,
    TaskOutcomeFailure, TaskStore, TaskStoreError,
};
#[cfg(test)]
use swarm_terminal::{
    CANONICAL_COMPACTION_INPUT_BYTES, MAX_CANONICAL_SNAPSHOT_BYTES, ProcessResourceSample,
};
use swarm_terminal::{
    HostClient, HostRequest, HostResponse, JournalLimits, MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS,
    MAX_TERMINAL_ROWS, MIN_TERMINAL_COLUMNS, MIN_TERMINAL_ROWS, ProviderActivity, TerminalSize,
};
use tokio::sync::{Mutex, Notify, RwLock, Semaphore};
#[cfg(test)]
use tokio::time::sleep;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use attach::AttachGrantStore;
use attachments::{AttachmentError, AttachmentStore, MAX_ATTACHMENT_BYTES};
use auth::authorize;
#[cfg(test)]
use terminal_socket::{TERMINAL_GRANT_PROTOCOL_PREFIX, TERMINAL_WEBSOCKET_PROTOCOL};

const MAX_TERMINAL_WEBSOCKETS: usize = 32;
const RESOURCE_ADVISORY_BYTES: u64 = 256 * 1024 * 1024;
const RESOURCE_CRITICAL_BYTES: u64 = 512 * 1024 * 1024;
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
    development_reload: Arc<Mutex<()>>,
    coordination_delivery: Arc<Mutex<()>>,
    jira_delivery: Arc<Mutex<()>>,
    email_delivery: Arc<Mutex<()>>,
    worker_errors: Arc<RwLock<HashMap<WorkerId, String>>>,
    worker_recovery_attempts: Arc<RwLock<HashMap<WorkerId, i64>>>,
    provider_activity: Arc<RwLock<HashMap<WorkerSessionId, ProviderActivity>>>,
    control_room_notify: Arc<Notify>,
    notification_sender: Option<notifications::NotificationSender>,
    attachment_store: Option<AttachmentStore>,
    email_attachment_store: Option<email_attachments::EmailAttachmentStore>,
    workspace_roots: Arc<Vec<PathBuf>>,
    maintenance_request_path: Option<Arc<PathBuf>>,
    development_reload_request_path: Option<Arc<PathBuf>>,
    development_reload_status_path: Option<Arc<PathBuf>>,
    development_checkout_path: Option<Arc<PathBuf>>,
    maintenance_timeout: Duration,
    jira_readiness: jira::JiraReadinessProbe,
    outlook: Arc<RwLock<outlook::OutlookProbe>>,
    email_oauth_configuration: Arc<RwLock<Option<EmailOAuthConfigurationState>>>,
    email_oauth_config_path: Option<Arc<PathBuf>>,
    email_oauth_token_path: Option<Arc<PathBuf>>,
    public_base_url: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmailOAuthConfigurationSource {
    Environment,
    Operator,
}

#[derive(Clone, Debug)]
struct EmailOAuthConfigurationState {
    tenant_id: String,
    client_id: String,
    source: EmailOAuthConfigurationSource,
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
            development_reload: Arc::new(Mutex::new(())),
            coordination_delivery: Arc::new(Mutex::new(())),
            jira_delivery: Arc::new(Mutex::new(())),
            email_delivery: Arc::new(Mutex::new(())),
            worker_errors: Arc::new(RwLock::new(HashMap::new())),
            worker_recovery_attempts: Arc::new(RwLock::new(HashMap::new())),
            provider_activity: Arc::new(RwLock::new(HashMap::new())),
            control_room_notify: Arc::new(Notify::new()),
            notification_sender: None,
            attachment_store: None,
            email_attachment_store: None,
            workspace_roots: Arc::new(Vec::new()),
            maintenance_request_path: None,
            development_reload_request_path: None,
            development_reload_status_path: None,
            development_checkout_path: None,
            maintenance_timeout: Duration::from_secs(45),
            jira_readiness: jira::JiraReadinessProbe::default(),
            outlook: Arc::new(RwLock::new(outlook::OutlookProbe::default())),
            email_oauth_configuration: Arc::new(RwLock::new(None)),
            email_oauth_config_path: None,
            email_oauth_token_path: None,
            public_base_url: None,
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
        let client_id = client_id.into();
        let client_secret = client_secret.into();
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

    /// Enables operator-driven Microsoft OAuth with host-owned durable tokens.
    ///
    /// # Errors
    /// Rejects invalid tenant/callback settings or unreadable token storage.
    pub fn with_outlook_oauth(
        mut self,
        tenant_id: &str,
        client_id: impl Into<Arc<str>>,
        client_secret: impl Into<Arc<str>>,
        public_base_url: &str,
        token_path: PathBuf,
    ) -> Result<Self, String> {
        let client_id = client_id.into();
        let client_secret = client_secret.into();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| format!("Microsoft OAuth client could not start: {error}"))?;
        let oauth = microsoft_oauth::MicrosoftOAuthClient::new(
            client,
            tenant_id,
            client_id.clone(),
            client_secret,
            public_base_url,
            token_path,
        )?;
        self.outlook = Arc::new(RwLock::new(outlook::OutlookProbe::oauth(oauth)));
        self.email_oauth_configuration =
            Arc::new(RwLock::new(Some(EmailOAuthConfigurationState {
                tenant_id: tenant_id.trim().to_owned(),
                client_id: client_id.to_string(),
                source: EmailOAuthConfigurationSource::Environment,
            })));
        Ok(self)
    }

    /// Configures private storage used by operator-managed Microsoft OAuth setup.
    #[must_use]
    pub fn with_email_oauth_paths(
        mut self,
        configuration_path: PathBuf,
        token_path: PathBuf,
    ) -> Self {
        self.email_oauth_config_path = Some(Arc::new(configuration_path));
        self.email_oauth_token_path = Some(Arc::new(token_path));
        self
    }

    /// Loads an operator-managed Microsoft OAuth registration when present.
    ///
    /// # Errors
    /// Rejects an unreadable registration or a missing public callback URL.
    pub fn with_saved_outlook_oauth(mut self) -> Result<Self, String> {
        let Some(configuration_path) = self.email_oauth_config_path.as_deref() else {
            return Ok(self);
        };
        let Some(configuration) = microsoft_oauth::load_configuration(configuration_path.as_ref())?
        else {
            return Ok(self);
        };
        let public_base_url = self
            .public_base_url
            .as_deref()
            .ok_or("Microsoft email OAuth requires SWARM_PUBLIC_BASE_URL")?;
        let token_path = self
            .email_oauth_token_path
            .as_deref()
            .ok_or("Microsoft email OAuth token storage is not configured")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| format!("Microsoft OAuth client could not start: {error}"))?;
        let oauth = microsoft_oauth::MicrosoftOAuthClient::new(
            client,
            &configuration.tenant_id,
            configuration.client_id.clone(),
            configuration.client_secret,
            public_base_url,
            token_path.clone(),
        )?;
        self.outlook = Arc::new(RwLock::new(outlook::OutlookProbe::oauth(oauth)));
        self.email_oauth_configuration =
            Arc::new(RwLock::new(Some(EmailOAuthConfigurationState {
                tenant_id: configuration.tenant_id,
                client_id: configuration.client_id,
                source: EmailOAuthConfigurationSource::Operator,
            })));
        Ok(self)
    }

    /// Configures the HTTPS endpoint placed in signed federation invitations.
    /// Loopback HTTP is accepted only for isolated local testing.
    ///
    /// # Errors
    /// Rejects credentials, query/fragment data, missing hosts, and insecure
    /// non-loopback transport.
    pub fn with_public_base_url(mut self, public_base_url: &str) -> Result<Self, String> {
        let parsed = reqwest::Url::parse(public_base_url.trim())
            .map_err(|_| "SWARM_PUBLIC_BASE_URL must be a valid URL")?;
        let local_http = parsed.scheme() == "http"
            && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
        if (parsed.scheme() != "https" && !local_http)
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.host_str().is_none()
        {
            return Err(
                "SWARM_PUBLIC_BASE_URL must be HTTPS without credentials, query, or fragment"
                    .into(),
            );
        }
        self.public_base_url = Some(Arc::from(public_base_url.trim().trim_end_matches('/')));
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
    pub fn with_email_attachment_store(mut self, root: PathBuf) -> Self {
        self.email_attachment_store = Some(email_attachments::EmailAttachmentStore::new(root));
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

    #[must_use]
    pub fn with_development_reload_paths(mut self, request: PathBuf, status: PathBuf) -> Self {
        self.development_reload_request_path = Some(Arc::new(request));
        self.development_reload_status_path = Some(Arc::new(status));
        self
    }

    #[must_use]
    pub fn with_development_checkout_path(mut self, checkout: PathBuf) -> Self {
        self.development_checkout_path = Some(Arc::new(checkout));
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

    /// Pulls one bounded federation snapshot from Keeper for a joined Member.
    /// Jira is deliberately absent from this path: every Hive continues to
    /// synchronize canonical Jira work directly with Jira.
    pub async fn reconcile_federation(&self) {
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let service = ApiaryService::new(store.clone());
        let now = unix_timestamp();
        let health = match service.federation_sync_health() {
            Ok(health) => health,
            Err(ApplicationError::Store(TaskStoreError::InvalidFederationSync)) => return,
            Err(error) => {
                tracing::warn!(%error, "federation reconciliation could not read local health");
                return;
            }
        };
        if matches!(
            health.condition,
            FederationSyncCondition::AuthenticationRequired | FederationSyncCondition::Incompatible
        ) || health.next_attempt_at.is_some_and(|next| next > now)
        {
            return;
        }
        let connection = match service.federation_member_connection() {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!(%error, "federation reconciliation could not load member transport");
                return;
            }
        };
        if connection.credential_expires_at <= now {
            record_federation_failure(
                &service,
                FederationSyncCondition::AuthenticationRequired,
                now,
                self.control_room_notify.as_ref(),
            );
            return;
        }
        let client = match federation_http::FederationHttpClient::new(&connection.keeper_endpoint) {
            Ok(client) => client,
            Err(error) => {
                record_federation_failure(
                    &service,
                    federation_sync_condition(error),
                    now,
                    self.control_room_notify.as_ref(),
                );
                return;
            }
        };
        let snapshot = match client.catalog(&connection.node_credential).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                record_federation_failure(
                    &service,
                    federation_sync_condition(error),
                    now,
                    self.control_room_notify.as_ref(),
                );
                return;
            }
        };
        if let Err(error) = service.acknowledge_federation_catalog(&snapshot, now) {
            tracing::warn!(%error, "Keeper returned an incompatible federation catalog");
            record_federation_failure(
                &service,
                FederationSyncCondition::Incompatible,
                now,
                self.control_room_notify.as_ref(),
            );
            return;
        }
        if let Err(condition) =
            reconcile_federation_tasks(&service, &client, &connection.node_credential, now).await
        {
            record_federation_failure(&service, condition, now, self.control_room_notify.as_ref());
            return;
        }
        match service.record_federation_sync_success(now) {
            Ok(_) => self.control_room_notify.notify_waiters(),
            Err(error) => {
                tracing::warn!(%error, "federation reconciliation result could not be persisted");
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

    /// Delivers one operator-approved email resolution reply, if queued.
    pub async fn deliver_email_replies(&self) {
        let _guard = self.email_delivery.lock().await;
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let dispatch = match store.claim_email_reply() {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%error, "email reply queue could not be claimed");
                return;
            }
        };
        let outlook = self.outlook.read().await.clone();
        let outcome = outlook.reply(&dispatch.message_id, &dispatch.body).await;
        let result = match outcome {
            Ok(receipt) => store.complete_email_reply(&dispatch.target_id, &receipt),
            Err(outlook::OutlookError::AmbiguousDelivery) => store.fail_email_reply(
                &dispatch.target_id,
                &swarm_persistence::EmailReplyFailure::Uncertain(
                    "Microsoft may have accepted the reply; review the original thread before retrying"
                        .into(),
                ),
            ),
            Err(outlook::OutlookError::NetworkUnavailable) => store.fail_email_reply(
                &dispatch.target_id,
                &swarm_persistence::EmailReplyFailure::Retryable(
                    "Microsoft Outlook is temporarily unavailable".into(),
                ),
            ),
            Err(error) => store.fail_email_reply(
                &dispatch.target_id,
                &swarm_persistence::EmailReplyFailure::Permanent(error.to_string()),
            ),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "email reply delivery state could not be recorded");
        }
        self.control_room_notify.notify_waiters();
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

    /// Marks crash-interrupted email sends uncertain so they cannot replay silently.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn recover_email_reply_deliveries(&self) -> Result<usize, TaskStoreError> {
        self.task_store
            .as_ref()
            .map_or(Ok(0), TaskStore::recover_inflight_email_replies)
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
        let live = match worker_runtime::reconcile_worker_bindings(self).await {
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
        provider_activity::refresh(self, &profiles, &live).await;
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
                worker_runtime::start_worker_process(self, profile.id, TerminalSize::default())
                    .await
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
    worker_engine_build_id: &'static str,
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

#[derive(Debug, Serialize)]
struct ApiaryHiveCandidateView {
    #[serde(flatten)]
    candidate: swarm_domain::ApiaryHiveCandidate,
    invitation_pending: bool,
}

#[derive(Serialize)]
struct FederationTransportReadinessView {
    configured: bool,
    endpoint: Option<String>,
    reachability: &'static str,
}

#[derive(Debug, Deserialize)]
struct FederationBootstrapRequest {
    secret: String,
    #[serde(default)]
    connection_card: Option<HiveConnectionCard>,
}

#[derive(Debug, Deserialize)]
struct SaveApiaryKeeperLinkRequest {
    link_id: ApiaryJoinLinkId,
    keeper_endpoint: String,
    secret: String,
}

#[derive(Debug, Serialize)]
struct ApiaryKeeperLinkPollView {
    link: ApiaryJoinLink,
    invitation_received: bool,
}

#[derive(Debug, Serialize)]
struct FederationJoinInvitationView {
    #[serde(flatten)]
    invitation: swarm_domain::FederationJoinInvitation,
    readiness: swarm_domain::FederationJoinReadiness,
}

#[derive(Debug, Deserialize)]
struct ReserveFederationClaimRequest {
    project_id: String,
    issue_id: String,
    issue_key: String,
}

#[derive(Debug, Deserialize)]
struct FederationTaskPageQuery {
    #[serde(default)]
    after: i64,
}

#[derive(Debug, Deserialize)]
struct CreateApiaryTaskRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
}

#[derive(Debug, Deserialize)]
struct TransitionApiaryTaskRequest {
    target_state: TaskState,
}

#[derive(Debug, Serialize)]
struct FederationClaimRollupView {
    #[serde(flatten)]
    claim: FederationSharedClaim,
    project_key: String,
    project_name: String,
    home_hive_name: String,
    home_operator_display_name: String,
}

impl From<FederationJoinInvitationOverview> for FederationJoinInvitationView {
    fn from(value: FederationJoinInvitationOverview) -> Self {
        Self {
            invitation: value.invitation,
            readiness: value.readiness,
        }
    }
}

impl From<ApiaryHiveCandidateOverview> for ApiaryHiveCandidateView {
    fn from(value: ApiaryHiveCandidateOverview) -> Self {
        Self {
            candidate: value.candidate,
            invitation_pending: value.invitation_pending,
        }
    }
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
struct RenameIdentityRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AcceptApiaryPolicyRequest {
    policy_revision: u64,
}

#[derive(Debug, Deserialize)]
struct SetStewardshipRequest {
    managed_hive_ids: Vec<HiveId>,
    capabilities: Vec<StewardCapability>,
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
struct JiraCommentRequest {
    body: String,
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
            get(auth::get_session)
                .post(auth::create_session)
                .delete(auth::delete_session),
        )
        .route("/api/v1/hive", get(local_hive).put(rename_local_hive))
        .route(
            "/api/v1/apiary",
            post(create_apiary).put(rename_local_apiary),
        )
        .route("/api/v1/apiary/members", get(apiary_members))
        .route("/api/v1/apiary/stewardships", get(apiary_stewardships))
        .route(
            "/api/v1/apiary/stewardships/by-operator/{operator_id}",
            put(set_apiary_stewardship),
        )
        .route(
            "/api/v1/apiary/stewardships/{stewardship_id}",
            delete(revoke_apiary_stewardship),
        )
        .route("/api/v1/apiary/shared-work", get(apiary_shared_work))
        .route("/api/v1/apiary/sync-health", get(apiary_sync_health))
        .route(
            "/api/v1/apiary/connection-card",
            get(download_hive_connection_card),
        )
        .route(
            "/api/v1/apiary/transport-readiness",
            get(federation_transport_readiness),
        )
        .route(
            "/api/v1/apiary/join-links",
            get(apiary_join_links).post(create_apiary_join_link),
        )
        .route(
            "/api/v1/apiary/join-links/{link_id}/approval",
            post(approve_apiary_join_link),
        )
        .route(
            "/api/v1/apiary/keeper-links",
            get(apiary_keeper_links).post(save_apiary_keeper_link),
        )
        .route(
            "/api/v1/apiary/keeper-links/{link_id}/poll",
            post(poll_apiary_keeper_link),
        )
        .route(
            "/api/v1/apiary/hive-candidates",
            get(apiary_hive_candidates).post(pin_apiary_hive_candidate),
        )
        .route(
            "/api/v1/apiary/hive-candidates/{hive_id}/invitation",
            post(invite_apiary_hive_candidate),
        )
        .route(
            "/api/v1/apiary/join-invitations",
            get(apiary_join_invitations).post(import_apiary_join_invitation),
        )
        .route(
            "/api/v1/apiary/join-invitations/{invitation_id}/policy-acceptance",
            post(accept_imported_apiary_policy),
        )
        .route(
            "/api/v1/apiary/join-invitations/{invitation_id}/submission",
            post(prepare_imported_apiary_join),
        )
        .route("/api/v1/federation/join", post(consume_federation_join))
        .route(
            "/api/v1/federation/bootstrap/{link_id}",
            post(poll_federation_bootstrap),
        )
        .route("/api/v1/federation/catalog", get(federation_catalog))
        .route("/api/v1/federation/tasks", get(federation_tasks))
        .route(
            "/api/v1/federation/tasks/commands",
            post(apply_federation_task_command),
        )
        .route("/api/v1/federation/claims", post(reserve_federation_claim))
        .route(
            "/api/v1/federation/claims/{claim_id}",
            delete(release_federation_claim),
        )
        .route(
            "/api/v1/federation/claims/{claim_id}/confirmation",
            post(confirm_federation_claim),
        )
        .route(
            "/api/v1/apiary/catalog-acknowledgement",
            get(get_federation_catalog_acknowledgement).post(acknowledge_federation_catalog),
        )
        .route(
            "/api/v1/apiary/catalog-readiness",
            get(get_federation_catalog_readiness),
        )
        .route(
            "/api/v1/apiary/tasks",
            get(get_apiary_tasks).post(create_apiary_task),
        )
        .route(
            "/api/v1/apiary/task-sync-status",
            get(get_federation_task_sync_status),
        )
        .route(
            "/api/v1/apiary/tasks/{task_id}/claim",
            post(queue_federation_task_claim),
        )
        .route(
            "/api/v1/apiary/tasks/{task_id}/transition",
            post(queue_federation_task_transition),
        )
        .route(
            "/api/v1/apiary/task-outbox",
            get(get_federation_task_outbox),
        )
        .route(
            "/api/v1/apiary/task-outbox-status",
            get(get_federation_task_outbox_status),
        )
        .route(
            "/api/v1/apiary/collapse-readiness",
            get(apiary_collapse_readiness),
        )
        .route("/api/v1/apiary/collapse", post(collapse_apiary))
        .route("/api/v1/apiary/jira-projects", get(apiary_jira_projects))
        .route(
            "/api/v1/apiary/jira-projects/{binding_id}/promotion",
            post(promote_apiary_jira_project),
        )
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
            get(presence::operator_presence).put(presence::set_operator_presence),
        )
        .route(
            "/api/v1/presence/devices/{device_id}",
            put(presence::observe_presence_device),
        )
        .route(
            "/api/v1/notifications/settings",
            get(notifications::notification_settings).put(notifications::set_notification_policy),
        )
        .route(
            "/api/v1/orchestration/queen-policy",
            get(orchestration::queen_autonomy_policy).put(orchestration::set_queen_autonomy_policy),
        )
        .route(
            "/api/v1/preferences/presentation/{device_class}",
            get(presentation::presentation_preferences)
                .put(presentation::set_presentation_preferences),
        )
        .route(
            "/api/v1/notifications/subscriptions/{device_id}",
            put(notifications::save_notification_subscription)
                .delete(notifications::remove_notification_subscription),
        )
        .route(
            "/api/v1/notifications/subscriptions/{device_id}/test",
            post(notifications::test_notification),
        )
        .route("/api/v1/control-room/events", get(control_room::events))
        .route("/api/v1/runtime/limits", get(runtime::limits))
        .route("/api/v1/runtime/resources", get(runtime::resources))
        .route(
            "/api/v1/runtime/terminal-host",
            get(runtime::terminal_host_status),
        )
        .route("/api/v1/runtime/development", get(runtime::development))
        .route(
            "/api/v1/runtime/development/reload",
            post(maintenance::request_development_reload),
        )
        .route(
            "/api/v1/runtime/terminal-host/maintenance",
            post(maintenance::maintain_worker_engine),
        )
        .route("/api/v1/backups/database", get(backups::download_database))
        .route(
            "/api/v1/feedback/reports",
            get(feedback::list_reports).post(feedback::create_report),
        )
        .route(
            "/api/v1/feedback/attachments",
            post(feedback::upload_attachment).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BYTES)),
        )
        .route(
            "/api/v1/feedback/attachments/{name}",
            get(feedback::download_attachment),
        )
        .route(
            "/api/v1/tasks",
            get(tasks::list_tasks).post(tasks::create_task),
        )
        .route("/api/v1/decisions", get(decisions::list_decisions))
        .route(
            "/api/v1/decisions/{decision_id}/resolution",
            patch(decisions::resolve_decision),
        )
        .route("/api/v1/tasks/order", put(tasks::reorder_tasks))
        .route("/api/v1/tasks/activity", get(tasks::recent_task_activity))
        .route("/api/v1/tasks/{task_id}", patch(tasks::update_task))
        .route(
            "/api/v1/tasks/{task_id}/activity",
            get(tasks::task_activity),
        )
        .route(
            "/api/v1/tasks/{task_id}/state",
            patch(tasks::transition_task),
        )
        .route(
            "/api/v1/tasks/{task_id}/assignment",
            put(tasks::assign_task),
        )
        .route(
            "/api/v1/workers",
            get(workers::list_workers).post(workers::create_worker),
        )
        .route("/api/v1/providers", get(provider_activity::capabilities))
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
        .route("/api/v1/integrations/email/readiness", get(email_readiness))
        .route(
            "/api/v1/integrations/email/configuration",
            get(email_configuration).put(update_email_configuration),
        )
        .route(
            "/api/v1/integrations/email/auth/start",
            post(email_auth_start),
        )
        .route("/api/v1/integrations/email/auth", delete(email_disconnect))
        .route("/auth/email/callback", get(email_auth_callback))
        .route("/api/v1/integrations/email/inbox", get(email_inbox))
        .route(
            "/api/v1/integrations/email/messages/{message_id}",
            get(email_message),
        )
        .route(
            "/api/v1/integrations/email/messages/{message_id}/attachments/{attachment_id}",
            get(preview_email_attachment),
        )
        .route(
            "/api/v1/integrations/email/messages/{message_id}/import",
            post(import_email_message),
        )
        .route("/api/v1/integrations/email/import", post(import_email_task))
        .route(
            "/api/v1/integrations/email/task-links",
            get(email_task_sources),
        )
        .route("/api/v1/tasks/{task_id}/email", get(email_task_source))
        .route(
            "/api/v1/tasks/{task_id}/email/attachments/{storage_name}",
            get(download_email_attachment),
        )
        .route(
            "/api/v1/tasks/{task_id}/deployments",
            get(task_deployments).post(record_task_deployment),
        )
        .route(
            "/api/v1/tasks/{task_id}/email/reply",
            get(email_reply)
                .post(prepare_email_reply)
                .put(update_email_reply_draft),
        )
        .route(
            "/api/v1/integrations/email/replies/{reply_id}/send",
            post(send_email_reply),
        )
        .route(
            "/api/v1/integrations/email/replies/{reply_id}/retry",
            post(retry_email_reply),
        )
        .route("/api/v1/workers/order", put(workers::reorder_workers))
        .route("/api/v1/workers/{worker_id}", patch(workers::update_worker))
        .route("/api/v1/workspaces", get(workers::list_workspaces))
        .route(
            "/api/v1/workers/{worker_id}/start",
            post(workers::start_worker),
        )
        .route(
            "/api/v1/workers/{worker_id}/session",
            delete(workers::stop_worker),
        )
        .route(
            "/api/v1/terminal/sessions",
            get(session_history::list_live_sessions).post(terminal_control::start),
        )
        .route(
            "/api/v1/terminal/history/diagnostics",
            get(session_history::diagnostics),
        )
        .route(
            "/api/v1/terminal/history/sessions",
            get(session_history::list_retained_sessions),
        )
        .route(
            "/api/v1/terminal/history/sessions/{session_id}",
            get(session_history::read),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}",
            delete(terminal_control::stop),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/output",
            get(terminal_control::read_output),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/input",
            post(terminal_control::write_input),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/engagements/{device_id}",
            delete(terminal_attach::release_engagement),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attachments",
            post(terminal_attach::upload_attachment)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_BYTES)),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/size",
            put(terminal_control::resize),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attach-grants",
            post(terminal_attach::issue_grant),
        )
        .route(
            "/api/v1/terminal/sessions/{session_id}/attach",
            get(terminal_attach::attach),
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
        worker_engine_build_id: worker_engine_build_id(),
    })
}

fn build_version() -> &'static str {
    option_env!("SWARM_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn worker_engine_build_id() -> &'static str {
    option_env!("SWARM_WORKER_ENGINE_BUILD_ID").unwrap_or(build_version())
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

async fn rename_local_hive(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RenameIdentityRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let identity = apiary_service(&state)?
        .rename_local_hive(&request.name, unix_timestamp())
        .map_err(application_error)?;
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

async fn apiary_members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let members = apiary_service(&state)?
        .members()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(members)).into_response())
}

async fn apiary_stewardships(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let stewardships = apiary_service(&state)?
        .stewardships()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(stewardships)).into_response())
}

async fn set_apiary_stewardship(
    State(state): State<Arc<AppState>>,
    Path(steward_operator_id): Path<OperatorId>,
    headers: HeaderMap,
    Json(request): Json<SetStewardshipRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let stewardship: Stewardship = apiary_service(&state)?
        .set_stewardship(
            steward_operator_id,
            &request.managed_hive_ids,
            &request.capabilities,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(stewardship)).into_response())
}

async fn revoke_apiary_stewardship(
    State(state): State<Arc<AppState>>,
    Path(stewardship_id): Path<StewardshipId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    apiary_service(&state)?
        .revoke_stewardship(stewardship_id, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response())
}

async fn apiary_shared_work(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let service = apiary_service(&state)?;
    let claims = service
        .active_federation_claims(unix_timestamp())
        .map_err(application_error)?;
    let members = service.members().map_err(application_error)?;
    let projects = service
        .promoted_jira_projects()
        .map_err(application_error)?;
    let member_names = members
        .into_iter()
        .map(|member| {
            (
                member.hive_id,
                (member.hive_name, member.operator_display_name),
            )
        })
        .collect::<HashMap<_, _>>();
    let project_names = projects
        .into_iter()
        .map(|project| {
            (
                project.project_id,
                (project.project_key, project.project_name),
            )
        })
        .collect::<HashMap<_, _>>();
    let rollup = claims
        .into_iter()
        .map(|claim| {
            let (home_hive_name, home_operator_display_name) = member_names
                .get(&claim.home_hive_id)
                .cloned()
                .unwrap_or_else(|| ("Unknown Hive".into(), "Unknown operator".into()));
            let (project_key, project_name) = project_names
                .get(&claim.project_id)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        claim.issue_key.split('-').next().unwrap_or("Jira").into(),
                        "Promoted Jira project".into(),
                    )
                });
            FederationClaimRollupView {
                claim,
                project_key,
                project_name,
                home_hive_name,
                home_operator_display_name,
            }
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(rollup)).into_response())
}

async fn apiary_sync_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let health = apiary_service(&state)?
        .federation_sync_health()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(health)).into_response())
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

async fn download_hive_connection_card(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let card = apiary_service(&state)?
        .connection_card(unix_timestamp())
        .map_err(application_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-hive-connection.json"),
    );
    Ok((response_headers, Json(card)).into_response())
}

async fn federation_transport_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let endpoint = state.public_base_url.as_deref().map(str::to_owned);
    let reachability = endpoint.as_deref().map_or("unconfigured", |endpoint| {
        if federation_endpoint_is_remotely_reachable(endpoint) {
            "remote_https"
        } else {
            "local_only"
        }
    });
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(FederationTransportReadinessView {
            configured: endpoint.is_some(),
            endpoint,
            reachability,
        }),
    )
        .into_response())
}

async fn create_apiary_join_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let endpoint = state.public_base_url.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "federation_endpoint_unavailable",
            "configure a remotely reachable SWARM_PUBLIC_BASE_URL before creating an invitation link",
        )
    })?;
    if !federation_endpoint_is_remotely_reachable(endpoint) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "federation_endpoint_local_only",
            "the Keeper URL must use remote HTTPS before another Hive can join",
        ));
    }
    let bundle = apiary_service(&state)?
        .create_join_link(endpoint, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(bundle),
    )
        .into_response())
}

async fn apiary_join_links(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let links = apiary_service(&state)?
        .join_links(unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(links)).into_response())
}

async fn approve_apiary_join_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(link_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let link = apiary_service(&state)?
        .approve_join_link(parse_apiary_join_link_id(&link_id)?, unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(link)).into_response())
}

async fn apiary_keeper_links(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let links: Vec<ApiaryKeeperLink> = apiary_service(&state)?
        .keeper_links()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(links)).into_response())
}

async fn save_apiary_keeper_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SaveApiaryKeeperLinkRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let now = unix_timestamp();
    apiary_service(&state)?
        .save_keeper_link(
            request.link_id,
            &request.keeper_endpoint,
            &request.secret,
            now,
        )
        .map_err(application_error)?;
    let view = poll_saved_apiary_keeper_link(&state, request.link_id, true, now).await?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(view),
    )
        .into_response())
}

async fn poll_apiary_keeper_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(link_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let view = poll_saved_apiary_keeper_link(
        &state,
        parse_apiary_join_link_id(&link_id)?,
        false,
        unix_timestamp(),
    )
    .await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(view)).into_response())
}

async fn poll_saved_apiary_keeper_link(
    state: &AppState,
    link_id: ApiaryJoinLinkId,
    present_identity: bool,
    now: i64,
) -> Result<ApiaryKeeperLinkPollView, ApiError> {
    let service = apiary_service(state)?;
    let (endpoint, secret) = service
        .keeper_link_credential(link_id)
        .map_err(application_error)?;
    let card = if present_identity {
        Some(service.connection_card(now).map_err(application_error)?)
    } else {
        None
    };
    let poll = federation_http::FederationHttpClient::new(&endpoint)
        .map_err(federation_http_error)?
        .bootstrap(link_id, &secret, card.as_ref())
        .await
        .map_err(federation_http_error)?;
    service
        .record_keeper_link_poll(&poll.link, now)
        .map_err(application_error)?;
    let invitation_received = if let Some(invitation) = poll.invitation.as_ref() {
        match service.import_invitation(invitation, now) {
            Ok(_) | Err(ApplicationError::Store(TaskStoreError::FederationInvitationConflict)) => {
                service
                    .remove_keeper_link(link_id)
                    .map_err(application_error)?;
                true
            }
            Err(error) => return Err(application_error(error)),
        }
    } else {
        false
    };
    Ok(ApiaryKeeperLinkPollView {
        link: poll.link,
        invitation_received,
    })
}

async fn poll_federation_bootstrap(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Json(request): Json<FederationBootstrapRequest>,
) -> Result<Response, ApiError> {
    let service = apiary_service(&state)?;
    let link_id = parse_apiary_join_link_id(&link_id)?;
    let now = unix_timestamp();
    if let Some(card) = request.connection_card.as_ref() {
        service
            .present_join_link_identity(link_id, &request.secret, card, now)
            .map_err(federation_bootstrap_error)?;
    }
    let poll = service
        .poll_join_link(link_id, &request.secret, now)
        .map_err(federation_bootstrap_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(poll)).into_response())
}

async fn apiary_hive_candidates(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let candidates = apiary_service(&state)?
        .hive_candidate_overviews(unix_timestamp())
        .map_err(application_error)?
        .into_iter()
        .map(ApiaryHiveCandidateView::from)
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(candidates)).into_response())
}

async fn pin_apiary_hive_candidate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(card): Json<HiveConnectionCard>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let candidate = apiary_service(&state)?
        .pin_hive_candidate(&card, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(candidate),
    )
        .into_response())
}

async fn invite_apiary_hive_candidate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(hive_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let endpoint = state.public_base_url.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "federation_endpoint_unavailable",
            "configure SWARM_PUBLIC_BASE_URL before inviting another Hive",
        )
    })?;
    let bundle = apiary_service(&state)?
        .invite_hive_candidate(parse_hive_id(&hive_id)?, endpoint, unix_timestamp())
        .map_err(application_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-apiary-invitation.json"),
    );
    Ok((StatusCode::CREATED, response_headers, Json(bundle)).into_response())
}

async fn apiary_join_invitations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let invitations = apiary_service(&state)?
        .imported_invitations(jira.connection, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            invitations
                .into_iter()
                .map(FederationJoinInvitationView::from)
                .collect::<Vec<_>>(),
        ),
    )
        .into_response())
}

async fn accept_imported_apiary_policy(
    State(state): State<Arc<AppState>>,
    Path(invitation_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<AcceptApiaryPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let overview = apiary_service(&state)?
        .accept_imported_policy(
            parse_apiary_invitation_id(&invitation_id)?,
            request.policy_revision,
            jira.connection,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(FederationJoinInvitationView::from(overview)),
    )
        .into_response())
}

async fn prepare_imported_apiary_join(
    State(state): State<Arc<AppState>>,
    Path(invitation_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let invitation_id = parse_apiary_invitation_id(&invitation_id)?;
    let now = unix_timestamp();
    let jira = state.jira_readiness.readiness().await;
    let service = apiary_service(&state)?;
    let invitation = service
        .imported_invitations(jira.connection, now)
        .map_err(application_error)?
        .into_iter()
        .find(|overview| overview.invitation.invitation_id == invitation_id)
        .ok_or_else(|| task_store_error(&TaskStoreError::ApiaryInvitationNotFound))?;
    let submission = service
        .prepare_imported_join_submission(invitation_id, jira.connection, now)
        .map_err(application_error)?;
    let acceptance =
        federation_http::FederationHttpClient::new(&invitation.invitation.keeper_endpoint)
            .map_err(federation_http_error)?
            .join(&submission)
            .await
            .map_err(federation_http_error)?;
    let context = service
        .apply_remote_join_acceptance(invitation_id, &acceptance, now)
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(context),
    )
        .into_response())
}

async fn import_apiary_join_invitation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(bundle): Json<ApiaryInvitationBundle>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let invitation = apiary_service(&state)?
        .import_invitation(&bundle, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(invitation),
    )
        .into_response())
}

async fn consume_federation_join(
    State(state): State<Arc<AppState>>,
    Json(submission): Json<FederationJoinSubmission>,
) -> Result<Response, ApiError> {
    let acceptance = apiary_service(&state)?
        .consume_remote_join_submission(&submission, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(acceptance),
    )
        .into_response())
}

async fn federation_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let snapshot = apiary_service(&state)?
        .federation_catalog(credential, unix_timestamp())
        .map_err(federation_catalog_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(snapshot)).into_response())
}

async fn federation_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<FederationTaskPageQuery>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let page: FederationTaskPage = apiary_service(&state)?
        .federation_task_page(credential, query.after, unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(page)).into_response())
}

async fn apply_federation_task_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(command): Json<FederationTaskCommand>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let receipt = apiary_service(&state)?
        .apply_federation_task_command(credential, &command, unix_timestamp())
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(receipt)).into_response())
}

async fn reserve_federation_claim(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReserveFederationClaimRequest>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let claim = apiary_service(&state)?
        .reserve_federation_claim(
            credential,
            &request.project_id,
            &request.issue_id,
            &request.issue_key,
            unix_timestamp(),
        )
        .map_err(federation_claim_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(claim),
    )
        .into_response())
}

async fn confirm_federation_claim(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(claim_id): Path<String>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let claim_id = parse_federation_claim_id(&claim_id)?;
    let claim = apiary_service(&state)?
        .confirm_federation_claim(credential, claim_id, unix_timestamp())
        .map_err(federation_claim_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(claim)).into_response())
}

async fn release_federation_claim(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(claim_id): Path<String>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    let claim_id = parse_federation_claim_id(&claim_id)?;
    let claim = apiary_service(&state)?
        .release_federation_claim(credential, claim_id, unix_timestamp())
        .map_err(federation_claim_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(claim)).into_response())
}

async fn acknowledge_federation_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(snapshot): Json<FederationCatalogSnapshot>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let acknowledgement = apiary_service(&state)?
        .acknowledge_federation_catalog(&snapshot, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(acknowledgement),
    )
        .into_response())
}

async fn get_federation_catalog_acknowledgement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let acknowledgement = apiary_service(&state)?
        .federation_catalog_acknowledgement()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(acknowledgement)).into_response())
}

async fn get_federation_catalog_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let jira = state.jira_readiness.readiness().await;
    let readiness = apiary_service(&state)?
        .federation_catalog_readiness(jira.connection, unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(readiness)).into_response())
}

async fn get_apiary_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks: Vec<ApiaryTask> = apiary_service(&state)?
        .visible_apiary_tasks()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

async fn create_apiary_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateApiaryTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = apiary_service(&state)?
        .create_apiary_task(
            &request.title,
            &request.description,
            request.priority,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(task),
    )
        .into_response())
}

async fn get_federation_task_sync_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status: FederationTaskSyncStatus = apiary_service(&state)?
        .federation_task_sync_status()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

fn parse_apiary_task_id(value: &str) -> Result<ApiaryTaskId, ApiError> {
    ApiaryTaskId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_apiary_task_id",
            "invalid Apiary task id",
        )
    })
}

async fn queue_federation_task_claim(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let entry = apiary_service(&state)?
        .queue_federation_task_claim(parse_apiary_task_id(&task_id)?, unix_timestamp())
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(entry),
    )
        .into_response())
}

async fn queue_federation_task_transition(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TransitionApiaryTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let entry = apiary_service(&state)?
        .queue_federation_task_transition(
            parse_apiary_task_id(&task_id)?,
            request.target_state,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(entry),
    )
        .into_response())
}

async fn get_federation_task_outbox(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let entries: Vec<FederationTaskOutboxEntry> = apiary_service(&state)?
        .federation_task_outbox()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(entries)).into_response())
}

async fn get_federation_task_outbox_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status: FederationTaskOutboxStatus = apiary_service(&state)?
        .federation_task_outbox_status()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
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

async fn rename_local_apiary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RenameIdentityRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let context = apiary_service(&state)?
        .rename_local_apiary(&request.name, unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(context)).into_response())
}

async fn apiary_collapse_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let readiness = apiary_service(&state)?
        .collapse_readiness()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(readiness)).into_response())
}

async fn collapse_apiary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let context = apiary_service(&state)?
        .collapse(unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(context)).into_response())
}

async fn apiary_jira_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let projects = apiary_service(&state)?
        .promoted_jira_projects()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(projects)).into_response())
}

async fn promote_apiary_jira_project(
    State(state): State<Arc<AppState>>,
    Path(binding_id): Path<JiraProjectBindingId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let project = apiary_service(&state)?
        .promote_jira_binding(binding_id, unix_timestamp())
        .map_err(application_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(project),
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

#[derive(Serialize)]
struct EmailAuthorizationStart {
    authorization_url: String,
}

#[derive(Serialize)]
struct EmailOAuthConfigurationView {
    configured: bool,
    managed_by: Option<&'static str>,
    tenant_id: Option<String>,
    client_id: Option<String>,
    callback_url: Option<String>,
    secret_stored: bool,
}

#[derive(Deserialize)]
struct UpdateEmailOAuthConfiguration {
    tenant_id: String,
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct EmailAuthorizationCallback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct EmailInboxQuery {
    query: Option<String>,
}

#[derive(Deserialize)]
struct ImportEmailRequest {
    #[serde(default)]
    priority: TaskPriority,
}

#[derive(Deserialize)]
struct ImportEmailTaskRequest {
    message_ids: Vec<String>,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
    worker_id: Option<WorkerId>,
    state: Option<TaskState>,
}

#[derive(Deserialize)]
struct RecordDeploymentRequest {
    environment: String,
    reference: String,
    deployed_at: Option<i64>,
}

#[derive(Deserialize)]
struct EmailReplyRequest {
    body: String,
}

struct StoredEmailAttachment {
    storage_name: String,
    display_name: String,
    media_type: String,
    byte_size: u64,
    inline: bool,
    content_id: Option<String>,
}

async fn email_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(outlook.readiness().await),
    )
        .into_response())
}

async fn email_configuration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let configuration = state.email_oauth_configuration.read().await.clone();
    let callback_url = state
        .public_base_url
        .as_deref()
        .map(|base| format!("{}/auth/email/callback", base.trim_end_matches('/')));
    let response = match configuration {
        Some(configuration) => EmailOAuthConfigurationView {
            configured: true,
            managed_by: Some(match configuration.source {
                EmailOAuthConfigurationSource::Environment => "environment",
                EmailOAuthConfigurationSource::Operator => "operator",
            }),
            tenant_id: Some(configuration.tenant_id),
            client_id: Some(configuration.client_id),
            callback_url,
            secret_stored: true,
        },
        None => EmailOAuthConfigurationView {
            configured: false,
            managed_by: None,
            tenant_id: None,
            client_id: None,
            callback_url,
            secret_stored: false,
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

async fn update_email_configuration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<UpdateEmailOAuthConfiguration>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    if state
        .email_oauth_configuration
        .read()
        .await
        .as_ref()
        .is_some_and(|configuration| {
            configuration.source == EmailOAuthConfigurationSource::Environment
        })
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "email_configuration_managed_by_host",
            "Microsoft email OAuth is managed by this host and cannot be changed here",
        ));
    }
    let current = state.outlook.read().await.clone();
    if let Some(client) = current.oauth_client()
        && client.has_connection().await
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "email_account_connected",
            "Disconnect Outlook before replacing its Microsoft app registration",
        ));
    }
    let tenant_id = request.tenant_id.trim().to_owned();
    let client_id = request.client_id.trim().to_owned();
    let client_secret = request.client_secret;
    let public_base_url = state.public_base_url.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "email_public_url_unconfigured",
            "Set the Hive public URL before configuring Microsoft email OAuth",
        )
    })?;
    let configuration_path = state.email_oauth_config_path.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_configuration_storage_unavailable",
            "Private Microsoft email configuration storage is unavailable",
        )
    })?;
    let token_path = state.email_oauth_token_path.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_token_storage_unavailable",
            "Private Microsoft email token storage is unavailable",
        )
    })?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "email_oauth_unavailable",
                "Microsoft email OAuth could not start",
            )
        })?;
    let oauth = microsoft_oauth::MicrosoftOAuthClient::new(
        client,
        &tenant_id,
        client_id.clone(),
        client_secret.clone(),
        public_base_url,
        token_path.clone(),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_email_configuration",
            error,
        )
    })?;
    microsoft_oauth::save_configuration(
        configuration_path.as_ref(),
        &microsoft_oauth::MicrosoftOAuthConfiguration {
            tenant_id: tenant_id.clone(),
            client_id: client_id.clone(),
            client_secret,
        },
    )
    .map_err(email_oauth_error)?;
    *state.outlook.write().await = outlook::OutlookProbe::oauth(oauth);
    *state.email_oauth_configuration.write().await = Some(EmailOAuthConfigurationState {
        tenant_id,
        client_id,
        source: EmailOAuthConfigurationSource::Operator,
    });
    email_configuration(State(state), headers).await
}

async fn email_auth_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let oauth = outlook.oauth_client().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_oauth_unavailable",
            "Microsoft email OAuth is not configured on this Swarm host",
        )
    })?;
    let url = oauth.authorization_url().await.map_err(email_oauth_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(EmailAuthorizationStart {
            authorization_url: url.to_string(),
        }),
    )
        .into_response())
}

async fn email_auth_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EmailAuthorizationCallback>,
) -> Response {
    let outlook = state.outlook.read().await.clone();
    let Some(oauth) = outlook.oauth_client() else {
        return Redirect::to("/?email=unavailable#settings-integrations").into_response();
    };
    if query.error.is_some() {
        return Redirect::to("/?email=denied#settings-integrations").into_response();
    }
    let result = match (query.state.as_deref(), query.code.as_deref()) {
        (Some(auth_state), Some(code)) => oauth.exchange_code(auth_state, code).await,
        _ => Err(microsoft_oauth::OAuthError::InvalidState),
    };
    let location = if result.is_ok() {
        "/?email=connected#settings-integrations"
    } else {
        "/?email=failed#settings-integrations"
    };
    Redirect::to(location).into_response()
}

async fn email_disconnect(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let oauth = outlook.oauth_client().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_oauth_unavailable",
            "Microsoft email OAuth is not configured on this Swarm host",
        )
    })?;
    oauth.disconnect().await.map_err(email_oauth_error)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn email_inbox(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EmailInboxQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let messages = outlook
        .inbox(query.query.as_deref())
        .await
        .map_err(outlook_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(messages)).into_response())
}

async fn email_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let message = outlook.message(&message_id).await.map_err(outlook_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(message)).into_response())
}

async fn preview_email_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((message_id, attachment_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let content = outlook
        .attachment(&message_id, &attachment_id)
        .await
        .map_err(outlook_error)?;
    let media_type = content.metadata.media_type.as_str();
    if !matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    ) {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "email_preview_unsupported",
            "only bounded raster images can be previewed",
        ));
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response_headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type)
            .map_err(|_| outlook_error(outlook::OutlookError::InvalidResponse))?,
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    Ok((response_headers, content.bytes).into_response())
}

async fn import_email_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Json(request): Json<ImportEmailRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let outlook = state.outlook.read().await.clone();
    let message = outlook.message(&message_id).await.map_err(outlook_error)?;
    let attachment_store = state.email_attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_attachment_store_unconfigured",
            "private email attachment storage is not configured",
        )
    })?;
    let mut stored = Vec::with_capacity(message.attachments.len());
    for attachment in &message.attachments {
        let content = outlook
            .attachment(&message_id, &attachment.id)
            .await
            .map_err(outlook_error)?;
        let storage_name = attachment_store
            .save(&content.metadata.media_type, &content.bytes)
            .await
            .map_err(email_attachment_error)?;
        stored.push(StoredEmailAttachment {
            storage_name,
            display_name: content.metadata.name,
            media_type: content.metadata.media_type,
            byte_size: content.bytes.len() as u64,
            inline: content.metadata.inline,
            content_id: content.metadata.content_id,
        });
    }
    let attachments = stored
        .iter()
        .map(|attachment| swarm_persistence::EmailAttachmentSnapshot {
            storage_name: &attachment.storage_name,
            display_name: &attachment.display_name,
            media_type: &attachment.media_type,
            byte_size: attachment.byte_size,
            inline: attachment.inline,
            content_id: attachment.content_id.as_deref(),
        })
        .collect::<Vec<_>>();
    let summary = &message.summary;
    let imported = task_store(&state)?
        .import_email_message(
            &swarm_persistence::EmailMessageSnapshot {
                integration_id: &message.integration_id,
                message_id: &summary.id,
                conversation_id: &summary.conversation_id,
                internet_message_id: summary.internet_message_id.as_deref(),
                subject: &summary.subject,
                sender_name: &summary.sender_name,
                sender_address: &summary.sender_address,
                received_at: summary.received_at,
                web_url: &summary.web_url,
                body_text: &message.body_text,
                attachments: &attachments,
            },
            request.priority,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    let status = if imported.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(imported),
    )
        .into_response())
}

async fn import_email_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ImportEmailTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    validate_email_selection(&request.message_ids)?;
    let (messages, stored_by_message) =
        load_email_import_messages(&state, &request.message_ids).await?;
    let attachment_snapshots = stored_by_message
        .iter()
        .map(|stored| {
            stored
                .iter()
                .map(|attachment| swarm_persistence::EmailAttachmentSnapshot {
                    storage_name: &attachment.storage_name,
                    display_name: &attachment.display_name,
                    media_type: &attachment.media_type,
                    byte_size: attachment.byte_size,
                    inline: attachment.inline,
                    content_id: attachment.content_id.as_deref(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let snapshots = messages
        .iter()
        .zip(&attachment_snapshots)
        .map(|(message, attachments)| {
            let summary = &message.summary;
            swarm_persistence::EmailMessageSnapshot {
                integration_id: &message.integration_id,
                message_id: &summary.id,
                conversation_id: &summary.conversation_id,
                internet_message_id: summary.internet_message_id.as_deref(),
                subject: &summary.subject,
                sender_name: &summary.sender_name,
                sender_address: &summary.sender_address,
                received_at: summary.received_at,
                web_url: &summary.web_url,
                body_text: &message.body_text,
                attachments,
            }
        })
        .collect::<Vec<_>>();
    let imported = task_store(&state)?
        .import_email_messages(
            &snapshots,
            &swarm_persistence::EmailTaskDraft {
                title: &request.title,
                description: &request.description,
                priority: request.priority,
                worker_id: request.worker_id,
                state: request.state.unwrap_or(TaskState::Draft),
            },
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let status = if imported.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(imported),
    )
        .into_response())
}

fn validate_email_selection(message_ids: &[String]) -> Result<(), ApiError> {
    if message_ids.is_empty() || message_ids.len() > 20 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_email_selection",
            "choose between 1 and 20 Inbox messages",
        ));
    }
    let unique = message_ids.iter().collect::<HashSet<_>>();
    if unique.len() != message_ids.len() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_email_selection",
            "each Inbox message may be selected only once",
        ));
    }
    Ok(())
}

async fn load_email_import_messages(
    state: &AppState,
    message_ids: &[String],
) -> Result<
    (
        Vec<outlook::OutlookMessage>,
        Vec<Vec<StoredEmailAttachment>>,
    ),
    ApiError,
> {
    let outlook = state.outlook.read().await.clone();
    let attachment_store = state.email_attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_attachment_store_unconfigured",
            "private email attachment storage is not configured",
        )
    })?;
    let mut messages = Vec::with_capacity(message_ids.len());
    let mut stored_by_message = Vec::with_capacity(message_ids.len());
    for message_id in message_ids {
        let message = outlook.message(message_id).await.map_err(outlook_error)?;
        let mut stored = Vec::with_capacity(message.attachments.len());
        for attachment in &message.attachments {
            let content = outlook
                .attachment(message_id, &attachment.id)
                .await
                .map_err(outlook_error)?;
            let storage_name = attachment_store
                .save(&content.metadata.media_type, &content.bytes)
                .await
                .map_err(email_attachment_error)?;
            stored.push(StoredEmailAttachment {
                storage_name,
                display_name: content.metadata.name,
                media_type: content.metadata.media_type,
                byte_size: content.bytes.len() as u64,
                inline: content.metadata.inline,
                content_id: content.metadata.content_id,
            });
        }
        messages.push(message);
        stored_by_message.push(stored);
    }
    Ok((messages, stored_by_message))
}

async fn email_task_source(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let source = task_store(&state)?
        .email_task_link(task_id)
        .map_err(|error| task_store_error(&error))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "email_source_not_found",
                "this task was not imported from email",
            )
        })?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(source)).into_response())
}

async fn email_task_sources(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let sources = task_store(&state)?
        .email_task_links()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(sources)).into_response())
}

async fn download_email_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((task_id, storage_name)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let sources = task_store(&state)?
        .email_task_links_for_task(task_id)
        .map_err(|error| task_store_error(&error))?;
    if sources.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "email_source_not_found",
            "this task was not imported from email",
        ));
    }
    let attachment = sources
        .iter()
        .flat_map(|source| &source.attachments)
        .find(|attachment| attachment.storage_name == storage_name)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "email_attachment_not_found",
                "the private email attachment was not found",
            )
        })?;
    let store = state.email_attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_attachment_store_unconfigured",
            "private email attachment storage is not configured",
        )
    })?;
    let (bytes, media_type) = store
        .read(&storage_name)
        .await
        .map_err(email_attachment_error)?;
    let safe_name = attachment
        .display_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(120)
        .collect::<String>();
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&media_type).map_err(|_| {
            email_attachment_error(email_attachments::EmailAttachmentError::Unavailable)
        })?,
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{safe_name}\"")).map_err(|_| {
            email_attachment_error(email_attachments::EmailAttachmentError::Unavailable)
        })?,
    );
    Ok((response_headers, bytes).into_response())
}

async fn task_deployments(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let deployments = task_store(&state)?
        .task_deployments(parse_task_id(&task_id)?)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(deployments)).into_response())
}

async fn record_task_deployment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<RecordDeploymentRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let record = task_store(&state)?
        .record_task_deployment(
            parse_task_id(&task_id)?,
            &request.environment,
            &request.reference,
            request.deployed_at.unwrap_or_else(unix_timestamp),
        )
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(record),
    )
        .into_response())
}

async fn email_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reply = task_store(&state)?
        .email_reply_for_task(parse_task_id(&task_id)?)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(reply)).into_response())
}

async fn prepare_email_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<EmailReplyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reply = task_store(&state)?
        .prepare_email_reply(parse_task_id(&task_id)?, &request.body)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(reply),
    )
        .into_response())
}

async fn update_email_reply_draft(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<EmailReplyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reply = task_store(&state)?
        .update_email_reply_draft(parse_task_id(&task_id)?, &request.body)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(reply)).into_response())
}

async fn send_email_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(reply_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let queued = task_store(&state)?
        .queue_email_reply(&reply_id)
        .map_err(|error| task_store_error(&error))?;
    state.deliver_email_replies().await;
    let reply = task_store(&state)?
        .email_reply_for_task(queued.task_id)
        .map_err(|error| task_store_error(&error))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "email_reply_not_found",
                "the email reply was not found",
            )
        })?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(reply),
    )
        .into_response())
}

async fn retry_email_reply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(reply_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let queued = task_store(&state)?
        .retry_uncertain_email_reply(&reply_id)
        .map_err(|error| task_store_error(&error))?;
    state.deliver_email_replies().await;
    let reply = task_store(&state)?
        .email_reply_for_task(queued.task_id)
        .map_err(|error| task_store_error(&error))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "email_reply_not_found",
                "the email reply was not found",
            )
        })?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(reply),
    )
        .into_response())
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

fn federation_endpoint_is_remotely_reachable(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    url.scheme() == "https" && !loopback
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
        ApplicationError::SharedWorkBackendUnavailable => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "apiary_backend_unavailable",
            error.to_string(),
        ),
        ApplicationError::Store(error) => task_store_error(&error),
    }
}

fn federation_catalog_error(error: ApplicationError) -> ApiError {
    match error {
        ApplicationError::Store(
            TaskStoreError::InvalidFederationCredential
            | TaskStoreError::ApiaryKeeperRequired
            | TaskStoreError::ApiaryNotFound,
        ) => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_federation_credential",
            "a current federation node credential is required",
        ),
        other => application_error(other),
    }
}

fn federation_bootstrap_error(error: ApplicationError) -> ApiError {
    match error {
        ApplicationError::Store(
            TaskStoreError::InvalidApiaryJoinLink | TaskStoreError::ApiaryJoinLinkNotFound,
        ) => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_apiary_join_link",
            "this Apiary invitation link is invalid or expired",
        ),
        other => application_error(other),
    }
}

fn federation_sync_condition(
    error: federation_http::FederationHttpError,
) -> FederationSyncCondition {
    use federation_http::FederationHttpError;
    match error {
        FederationHttpError::TransportUnavailable => FederationSyncCondition::Offline,
        FederationHttpError::AuthenticationRejected => {
            FederationSyncCondition::AuthenticationRequired
        }
        FederationHttpError::InvalidEndpoint
        | FederationHttpError::Conflict
        | FederationHttpError::RemoteRejected(_)
        | FederationHttpError::ResponseTooLarge
        | FederationHttpError::InvalidResponse => FederationSyncCondition::Incompatible,
    }
}

fn record_federation_failure(
    service: &ApiaryService,
    condition: FederationSyncCondition,
    now: i64,
    notify: &Notify,
) {
    match service.record_federation_sync_failure(condition, now) {
        Ok(_) => notify.notify_waiters(),
        Err(error) => {
            tracing::warn!(%error, "federation reconciliation failure could not be persisted");
        }
    }
}

async fn reconcile_federation_tasks(
    service: &ApiaryService,
    client: &federation_http::FederationHttpClient,
    node_credential: &str,
    now: i64,
) -> Result<(), FederationSyncCondition> {
    let commands = service
        .pending_federation_task_commands(swarm_persistence::MAX_FEDERATION_TASK_COMMAND_BATCH)
        .map_err(|error| {
            tracing::warn!(%error, "federation task outbox could not be read");
            FederationSyncCondition::Incompatible
        })?;
    for entry in commands {
        service
            .record_federation_task_command_attempt(entry.command.id, now)
            .map_err(|error| {
                tracing::warn!(%error, "federation task attempt could not be recorded");
                FederationSyncCondition::Incompatible
            })?;
        let receipt = client
            .submit_task_command(node_credential, &entry.command)
            .await
            .map_err(federation_sync_condition)?;
        service
            .apply_federation_task_command_receipt(&receipt, now)
            .map_err(|error| {
                tracing::warn!(%error, "Keeper returned an incompatible task receipt");
                FederationSyncCondition::Incompatible
            })?;
    }
    let mut cursor = service.federation_task_sync_status().map_or_else(
        |error| {
            tracing::warn!(%error, "federation task cursor could not be read");
            Err(FederationSyncCondition::Incompatible)
        },
        |status| Ok(status.cursor),
    )?;
    for _ in 0..10 {
        let page = client
            .tasks(node_credential, cursor)
            .await
            .map_err(federation_sync_condition)?;
        let has_more = page.has_more;
        cursor = service
            .apply_federation_task_page(&page, now)
            .map_err(|error| {
                tracing::warn!(%error, "Keeper returned an incompatible federation task page");
                FederationSyncCondition::Incompatible
            })?
            .cursor;
        if !has_more {
            break;
        }
    }
    Ok(())
}

fn federation_http_error(error: federation_http::FederationHttpError) -> ApiError {
    use federation_http::FederationHttpError;
    match error {
        FederationHttpError::InvalidEndpoint => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_keeper_endpoint",
            error.to_string(),
        ),
        FederationHttpError::AuthenticationRejected => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "apiary_invitation_rejected",
            "the Keeper rejected this invitation link",
        ),
        FederationHttpError::Conflict => ApiError::new(
            StatusCode::CONFLICT,
            "apiary_bootstrap_conflict",
            error.to_string(),
        ),
        FederationHttpError::TransportUnavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "keeper_unavailable",
            "the Keeper is temporarily unreachable; this Hive will keep the pending invitation",
        ),
        FederationHttpError::RemoteRejected(_)
        | FederationHttpError::ResponseTooLarge
        | FederationHttpError::InvalidResponse => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "keeper_response_invalid",
            error.to_string(),
        ),
    }
}

fn federation_claim_error(error: ApplicationError) -> ApiError {
    match error {
        ApplicationError::Store(
            TaskStoreError::InvalidFederationCredential
            | TaskStoreError::ApiaryKeeperRequired
            | TaskStoreError::ApiaryNotFound,
        ) => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_federation_credential",
            "a current federation node credential is required",
        ),
        other => application_error(other),
    }
}

fn parse_federation_claim_id(value: &str) -> Result<FederationClaimId, ApiError> {
    FederationClaimId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_federation_claim",
            "federation claim ID must be a UUID",
        )
    })
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

fn email_oauth_error(error: microsoft_oauth::OAuthError) -> ApiError {
    match error {
        microsoft_oauth::OAuthError::NotConnected => ApiError::new(
            StatusCode::CONFLICT,
            "email_not_connected",
            "connect Microsoft Outlook before continuing",
        ),
        microsoft_oauth::OAuthError::CredentialsInvalid => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "email_oauth_invalid",
            "Microsoft authorization needs to be renewed",
        ),
        microsoft_oauth::OAuthError::PermissionDenied => ApiError::new(
            StatusCode::FORBIDDEN,
            "email_oauth_denied",
            "Microsoft did not grant the required mailbox access",
        ),
        microsoft_oauth::OAuthError::NetworkUnavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_oauth_unavailable",
            "Microsoft authorization is temporarily unavailable",
        ),
        microsoft_oauth::OAuthError::InvalidState => ApiError::new(
            StatusCode::BAD_REQUEST,
            "email_oauth_state_invalid",
            "This email connection attempt expired or was already used",
        ),
        microsoft_oauth::OAuthError::InvalidResponse | microsoft_oauth::OAuthError::Storage => {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "email_oauth_failed",
                "Microsoft authorization could not be stored safely",
            )
        }
    }
}

fn outlook_error(error: outlook::OutlookError) -> ApiError {
    match error {
        outlook::OutlookError::NotConfigured => ApiError::new(
            StatusCode::CONFLICT,
            "email_not_connected",
            "connect Microsoft Outlook before browsing Inbox",
        ),
        outlook::OutlookError::CredentialsInvalid => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "email_credentials_invalid",
            "Microsoft authorization needs to be renewed",
        ),
        outlook::OutlookError::PermissionDenied => ApiError::new(
            StatusCode::FORBIDDEN,
            "email_access_denied",
            "Microsoft denied access to this mailbox",
        ),
        outlook::OutlookError::NetworkUnavailable => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_network_unavailable",
            "Microsoft Outlook is temporarily unavailable",
        ),
        outlook::OutlookError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "email_message_not_found",
            "The email message is no longer available",
        ),
        outlook::OutlookError::InvalidRequest => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_email_request",
            "The email request was invalid",
        ),
        outlook::OutlookError::InvalidResponse => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "email_response_invalid",
            "Microsoft Outlook returned an invalid response",
        ),
        outlook::OutlookError::ResponseLimitExceeded => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "email_response_limit_exceeded",
            "Microsoft Outlook returned more data than this bounded operation permits",
        ),
        outlook::OutlookError::UnsupportedAttachment => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "email_attachment_unsupported",
            "This message contains an attachment type Swarm cannot safely import yet",
        ),
        outlook::OutlookError::AmbiguousDelivery => ApiError::new(
            StatusCode::CONFLICT,
            "email_delivery_uncertain",
            "Microsoft may have accepted this reply; review the original thread before retrying",
        ),
    }
}

fn email_attachment_error(error: email_attachments::EmailAttachmentError) -> ApiError {
    let (status, code) = match error {
        email_attachments::EmailAttachmentError::InvalidSize
        | email_attachments::EmailAttachmentError::InvalidSignature => {
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_email_attachment")
        }
        email_attachments::EmailAttachmentError::Capacity => (
            StatusCode::INSUFFICIENT_STORAGE,
            "email_attachment_store_full",
        ),
        email_attachments::EmailAttachmentError::NotFound => {
            (StatusCode::NOT_FOUND, "email_attachment_not_found")
        }
        email_attachments::EmailAttachmentError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "email_attachment_store_unavailable",
        ),
    };
    ApiError::new(status, code, error.to_string())
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
        TaskStoreError::InvalidApiaryJoinLink => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_apiary_join_link",
            error.to_string(),
        ),
        TaskStoreError::ApiaryJoinLinkNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "apiary_join_link_not_found",
            error.to_string(),
        ),
        TaskStoreError::ApiaryJoinLinkResolved => ApiError::new(
            StatusCode::CONFLICT,
            "apiary_join_link_resolved",
            error.to_string(),
        ),
        TaskStoreError::ApiaryJoinLinkLimit => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "apiary_join_link_limit",
            error.to_string(),
        ),
        TaskStoreError::InvalidApiary => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_apiary", error.to_string())
        }
        TaskStoreError::InvalidHiveIdentity => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_hive_identity",
            error.to_string(),
        ),
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
        TaskStoreError::ApiaryCollapseNotReady => ApiError::new(
            StatusCode::CONFLICT,
            "apiary_collapse_not_ready",
            error.to_string(),
        ),
        TaskStoreError::ApiaryProjectPromotionNotReady => ApiError::new(
            StatusCode::CONFLICT,
            "apiary_project_promotion_not_ready",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationConnectionCard => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_hive_connection_card",
            error.to_string(),
        ),
        TaskStoreError::ApiaryKeeperRequired => ApiError::new(
            StatusCode::FORBIDDEN,
            "apiary_keeper_required",
            error.to_string(),
        ),
        TaskStoreError::InvalidStewardship => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_stewardship",
            error.to_string(),
        ),
        TaskStoreError::StewardshipNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "stewardship_not_found",
            error.to_string(),
        ),
        TaskStoreError::HiveCandidateIdentityConflict => ApiError::new(
            StatusCode::CONFLICT,
            "hive_identity_conflict",
            error.to_string(),
        ),
        TaskStoreError::HiveCandidateNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "hive_candidate_not_found",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationInvitation => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_federation_invitation",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationCredential => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_federation_credential",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationCatalog => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_federation_catalog",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationClaim => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_federation_claim",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationSync => ApiError::new(
            StatusCode::FORBIDDEN,
            "apiary_member_required",
            error.to_string(),
        ),
        TaskStoreError::InvalidFederationTask => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_federation_task",
            error.to_string(),
        ),
        TaskStoreError::FederationClaimConflict => ApiError::new(
            StatusCode::CONFLICT,
            "federation_claim_conflict",
            error.to_string(),
        ),
        TaskStoreError::FederationInvitationConflict => ApiError::new(
            StatusCode::CONFLICT,
            "federation_invitation_conflict",
            error.to_string(),
        ),
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
        TaskStoreError::InvalidEmailMessage
        | TaskStoreError::InvalidEmailAttachment
        | TaskStoreError::InvalidTaskDeployment
        | TaskStoreError::InvalidEmailReply => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_email_workflow",
            error.to_string(),
        ),
        TaskStoreError::EmailMergeConflict => ApiError::new(
            StatusCode::CONFLICT,
            "email_merge_conflict",
            error.to_string(),
        ),
        TaskStoreError::EmailSourceNotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "email_source_not_found",
            error.to_string(),
        ),
        TaskStoreError::EmailReplyNotReady | TaskStoreError::EmailReplyAlreadyExists => {
            ApiError::new(
                StatusCode::CONFLICT,
                "email_reply_not_ready",
                error.to_string(),
            )
        }
        TaskStoreError::EmailReplyQueueFull => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "email_reply_queue_full",
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
        | TaskStoreError::InvalidFederationIdentity
        | TaskStoreError::FederationEntropyUnavailable
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

fn federation_node_credential(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_federation_credential",
                "a current federation node credential is required",
            )
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

fn parse_apiary_join_link_id(value: &str) -> Result<ApiaryJoinLinkId, ApiError> {
    ApiaryJoinLinkId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_apiary_join_link_id",
            "Apiary join link ID must be a UUID",
        )
    })
}

fn parse_hive_id(value: &str) -> Result<HiveId, ApiError> {
    HiveId::from_str(value).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_hive_id",
            "Hive ID must be a UUID",
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
        assert_eq!(json["worker_engine_build_id"], worker_engine_build_id());
    }

    #[tokio::test]
    async fn development_reload_is_authenticated_explicit_and_content_free() {
        let runtime = tempfile::tempdir().unwrap();
        let request_path = runtime.path().join("development-reload.request");
        let status_path = runtime.path().join("development-reload.status");
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_development_reload_paths(request_path.clone(), status_path.clone()),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/runtime/development")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let status = authorized_get(app.clone(), "/api/v1/runtime/development").await;
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(status.headers()[header::CACHE_CONTROL], "no-store");
        let status = response_json(status).await;
        assert_eq!(status["enabled"], true);
        assert_eq!(status["version"], build_version());
        assert_eq!(status["reload_available"], false);

        let requested = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/development/reload")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(requested.status(), StatusCode::ACCEPTED);
        let request = std::fs::read_to_string(request_path).unwrap();
        assert!(request.contains("requested_at="));
        assert!(request.contains(&format!("source_version={}", build_version())));
        assert!(!request.contains("secret"));
        assert_eq!(
            std::fs::read_to_string(status_path).unwrap(),
            "state=requested\n"
        );

        let duplicate = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/development/reload")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(duplicate).await["code"],
            "development_reload_in_progress"
        );
    }

    #[test]
    fn development_versions_expose_the_source_revision() {
        assert_eq!(
            deployed_source_revision("0.1.0-dev-d85e1e875ce2-20260815003728-2607939").as_deref(),
            Some("d85e1e875ce2")
        );
        assert_eq!(
            deployed_source_revision("0.1.0-a5d95af96bee").as_deref(),
            Some("a5d95af96bee")
        );
        assert_eq!(deployed_source_revision("0.1.0"), None);
    }

    #[test]
    fn development_reload_only_tracks_product_changes() {
        let checkout = TempDir::new().unwrap();
        let checkout_path = checkout.path();
        std::fs::create_dir_all(checkout_path.join("web/src")).unwrap();
        std::fs::create_dir_all(checkout_path.join("docs")).unwrap();
        std::fs::create_dir_all(checkout_path.join("scripts")).unwrap();
        std::fs::write(checkout_path.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(checkout_path.join("web/src/main.ts"), "export {};\n").unwrap();
        std::fs::write(checkout_path.join("docs/guide.md"), "first\n").unwrap();
        std::fs::write(checkout_path.join("scripts/check.sh"), "exit 0\n").unwrap();

        let git = |arguments: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(checkout_path)
                .args(arguments)
                .status()
                .unwrap();
            assert!(status.success(), "git {arguments:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Swarm Test",
            "-c",
            "user.email=swarm-test@example.com",
            "commit",
            "--quiet",
            "-m",
            "base",
        ]);
        let deployed_revision = git_output(checkout_path, &["rev-parse", "HEAD"]).unwrap();

        std::fs::write(checkout_path.join("docs/guide.md"), "second\n").unwrap();
        std::fs::write(checkout_path.join("scripts/check.sh"), "exit 1\n").unwrap();
        let docs_only = development_source_status_for(checkout_path, Some(&deployed_revision))
            .expect("development checkout should be readable");
        assert!(!docs_only.dirty);
        assert!(!docs_only.reload_available);

        git(&["add", "docs", "scripts"]);
        git(&[
            "-c",
            "user.name=Swarm Test",
            "-c",
            "user.email=swarm-test@example.com",
            "commit",
            "--quiet",
            "-m",
            "docs only",
        ]);
        let committed_docs =
            development_source_status_for(checkout_path, Some(&deployed_revision)).unwrap();
        assert!(!committed_docs.dirty);
        assert!(!committed_docs.reload_available);

        std::fs::write(
            checkout_path.join("web/src/main.ts"),
            "export const changed = true;\n",
        )
        .unwrap();
        let dirty_product =
            development_source_status_for(checkout_path, Some(&deployed_revision)).unwrap();
        assert!(dirty_product.dirty);
        assert!(dirty_product.reload_available);

        git(&["add", "web"]);
        git(&[
            "-c",
            "user.name=Swarm Test",
            "-c",
            "user.email=swarm-test@example.com",
            "commit",
            "--quiet",
            "-m",
            "product change",
        ]);
        let committed_product =
            development_source_status_for(checkout_path, Some(&deployed_revision)).unwrap();
        assert!(!committed_product.dirty);
        assert!(committed_product.reload_available);
    }

    #[tokio::test]
    async fn development_reload_fails_closed_when_not_enabled() {
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret"),
        );
        let status = authorized_get(app.clone(), "/api/v1/runtime/development").await;
        assert_eq!(response_json(status).await["enabled"], false);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/development/reload")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response_json(response).await["code"],
            "development_reload_unavailable"
        );
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
    async fn local_hive_and_keeper_apiary_names_are_private_bounded_commands() {
        let store = TaskStore::in_memory().unwrap();
        let original_hive_id = store.local_hive_identity().unwrap().hive.id;
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone()),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/hive")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Clover House"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let invalid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/hive")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(invalid).await["code"],
            "invalid_hive_identity"
        );

        let renamed_hive = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/hive")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"  Clover House  "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(renamed_hive.status(), StatusCode::OK);
        assert_eq!(renamed_hive.headers()[header::CACHE_CONTROL], "no-store");
        let renamed_hive = response_json(renamed_hive).await;
        assert_eq!(renamed_hive["hive"]["name"], "Clover House");
        assert_eq!(renamed_hive["hive"]["id"], original_hive_id.to_string());
        assert_eq!(renamed_hive["apiary_context"]["mode"], "personal");

        store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let renamed_apiary = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/apiary")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Grand Garden"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = renamed_apiary.status();
        let cache_control = renamed_apiary.headers().get(header::CACHE_CONTROL).cloned();
        let renamed_apiary = response_json(renamed_apiary).await;
        assert_eq!(status, StatusCode::OK, "{renamed_apiary}");
        assert_eq!(cache_control.unwrap(), "no-store");
        assert_eq!(renamed_apiary["apiary"]["name"], "Grand Garden");
        assert_eq!(renamed_apiary["apiary"]["shared_work_backend"], "jira");
        assert_eq!(renamed_apiary["apiary"]["policy_revision"], 1);
    }

    #[tokio::test]
    async fn apiary_member_roster_is_private_no_store_and_public_identity_only() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        store
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                unix_timestamp(),
            )
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
                    .uri("/api/v1/apiary/members")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/apiary/members").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        let members = json.as_array().unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["hive_id"], identity.hive.id.to_string());
        assert_eq!(members[0]["role"], "keeper");
        assert_eq!(members[0]["is_local"], true);
        assert!(members[0].get("node_credential").is_none());
        assert!(members[0].get("receipt").is_none());
    }

    #[tokio::test]
    async fn hive_connection_card_is_private_signed_and_downloadable() {
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
                    .uri("/api/v1/apiary/connection-card")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/apiary/connection-card").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=swarm-next-hive-connection.json"
        );
        let card: swarm_domain::HiveConnectionCard =
            serde_json::from_value(response_json(response).await).unwrap();
        swarm_persistence::verify_hive_connection_card(&card, unix_timestamp()).unwrap();
    }

    #[tokio::test]
    async fn federation_transport_readiness_distinguishes_remote_local_and_missing_urls() {
        let state = |endpoint: Option<&str>| {
            let state = AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret");
            match endpoint {
                Some(endpoint) => state.with_public_base_url(endpoint),
                None => Ok(state),
            }
        };

        let missing = authorized_get(
            router(state(None).unwrap()),
            "/api/v1/apiary/transport-readiness",
        )
        .await;
        assert_eq!(missing.status(), StatusCode::OK);
        let missing = response_json(missing).await;
        assert_eq!(missing["configured"], false);
        assert_eq!(missing["reachability"], "unconfigured");
        assert_eq!(missing["endpoint"], Value::Null);

        let local = authorized_get(
            router(state(Some("http://127.0.0.1:8766")).unwrap()),
            "/api/v1/apiary/transport-readiness",
        )
        .await;
        let local = response_json(local).await;
        assert_eq!(local["configured"], true);
        assert_eq!(local["reachability"], "local_only");

        let remote = authorized_get(
            router(state(Some("https://swarm2.example.test")).unwrap()),
            "/api/v1/apiary/transport-readiness",
        )
        .await;
        let remote = response_json(remote).await;
        assert_eq!(remote["reachability"], "remote_https");
        assert_eq!(remote["endpoint"], "https://swarm2.example.test");
    }

    #[tokio::test]
    async fn keeper_link_bootstrap_is_public_secret_bound_and_approval_gated() {
        let now = unix_timestamp();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(keeper)
                .with_public_base_url("https://keeper.example.test/swarm")
                .unwrap(),
        );
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/join-links")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
        let bundle: swarm_domain::ApiaryJoinLinkBundle =
            serde_json::from_value(response_json(created).await).unwrap();

        let remote = TaskStore::in_memory().unwrap();
        let card = remote.issue_hive_connection_card(now, 3_600).unwrap();
        let bootstrap_uri = format!("/api/v1/federation/bootstrap/{}", bundle.link.id);
        let presented = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&bootstrap_uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "secret": &bundle.one_time_secret,
                            "connection_card": card,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(presented.status(), StatusCode::OK);
        let waiting = response_json(presented).await;
        assert_eq!(waiting["link"]["state"], "awaiting_approval");
        assert!(waiting["invitation"].is_null());

        let approved = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/join-links/{}/approval",
                        bundle.link.id
                    ))
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);

        let issued = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&bootstrap_uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({"secret": &bundle.one_time_secret}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issued.status(), StatusCode::OK);
        assert_eq!(issued.headers()[header::CACHE_CONTROL], "no-store");
        let issued = response_json(issued).await;
        assert_eq!(issued["link"]["state"], "invitation_issued");
        assert_eq!(
            issued["invitation"]["invitation"]["payload"]["invited_hive_id"],
            card.payload.hive_id.to_string()
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn member_keeper_link_polls_outbound_and_imports_the_approved_invitation() {
        let now = unix_timestamp();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let keeper_endpoint = format!("http://{address}");
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        let bundle = keeper
            .issue_apiary_join_link(&keeper_endpoint, now, 3_600)
            .unwrap();
        let keeper_app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(keeper)
                .with_public_base_url(&keeper_endpoint)
                .unwrap(),
        );
        let keeper_server = tokio::spawn({
            let app = keeper_app.clone();
            async move { axum::serve(listener, app).await }
        });

        let member = TaskStore::in_memory().unwrap();
        let member_app = router(
            AppState::default()
                .with_terminal_host(
                    HostClient::new("/unreachable/terminal.sock"),
                    "member-secret",
                )
                .with_task_store(member),
        );
        let saved = member_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/keeper-links")
                    .header(header::AUTHORIZATION, "Bearer member-secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "link_id": bundle.link.id,
                            "keeper_endpoint": keeper_endpoint,
                            "secret": bundle.one_time_secret,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saved.status(), StatusCode::CREATED);
        let saved = response_json(saved).await;
        assert_eq!(saved["link"]["state"], "awaiting_approval");
        assert_eq!(saved["invitation_received"], false);

        let approved = keeper_app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/join-links/{}/approval",
                        bundle.link.id
                    ))
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);

        let polled = member_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/keeper-links/{}/poll",
                        bundle.link.id
                    ))
                    .header(header::AUTHORIZATION, "Bearer member-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(polled.status(), StatusCode::OK);
        let polled = response_json(polled).await;
        assert_eq!(polled["link"]["state"], "invitation_issued");
        assert_eq!(polled["invitation_received"], true);

        let pending = authorized_get_with_token(
            member_app.clone(),
            "/api/v1/apiary/keeper-links",
            "member-secret",
        )
        .await;
        assert_eq!(response_json(pending).await, serde_json::json!([]));
        let invitations = authorized_get_with_token(
            member_app,
            "/api/v1/apiary/join-invitations",
            "member-secret",
        )
        .await;
        assert_eq!(
            response_json(invitations).await.as_array().unwrap().len(),
            1
        );

        keeper_server.abort();
        let _ = keeper_server.await;
    }

    #[tokio::test]
    async fn keeper_pins_a_remote_hive_identity_without_creating_membership() {
        let now = unix_timestamp();
        let remote = TaskStore::in_memory().unwrap();
        let remote_identity = remote.local_hive_identity().unwrap();
        let card = remote.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(keeper.clone()),
        );
        let body = serde_json::to_string(&card).unwrap();

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/hive-candidates")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let pinned = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/hive-candidates")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pinned.status(), StatusCode::CREATED);
        assert_eq!(pinned.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(pinned).await;
        assert_eq!(json["hive_id"], remote_identity.hive.id.to_string());

        let listed = authorized_get(app, "/api/v1/apiary/hive-candidates").await;
        assert_eq!(listed.status(), StatusCode::OK);
        assert_eq!(listed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(listed).await.as_array().unwrap().len(), 1);
        let apiary_id = keeper
            .local_hive_identity()
            .unwrap()
            .hive
            .apiary_id
            .unwrap();
        assert_eq!(
            keeper
                .apiary_collapse_readiness(apiary_id)
                .unwrap()
                .active_hive_count,
            1
        );
    }

    #[tokio::test]
    async fn keeper_downloads_one_authenticated_invitation_for_the_pinned_hive() {
        let now = unix_timestamp();
        let remote = TaskStore::in_memory().unwrap();
        let card = remote.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&card, now).unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(keeper.clone())
            .with_public_base_url("https://keeper.example.test/swarm")
            .unwrap();
        let app = router(state);
        let uri = format!(
            "/api/v1/apiary/hive-candidates/{}/invitation",
            candidate.hive_id
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=swarm-next-apiary-invitation.json"
        );
        let bundle: swarm_domain::ApiaryInvitationBundle =
            serde_json::from_value(response_json(response).await).unwrap();
        assert_eq!(bundle.invitation.payload.invited_hive_id, candidate.hive_id);
        assert_eq!(
            bundle.invitation.payload.keeper_endpoint,
            "https://keeper.example.test/swarm"
        );
        swarm_persistence::verify_apiary_invitation_envelope(
            &bundle.invitation,
            &bundle.keeper_connection_card.payload.public_key,
            now,
        )
        .unwrap();
        assert_eq!(
            keeper
                .pending_federation_invitation_count(candidate.hive_id, now)
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn exact_invited_hive_privately_pins_keeper_without_joining() {
        let now = unix_timestamp();
        let invited = TaskStore::in_memory().unwrap();
        let invited_card = invited.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        let candidate = keeper.pin_hive_candidate(&invited_card, now).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                candidate.hive_id,
                "https://keeper.example.test/swarm",
                now,
                3_600,
            )
            .unwrap();
        let bundle_json = serde_json::to_string(&bundle).unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(invited.clone()),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/join-invitations")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(bundle_json.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let imported = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/join-invitations")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(bundle_json))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(imported.status(), StatusCode::CREATED);
        assert_eq!(imported.headers()[header::CACHE_CONTROL], "no-store");
        let imported_json = response_json(imported).await;
        let serialized = imported_json.to_string();
        assert_eq!(imported_json["state"], "keeper_pinned");
        assert_eq!(imported_json["promoted_projects"], serde_json::json!([]));
        assert!(!serialized.contains(&bundle.one_time_secret));
        assert!(!serialized.contains(&bundle.keeper_connection_card.payload.public_key));
        assert!(
            invited
                .local_hive_identity()
                .unwrap()
                .hive
                .apiary_id
                .is_none()
        );

        let listed = authorized_get(app.clone(), "/api/v1/apiary/join-invitations").await;
        assert_eq!(listed.status(), StatusCode::OK);
        assert_eq!(listed.headers()[header::CACHE_CONTROL], "no-store");
        let listed_json = response_json(listed).await;
        assert_eq!(listed_json.as_array().unwrap().len(), 1);
        assert_eq!(listed_json[0]["state"], "keeper_pinned");
        assert_eq!(
            listed_json[0]["readiness"]["projects"],
            serde_json::json!([])
        );
        assert!(
            listed_json[0]["readiness"]["blockers"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("policy_not_accepted"))
        );

        assert_imported_policy_acceptance(app, bundle.invitation.payload.invitation_id).await;
    }

    async fn start_ready_jira_test_server() -> std::net::SocketAddr {
        let jira_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let jira_address = jira_listener.local_addr().unwrap();
        let jira_server = axum::Router::new()
            .route(
                "/rest/api/3/project/search",
                get(|| async { Json(serde_json::json!({ "isLast": true, "values": [] })) }),
            )
            .route(
                "/rest/api/3/myself",
                get(|| async {
                    Json(serde_json::json!({
                        "accountId": "operator-1",
                        "displayName": "Bea"
                    }))
                }),
            );
        tokio::spawn(async move { axum::serve(jira_listener, jira_server).await.unwrap() });
        jira_address
    }

    async fn start_keeper_join_test_server(
        keeper: TaskStore,
    ) -> (String, tokio::task::JoinHandle<Result<(), std::io::Error>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = router(
            AppState::default()
                .with_terminal_host(
                    HostClient::new("/unreachable/terminal.sock"),
                    "keeper-secret",
                )
                .with_task_store(keeper),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        (endpoint, server)
    }

    #[test]
    fn federation_transport_failures_have_operator_meaningful_sync_states() {
        use federation_http::FederationHttpError;
        assert_eq!(
            federation_sync_condition(FederationHttpError::TransportUnavailable),
            FederationSyncCondition::Offline
        );
        assert_eq!(
            federation_sync_condition(FederationHttpError::AuthenticationRejected),
            FederationSyncCondition::AuthenticationRequired
        );
        assert_eq!(
            federation_sync_condition(FederationHttpError::InvalidResponse),
            FederationSyncCondition::Incompatible
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn invited_hive_joins_through_one_outbound_signed_request() {
        let jira_address = start_ready_jira_test_server().await;

        let now = unix_timestamp();
        let invited = TaskStore::in_memory().unwrap();
        let invited_card = invited.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        keeper
            .create_apiary_task(
                "Coordinate the release across Hives",
                "Swarm-generated work stays on Keeper.",
                TaskPriority::High,
                now,
            )
            .unwrap();
        keeper.pin_hive_candidate(&invited_card, now).unwrap();
        let (keeper_endpoint, keeper_server) = start_keeper_join_test_server(keeper.clone()).await;
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                invited_card.payload.hive_id,
                &keeper_endpoint,
                now,
                3_600,
            )
            .unwrap();
        let imported = invited
            .import_apiary_invitation_bundle(&bundle, now)
            .unwrap();
        invited
            .accept_federation_join_policy(imported.invitation_id, 1, now)
            .unwrap();
        let state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(invited.clone())
            .with_jira_configuration(
                &format!("http://{jira_address}"),
                "operator@example.test",
                "api-token",
            )
            .unwrap();
        let app = router(state.clone());
        let uri = format!(
            "/api/v1/apiary/join-invitations/{}/submission",
            imported.invitation_id
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json["mode"], "federated");
        assert_eq!(json["local_role"], "member");
        let serialized = json.to_string();
        assert!(!serialized.contains(&bundle.one_time_secret));
        assert!(!serialized.contains("signature"));
        assert!(!serialized.contains("credential"));
        assert!(matches!(
            invited.local_apiary_context().unwrap(),
            swarm_domain::LocalApiaryContext::Federated {
                local_role: swarm_domain::LocalApiaryRole::Member,
                ..
            }
        ));
        state.reconcile_federation().await;
        assert!(
            invited
                .federation_catalog_acknowledgement()
                .unwrap()
                .is_some()
        );
        assert_eq!(
            invited.federation_sync_health().unwrap().condition,
            FederationSyncCondition::Current
        );
        let task_sync = invited.federation_task_sync_status().unwrap();
        assert_eq!(task_sync.cursor, 1);
        assert_eq!(task_sync.task_count, 1);
        assert!(task_sync.last_applied_at.is_some());
        let projected = invited.list_local_apiary_tasks().unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].title, "Coordinate the release across Hives");

        keeper_server.abort();
        let _ = keeper_server.await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn signed_one_time_federation_join_is_publicly_consumed_once_without_browser_auth() {
        let now = unix_timestamp();
        let invited = TaskStore::in_memory().unwrap();
        let invited_identity = invited.local_hive_identity().unwrap();
        let invited_card = invited.issue_hive_connection_card(now, 3_600).unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive(
                "Wildflower Garden",
                SharedWorkBackend::Jira,
                now.saturating_sub(1),
            )
            .unwrap();
        keeper
            .create_apiary_task(
                "Prepare the shared release brief",
                "This is a Swarm task, not a Jira issue.",
                TaskPriority::Normal,
                now,
            )
            .unwrap();
        keeper.pin_hive_candidate(&invited_card, now).unwrap();
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                invited_identity.hive.id,
                "https://keeper.example.test/swarm",
                now,
                3_600,
            )
            .unwrap();
        let imported = invited
            .import_apiary_invitation_bundle(&bundle, now)
            .unwrap();
        invited
            .accept_federation_join_policy(imported.invitation_id, 1, now)
            .unwrap();
        let submission = invited
            .prepare_federation_join_submission(
                imported.invitation_id,
                &swarm_domain::FederationJoinReadiness {
                    jira_connection: JiraConnectionState::Ready,
                    projects: Vec::new(),
                    blockers: Vec::new(),
                },
                now,
            )
            .unwrap();
        let body = serde_json::to_string(&submission).unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(keeper.clone()),
        );

        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/federation/join")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);
        assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
        let first_json = response_json(first).await;
        assert_eq!(
            first_json["receipt"]["payload"]["member_hive_id"],
            invited_identity.hive.id.to_string()
        );
        assert!(first_json["node_credential"].as_str().unwrap().len() >= 43);

        let retry = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/federation/join")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retry.status(), StatusCode::CREATED);
        assert_eq!(response_json(retry).await, first_json);

        let credential = first_json["node_credential"].as_str().unwrap();
        assert_federation_catalog_endpoint(app.clone(), credential, invited_card.payload.node_id)
            .await;
        assert_federation_task_endpoint(app.clone(), credential, invited_card.payload.node_id)
            .await;
        let acceptance: swarm_domain::FederationJoinAcceptance =
            serde_json::from_value(first_json.clone()).unwrap();
        assert_stewardship_endpoints(
            app.clone(),
            invited_identity.operator.id,
            invited_identity.hive.id,
        )
        .await;
        assert_member_catalog_acknowledgement_endpoint(
            invited,
            &keeper,
            imported.invitation_id,
            &acceptance,
            now,
        )
        .await;
        assert_federation_claim_endpoints(app, &keeper, credential).await;
        assert_keeper_has_two_hives(&keeper);
    }

    async fn assert_stewardship_endpoints(
        app: Router,
        steward_operator_id: OperatorId,
        managed_hive_id: HiveId,
    ) {
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/apiary/stewardships")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let body = serde_json::json!({
            "managed_hive_ids": [managed_hive_id],
            "capabilities": ["observe", "assign", "assist", "takeover"]
        })
        .to_string();
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v1/apiary/stewardships/by-operator/{steward_operator_id}"
                    ))
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
        let created_json = response_json(created).await;
        assert_eq!(
            created_json["steward_operator_id"],
            steward_operator_id.to_string()
        );
        assert_eq!(
            created_json["managed_hive_ids"],
            serde_json::json!([managed_hive_id])
        );
        let serialized = created_json.to_string();
        for forbidden in ["credential", "endpoint", "repository", "terminal"] {
            assert!(!serialized.contains(forbidden));
        }

        let listed = authorized_get(app.clone(), "/api/v1/apiary/stewardships").await;
        assert_eq!(listed.status(), StatusCode::OK);
        assert_eq!(listed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response_json(listed).await,
            serde_json::json!([created_json])
        );

        let stewardship_id = created_json["id"].as_str().unwrap();
        let revoked = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/apiary/stewardships/{stewardship_id}"))
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
        assert_eq!(revoked.headers()[header::CACHE_CONTROL], "no-store");
        let listed = authorized_get(app, "/api/v1/apiary/stewardships").await;
        assert_eq!(response_json(listed).await, serde_json::json!([]));
    }

    fn assert_keeper_has_two_hives(keeper: &TaskStore) {
        let apiary_id = keeper
            .local_hive_identity()
            .unwrap()
            .hive
            .apiary_id
            .unwrap();
        assert_eq!(
            keeper
                .apiary_collapse_readiness(apiary_id)
                .unwrap()
                .active_hive_count,
            2
        );
    }

    async fn assert_federation_catalog_endpoint(
        app: Router,
        credential: &str,
        member_node_id: swarm_domain::FederationNodeId,
    ) {
        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/federation/catalog")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/federation/catalog")
                    .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(
            json["payload"]["member_node_id"],
            member_node_id.to_string()
        );
        let serialized = json.to_string();
        assert!(!serialized.contains("node_credential"));
        assert!(!serialized.contains("receipt"));
    }

    async fn assert_federation_task_endpoint(
        app: Router,
        credential: &str,
        member_node_id: swarm_domain::FederationNodeId,
    ) {
        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/federation/tasks?after=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        let request = || {
            Request::builder()
                .uri("/api/v1/federation/tasks?after=0")
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .body(Body::empty())
                .unwrap()
        };
        let first = app.clone().oneshot(request()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
        let first_json = response_json(first).await;
        assert_eq!(first_json["member_node_id"], member_node_id.to_string());
        assert_eq!(first_json["next_cursor"], 1);
        assert_eq!(first_json["has_more"], false);
        assert_eq!(first_json["events"].as_array().unwrap().len(), 1);
        assert_eq!(first_json["events"][0]["task"]["source"], "swarm");
        assert_eq!(
            first_json["events"][0]["task"]["title"],
            "Prepare the shared release brief"
        );
        let retry = app.oneshot(request()).await.unwrap();
        assert_eq!(response_json(retry).await, first_json);
        let serialized = first_json.to_string();
        for forbidden in [
            "jira_status",
            "jira_assignee",
            "node_credential",
            "endpoint",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    async fn assert_federation_claim_endpoints(app: Router, keeper: &TaskStore, credential: &str) {
        let apiary_id = keeper
            .local_hive_identity()
            .unwrap()
            .hive
            .apiary_id
            .unwrap();
        keeper
            .promote_apiary_jira_project(
                apiary_id,
                "10001",
                "WWD",
                "Website Development",
                keeper.local_hive_identity().unwrap().operator.id,
                unix_timestamp(),
            )
            .unwrap();
        let body = serde_json::json!({
            "project_id": "10001",
            "issue_id": "20001",
            "issue_key": "WWD-101"
        })
        .to_string();
        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/federation/claims")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let reserve = || {
            Request::builder()
                .method("POST")
                .uri("/api/v1/federation/claims")
                .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.clone()))
                .unwrap()
        };
        let reserved = app.clone().oneshot(reserve()).await.unwrap();
        assert_eq!(reserved.status(), StatusCode::CREATED);
        assert_eq!(reserved.headers()[header::CACHE_CONTROL], "no-store");
        let reserved_json = response_json(reserved).await;
        assert_eq!(reserved_json["state"], "reserved");
        assert!(!reserved_json.to_string().contains("credential"));

        let retry = app.clone().oneshot(reserve()).await.unwrap();
        assert_eq!(retry.status(), StatusCode::CREATED);
        assert_eq!(response_json(retry).await, reserved_json);

        let claim_id = reserved_json["id"].as_str().unwrap();
        let confirmed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/federation/claims/{claim_id}/confirmation"))
                    .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(confirmed.status(), StatusCode::OK);
        assert_eq!(confirmed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(confirmed).await["state"], "confirmed");

        assert_keeper_shared_work_rollup(app.clone()).await;

        let release_confirmed = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/federation/claims/{claim_id}"))
                    .header(header::AUTHORIZATION, format!("Bearer {credential}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(release_confirmed.status(), StatusCode::BAD_REQUEST);
    }

    async fn assert_keeper_shared_work_rollup(app: Router) {
        let unauthenticated = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/apiary/shared-work")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let response = authorized_get(app, "/api/v1/apiary/shared-work").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(response).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["issue_key"], "WWD-101");
        assert_eq!(json[0]["project_name"], "Website Development");
        assert_eq!(json[0]["state"], "confirmed");
        assert!(json[0]["home_hive_name"].as_str().is_some());
        assert!(json[0]["home_operator_display_name"].as_str().is_some());
        let serialized = json.to_string();
        assert!(!serialized.contains("credential"));
        assert!(!serialized.contains("receipt"));
    }

    async fn assert_member_catalog_acknowledgement_endpoint(
        invited: TaskStore,
        keeper: &TaskStore,
        invitation_id: ApiaryInvitationId,
        acceptance: &swarm_domain::FederationJoinAcceptance,
        now: i64,
    ) {
        invited
            .apply_federation_join_acceptance(invitation_id, acceptance, now)
            .unwrap();
        let snapshot = keeper
            .signed_federation_catalog(&acceptance.node_credential, now)
            .unwrap();
        let body = serde_json::to_string(&snapshot).unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(invited),
        );
        let sync_unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/apiary/sync-health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sync_unauthorized.status(), StatusCode::UNAUTHORIZED);
        let sync = authorized_get(app.clone(), "/api/v1/apiary/sync-health").await;
        assert_eq!(sync.status(), StatusCode::OK);
        assert_eq!(sync.headers()[header::CACHE_CONTROL], "no-store");
        let sync_json = response_json(sync).await;
        assert_eq!(sync_json["condition"], "idle");
        assert_eq!(sync_json["consecutive_failures"], 0);
        let serialized_sync = sync_json.to_string();
        for forbidden in ["credential", "endpoint", "receipt", "jira", "task"] {
            assert!(!serialized_sync.contains(forbidden));
        }
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/catalog-acknowledgement")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let acknowledged = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/catalog-acknowledgement")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(acknowledged.status(), StatusCode::ACCEPTED);
        assert_eq!(acknowledged.headers()[header::CACHE_CONTROL], "no-store");
        let acknowledged_json = response_json(acknowledged).await;
        assert_eq!(acknowledged_json["project_count"], 0);
        let current = authorized_get(app.clone(), "/api/v1/apiary/catalog-acknowledgement").await;
        assert_eq!(current.status(), StatusCode::OK);
        assert_eq!(current.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(current).await, acknowledged_json);
        let readiness = authorized_get(app, "/api/v1/apiary/catalog-readiness").await;
        assert_eq!(readiness.status(), StatusCode::OK);
        assert_eq!(readiness.headers()[header::CACHE_CONTROL], "no-store");
        let readiness_json = response_json(readiness).await;
        assert_eq!(readiness_json["projects"], serde_json::json!([]));
        assert_eq!(
            readiness_json["blockers"],
            serde_json::json!(["integration_not_ready"])
        );
    }

    async fn assert_imported_policy_acceptance(app: Router, invitation_id: ApiaryInvitationId) {
        let policy_uri =
            format!("/api/v1/apiary/join-invitations/{invitation_id}/policy-acceptance");
        let stale = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&policy_uri)
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"policy_revision":2}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        let accepted = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(policy_uri)
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"policy_revision":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        assert_eq!(accepted.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(accepted).await["state"], "policy_accepted");
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

        let unavailable_native = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Orchard","shared_work_backend":"native"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            unavailable_native.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            response_json(unavailable_native).await["code"],
            "apiary_backend_unavailable"
        );

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
    async fn sole_keeper_collapse_routes_are_private_and_return_to_personal_mode() {
        let store = TaskStore::in_memory().unwrap();
        store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
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
                    .uri("/api/v1/apiary/collapse-readiness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let readiness = authorized_get(app.clone(), "/api/v1/apiary/collapse-readiness").await;
        assert_eq!(readiness.status(), StatusCode::OK);
        assert_eq!(readiness.headers()[header::CACHE_CONTROL], "no-store");
        let json = response_json(readiness).await;
        assert_eq!(json["active_hive_count"], 1);
        assert_eq!(json["pending_invitation_count"], 0);

        let collapsed = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/apiary/collapse")
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(collapsed.status(), StatusCode::OK);
        assert_eq!(collapsed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(collapsed).await["mode"], "personal");
    }

    #[tokio::test]
    async fn keeper_jira_project_promotion_is_private_atomic_and_listed() {
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
                &[swarm_domain::JiraStatusMapping {
                    jira_status_id: "1".into(),
                    jira_status_name: "To Do".into(),
                    task_state: TaskState::Ready,
                }],
            )
            .unwrap();
        store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                .with_task_store(store.clone()),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/jira-projects/{}/promotion",
                        binding.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let promoted = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/apiary/jira-projects/{}/promotion",
                        binding.id
                    ))
                    .header(header::AUTHORIZATION, "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(promoted.status(), StatusCode::CREATED);
        assert_eq!(promoted.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response_json(promoted).await["project_key"], "WEB");
        assert_eq!(
            store.get_jira_project_binding(binding.id).unwrap().scope,
            JiraProjectScope::Apiary
        );

        let listed = authorized_get(app, "/api/v1/apiary/jira-projects").await;
        assert_eq!(listed.status(), StatusCode::OK);
        let json = response_json(listed).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["project_name"], "Website Services");
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
        assert_operator_activity(&activity);
        assert_eq!(activity["events"][2]["from_state"], "draft");
        assert_eq!(activity["events"][2]["to_state"], "ready");

        let recent = authorized_get(app.clone(), "/api/v1/tasks/activity?limit=10").await;
        assert_eq!(recent.status(), StatusCode::OK);
        assert_eq!(
            recent.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let recent = response_json(recent).await;
        assert_eq!(recent["events"].as_array().unwrap().len(), 3);
        assert_eq!(recent["events"][0]["task_id"], created["id"]);

        let listed = authorized_get(app, "/api/v1/tasks").await;
        let listed = response_json(listed).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["title"], "Recover every terminal");
    }

    fn assert_operator_activity(activity: &serde_json::Value) {
        assert!(
            activity["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| { entry["actor_kind"] == "operator" && entry["actor_id"].is_null() })
        );
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

        let response = authorized_get(
            router(
                AppState::default()
                    .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
                    .with_task_store(TaskStore::in_memory().unwrap()),
            ),
            "/api/v1/tasks/activity?limit=0",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
        let server = HostServer::bind_with_identity(
            &socket,
            Arc::clone(&registry),
            "old-host",
            "old-engine",
        )
        .unwrap();
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
            let replacement = HostServer::bind_with_identity(
                replacement_socket,
                replacement_registry,
                build_version(),
                worker_engine_build_id(),
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
        let status = response.status();
        let response = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{response:?}");
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
    async fn app_only_release_does_not_restart_a_matching_worker_engine() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 1, [workspace.clone()]).unwrap(),
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
        let server = HostServer::bind_with_identity(
            &socket,
            registry,
            "older-app-release",
            worker_engine_build_id(),
        )
        .unwrap();
        let server_task = tokio::spawn(server.run());
        tokio::task::yield_now().await;
        let maintenance_request = runtime.path().join("worker-engine-maintenance.request");
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new(&socket), "secret")
                .with_task_store(TaskStore::in_memory().unwrap())
                .with_maintenance_request_path(maintenance_request.clone()),
        );

        let response = app
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
        let status = response.status();
        let response = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{response:?}");
        assert_eq!(response["stopped_sessions"], 0);
        assert!(session.is_running().unwrap());
        assert!(!maintenance_request.exists());

        server_task.abort();
        let _ = server_task.await;
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
        authorized_get_with_token(app, uri, "secret").await
    }

    async fn authorized_get_with_token(app: Router, uri: &str, token: &str) -> Response {
        app.oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
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
    async fn email_readiness_is_private_and_explicit_when_not_configured() {
        let runtime = TempDir::new().unwrap();
        let app = router(AppState::default().with_terminal_host(
            HostClient::new(runtime.path().join("absent.sock")),
            "secret",
        ));
        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/integrations/email/readiness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let response = authorized_get(app, "/api/v1/integrations/email/readiness").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["configured"], false);
        assert_eq!(body["connection"], "not_connected");
        assert_eq!(body["account_address"], Value::Null);
    }

    #[tokio::test]
    async fn operator_can_store_a_private_microsoft_registration_without_a_restart() {
        let runtime = TempDir::new().unwrap();
        let configuration_path = runtime.path().join("secrets/email-oauth-config.json");
        let token_path = runtime.path().join("secrets/email-oauth.json");
        let state = AppState::default()
            .with_terminal_host(
                HostClient::new(runtime.path().join("absent.sock")),
                "secret",
            )
            .with_public_base_url("https://swarm.example.test/")
            .unwrap()
            .with_email_oauth_paths(configuration_path.clone(), token_path.clone());
        let app = router(state);

        let before = authorized_get(app.clone(), "/api/v1/integrations/email/configuration").await;
        let before = response_json(before).await;
        assert_eq!(before["configured"], false);
        assert_eq!(
            before["callback_url"],
            "https://swarm.example.test/auth/email/callback"
        );

        let configured = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/integrations/email/configuration")
                    .header("authorization", "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"tenant_id":"organizations","client_id":"11112222-bbbb-3333-cccc-4444dddd5555","client_secret":"private-value"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(configured.status(), StatusCode::OK);
        let configured = response_json(configured).await;
        assert_eq!(configured["configured"], true);
        assert_eq!(configured["managed_by"], "operator");
        assert_eq!(configured["secret_stored"], true);
        assert_eq!(configured.get("client_secret"), None);
        assert!(!configured.to_string().contains("private-value"));
        assert!(configuration_path.exists());

        let readiness = authorized_get(app, "/api/v1/integrations/email/readiness").await;
        let readiness = response_json(readiness).await;
        assert_eq!(readiness["configured"], true);
        assert_eq!(readiness["connection"], "not_connected");

        let restored = AppState::default()
            .with_public_base_url("https://swarm.example.test/")
            .unwrap()
            .with_email_oauth_paths(configuration_path, token_path)
            .with_saved_outlook_oauth()
            .unwrap();
        assert_eq!(
            restored
                .email_oauth_configuration
                .read()
                .await
                .as_ref()
                .map(|configuration| configuration.source),
            Some(EmailOAuthConfigurationSource::Operator)
        );
    }

    #[tokio::test]
    async fn deployed_email_task_can_prepare_but_not_silently_send_a_reply() {
        let runtime = TempDir::new().unwrap();
        let store = TaskStore::in_memory().unwrap();
        let imported = store
            .import_email_message(
                &swarm_persistence::EmailMessageSnapshot {
                    integration_id: "account-1",
                    message_id: "message-1",
                    conversation_id: "conversation-1",
                    internet_message_id: Some("<one@example.test>"),
                    subject: "Website issue",
                    sender_name: "Reporter",
                    sender_address: "reporter@example.test",
                    received_at: 1_786_730_000,
                    web_url: "https://outlook.office.com/mail/message-1",
                    body_text: "The page is broken.",
                    attachments: &[],
                },
                TaskPriority::Normal,
            )
            .unwrap();
        for target in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(imported.task.id, target).unwrap();
        }
        let app = router(
            AppState::default()
                .with_terminal_host(
                    HostClient::new(runtime.path().join("absent.sock")),
                    "secret",
                )
                .with_task_store(store.clone())
                .with_email_attachment_store(runtime.path().join("email-attachments")),
        );
        let deployment = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tasks/{}/deployments", imported.task.id))
                    .header("authorization", "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"environment":"production","reference":"release-42"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deployment.status(), StatusCode::CREATED);

        let drafted = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/tasks/{}/email/reply", imported.task.id))
                    .header("authorization", "Bearer secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"body":"The issue is fixed and available now."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(drafted.status(), StatusCode::CREATED);
        let draft = response_json(drafted).await;
        assert_eq!(draft["state"], "draft");
        assert_eq!(store.claim_email_reply().unwrap(), None);

        let current = authorized_get(
            app,
            &format!("/api/v1/tasks/{}/email/reply", imported.task.id),
        )
        .await;
        assert_eq!(current.status(), StatusCode::OK);
        assert_eq!(response_json(current).await["state"], "draft");
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
