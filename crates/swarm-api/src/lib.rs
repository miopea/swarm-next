mod agent;
mod attach;
mod attachments;
mod notifications;
mod terminal_socket;

use std::{
    collections::{HashMap, HashSet},
    path::{Path as FilePath, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use base64ct::{Base64UrlUnpadded, Encoding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use swarm_application::{ApplicationError, TaskService};
use swarm_domain::{
    ControlRoomEventKind, DecisionRequestId, NotificationPolicy, PresenceDeviceClass,
    PresenceDeviceId, PresenceMode, PresenceObservationState, ProviderConversationId, ProviderKind,
    TaskDetailsUpdate, TaskId, TaskPriority, TaskState, WorkerAttentionState, WorkerId,
    WorkerProfile, WorkerSessionId,
};
use swarm_persistence::{
    DecisionDeliveryFailure, DecisionDispatch, MAX_OPEN_TASKS_PER_ORDER, MAX_TASK_ACTIVITY_PAGE,
    NotificationSettings, PushSubscriptionInput, TaskDispatch, TaskDispatchFailure,
    TaskOutcomeDispatch, TaskOutcomeFailure, TaskStore, TaskStoreError,
};
use swarm_terminal::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, ClaudeConversationStart,
    HistoryCursor, HostClient, HostRequest, HostResponse, JournalLimits,
    MAX_CANONICAL_SNAPSHOT_BYTES, MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS,
    ProcessResourceSample, TerminalSize, sample_current_process,
};
use tokio::{
    sync::{Mutex, Notify, RwLock, Semaphore},
    time::timeout,
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
    worker_errors: Arc<RwLock<HashMap<WorkerId, String>>>,
    control_room_notify: Arc<Notify>,
    notification_sender: Option<notifications::NotificationSender>,
    attachment_store: Option<AttachmentStore>,
    workspace_roots: Arc<Vec<PathBuf>>,
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
            worker_errors: Arc::new(RwLock::new(HashMap::new())),
            control_room_notify: Arc::new(Notify::new()),
            notification_sender: None,
            attachment_store: None,
            workspace_roots: Arc::new(Vec::new()),
        }
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
    pub fn with_agent_configuration(
        mut self,
        config_root: PathBuf,
        mcp_url: impl Into<Arc<str>>,
    ) -> Self {
        if let Some(store) = self.task_store.clone() {
            self.agent_bridge = Some(agent::AgentBridge::new(
                store,
                config_root,
                mcp_url,
                self.control_room_notify.clone(),
            ));
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
        if let Err(error) = reconcile_worker_bindings(self).await {
            tracing::warn!(message = %error.message, "worker supervisor could not inspect the terminal host");
            return;
        }
        let Ok(profiles) = task_store(self).and_then(|store| {
            store
                .list_worker_profiles()
                .map_err(|error| task_store_error(&error))
        }) else {
            tracing::warn!("worker supervisor could not load the durable roster");
            return;
        };
        for profile in profiles
            .into_iter()
            .filter(|profile| profile.autostart && profile.active_session_id.is_none())
        {
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
            let request = HostRequest::Write {
                session_id: delivery.session_id,
                bytes: decision_delivery_message(&delivery),
            };
            let outcome = match client.request(&request).await {
                Ok(HostResponse::Acknowledged) => {
                    store.complete_decision_delivery(delivery.decision_id, unix_timestamp())
                }
                Ok(HostResponse::Error { code, message }) => {
                    tracing::warn!(decision_id = %delivery.decision_id, worker_id = %delivery.worker_id, %code, %message, "decision delivery was rejected by terminal host");
                    store.fail_decision_delivery(
                        delivery.decision_id,
                        unix_timestamp(),
                        DecisionDeliveryFailure::Retryable,
                    )
                }
                Ok(_) => store.fail_decision_delivery(
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
            let request = HostRequest::Write {
                session_id: delivery.session_id,
                bytes: task_dispatch_message(&delivery),
            };
            let outcome = match client.request(&request).await {
                Ok(HostResponse::Acknowledged) => {
                    store.complete_task_dispatch(&delivery.assignment_id, unix_timestamp())
                }
                Ok(HostResponse::Error { code, message }) => {
                    tracing::warn!(task_id = %delivery.task_id, worker_id = %delivery.worker_id, %code, %message, "task briefing was rejected by terminal host");
                    store.fail_task_dispatch(
                        &delivery.assignment_id,
                        unix_timestamp(),
                        TaskDispatchFailure::Retryable,
                    )
                }
                Ok(_) => store.fail_task_dispatch(
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
            let request = HostRequest::Write {
                session_id: outcome.session_id,
                bytes: task_outcome_message(&outcome),
            };
            let result = match client.request(&request).await {
                Ok(HostResponse::Acknowledged) => {
                    store.complete_task_outcome(&outcome.id, unix_timestamp())
                }
                Ok(HostResponse::Error { code, message }) => {
                    tracing::warn!(task_id = %outcome.task_id, reporter_id = %outcome.reporting_worker_id, recipient_id = %outcome.recipient_worker_id, %code, %message, "task outcome was rejected by terminal host");
                    store.fail_task_outcome(
                        &outcome.id,
                        unix_timestamp(),
                        TaskOutcomeFailure::Retryable,
                    )
                }
                Ok(_) => store.fail_task_outcome(
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
struct RuntimeResourcesResponse {
    sampled_at: i64,
    policy: ResourcePolicyResponse,
    api: ProcessResourceResponse,
    terminal_host: ProcessResourceResponse,
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

fn worker_view(profile: WorkerProfile, running: bool, runtime_error: Option<String>) -> WorkerView {
    let engagement_expires_at = profile.engagement_expires_at;
    let attention_state = if runtime_error.is_some() {
        WorkerAttentionState::Blocked
    } else if !running {
        WorkerAttentionState::Sleeping
    } else if engagement_expires_at.is_some() {
        WorkerAttentionState::WithOperator
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
struct ResolveDecisionRequest {
    action: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct AssignTaskRequest {
    session_id: WorkerSessionId,
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
struct HistoryQuery {
    segment: Option<u64>,
    record: Option<u32>,
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
}

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
        .route("/api/v1/workers/order", put(reorder_workers))
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
        version: env!("CARGO_PKG_VERSION"),
    })
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
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(identity)).into_response())
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
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

fn resource_response(sample: Option<ProcessResourceSample>) -> ProcessResourceResponse {
    let resident_memory_bytes = sample.and_then(|sample| sample.resident_memory_bytes);
    let pressure = match resident_memory_bytes {
        Some(bytes) if bytes >= RESOURCE_CRITICAL_BYTES => ResourcePressure::Critical,
        Some(bytes) if bytes >= RESOURCE_ADVISORY_BYTES => ResourcePressure::Advisory,
        Some(_) => ResourcePressure::Normal,
        None => ResourcePressure::Unavailable,
    };
    ProcessResourceResponse {
        resident_memory_bytes,
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
    let task = task_service(&state)?
        .transition_operator_task_with_note(parse_task_id(&task_id)?, request.state, &request.note)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(Json(task).into_response())
}

async fn assign_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<AssignTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let client = terminal_client(&state)?;
    let sessions = client
        .request(&HostRequest::ListSessions)
        .await
        .map_err(|error| host_unavailable(&error))?;
    let HostResponse::Sessions { sessions } = sessions else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    if !sessions
        .iter()
        .any(|session| session.session_id == request.session_id && session.running)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "worker_session_unavailable",
            "task assignment requires a running worker session",
        ));
    }
    let task = task_service(&state)?
        .assign_operator_task(parse_task_id(&task_id)?, request.session_id)
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
    let errors = state.worker_errors.read().await;
    let workers = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .map(|profile| {
            let running = profile
                .active_session_id
                .is_some_and(|session_id| live.contains(&session_id));
            let runtime_error = errors.get(&profile.id).cloned();
            worker_view(profile, running, runtime_error)
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(workers)).into_response())
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
    let mut workspaces = Vec::new();
    for root in state.workspace_roots.iter() {
        let mut entries = tokio::fs::read_dir(root).await.map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "workspace_catalog_unavailable",
                "configured repository catalog is unavailable",
            )
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "workspace_catalog_unavailable",
                "configured repository catalog could not be read",
            )
        })? {
            if workspaces.len() >= 256 {
                break;
            }
            let file_type = entry.file_type().await.map_err(|_| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "workspace_catalog_unavailable",
                    "configured repository catalog could not be inspected",
                )
            })?;
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
            let kind = if tokio::fs::try_exists(path.join(".git"))
                .await
                .unwrap_or(false)
            {
                "repository"
            } else {
                "folder"
            };
            workspaces.push(WorkspaceView {
                name,
                path: path_text,
                kind,
                configured_worker_id,
            });
        }
    }
    workspaces.sort_by_key(|workspace| workspace.name.to_lowercase());
    Ok(workspaces)
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
    let workspace = workspace_catalog(&state, &profiles)
        .await?
        .iter()
        .find(|workspace| workspace.path == request.workspace)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_workspace",
                "choose a repository from the configured catalog",
            )
        })?;
    if workspace.configured_worker_id.is_some() {
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
            &request.workspace,
            request.autostart,
            position,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(worker_view(profile, false, None))).into_response())
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

async fn start_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
    Json(request): Json<StartWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    require_valid_size(request.rows, request.columns)?;
    let worker = start_worker_process(
        &state,
        parse_worker_id(&worker_id)?,
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
    state.control_room_notify.notify_waiters();
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(worker_view(profile, false, None)).into_response())
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(&state, &headers, HostRequest::ListSessions).await
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
        return Ok(worker_view(profile, true, None));
    }
    if profile.provider != ProviderKind::ClaudeCode {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "provider_not_available",
            "this worker provider is not available in the current runtime",
        ));
    }
    let mcp_config = state
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
        })?;

    let response = request_host(
        state,
        HostRequest::StartClaude {
            workspace: PathBuf::from(&profile.workspace),
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
        },
    )
    .await?;
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
    Ok(worker_view(profile, true, None))
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
        ApplicationError::Store(error) => task_store_error(&error),
    }
}
fn task_store_error(error: &TaskStoreError) -> ApiError {
    match error {
        TaskStoreError::NotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "task_not_found", error.to_string())
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
        TaskStoreError::WorkerNotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "worker_not_found", error.to_string())
        }
        TaskStoreError::InvalidWorkerName => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_worker", error.to_string())
        }
        TaskStoreError::DuplicateWorkerName | TaskStoreError::QueenAlreadyExists => {
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

fn require_valid_size(rows: u16, columns: u16) -> Result<(), ApiError> {
    let cells = usize::from(rows) * usize::from(columns);
    if rows == 0
        || columns == 0
        || rows > MAX_TERMINAL_ROWS
        || columns > MAX_TERMINAL_COLUMNS
        || cells > MAX_TERMINAL_CELLS
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_terminal_size",
            format!(
                "terminal dimensions must be non-zero and within {MAX_TERMINAL_ROWS} rows, \
                 {MAX_TERMINAL_COLUMNS} columns, and {MAX_TERMINAL_CELLS} cells"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, time::Duration};

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
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn resource_pressure_classification_is_explicit_at_each_boundary() {
        let sample = |resident_memory_bytes| {
            resource_response(Some(ProcessResourceSample {
                resident_memory_bytes,
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

    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn task_assignment_requires_and_releases_a_real_worker_session() {
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
                    .body(Body::from(format!(
                        r#"{{"session_id":"{}"}}"#,
                        session.id()
                    )))
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
        assert_eq!(store.get_task(task.id).unwrap().assigned_session_id, None);

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
                    r#"{{"type":"resume","after_sequence":{},"rows":{rows},"columns":{columns}}}"#,
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
