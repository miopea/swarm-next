mod attach;
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
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use swarm_domain::{
    ControlRoomEventKind, ProviderKind, TaskDetailsUpdate, TaskId, TaskPriority, TaskState,
    WorkerId, WorkerProfile, WorkerSessionId,
};
use swarm_persistence::{TaskStore, TaskStoreError};
use swarm_terminal::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, HistoryCursor, HostClient,
    HostRequest, HostResponse, JournalLimits, MAX_CANONICAL_SNAPSHOT_BYTES, MAX_TERMINAL_CELLS,
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, TerminalSize,
};
use tokio::{
    sync::{Mutex, Notify, RwLock, Semaphore},
    time::timeout,
};
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

use attach::{ATTACH_GRANT_TTL, AttachGrantError, AttachGrantStore, MAX_ATTACH_GRANTS};
use terminal_socket::{
    MAX_WEBSOCKET_MESSAGE_BYTES, TERMINAL_GRANT_PROTOCOL_PREFIX, TERMINAL_WEBSOCKET_PROTOCOL,
    serve_terminal_socket,
};

const MAX_TERMINAL_WEBSOCKETS: usize = 32;

#[derive(Clone)]
pub struct AppState {
    terminal_limits: JournalLimits,
    terminal_host: Option<HostClient>,
    operator_token: Option<Arc<str>>,
    attach_grants: Arc<AttachGrantStore>,
    websocket_limit: Arc<Semaphore>,
    task_store: Option<TaskStore>,
    worker_lifecycle: Arc<Mutex<()>>,
    worker_errors: Arc<RwLock<HashMap<WorkerId, String>>>,
    control_room_notify: Arc<Notify>,
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
            worker_lifecycle: Arc::new(Mutex::new(())),
            worker_errors: Arc::new(RwLock::new(HashMap::new())),
            control_room_notify: Arc::new(Notify::new()),
        }
    }

    #[must_use]
    pub fn with_task_store(mut self, task_store: TaskStore) -> Self {
        self.task_store = Some(task_store);
        self
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
struct AttachGrantResponse {
    grant: String,
    protocol: &'static str,
    websocket_path: String,
    expires_in_ms: u64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_error: Option<String>,
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
        .route("/health", get(health))
        .route("/api/v1/hive", get(local_hive))
        .route("/api/v1/control-room/events", get(control_room_events))
        .route("/api/v1/runtime/limits", get(runtime_limits))
        .route("/api/v1/runtime/terminal-host", get(terminal_host_status))
        .route("/api/v1/tasks", get(list_tasks).post(create_task))
        .route("/api/v1/tasks/{task_id}", patch(update_task))
        .route("/api/v1/tasks/{task_id}/state", patch(transition_task))
        .route("/api/v1/tasks/{task_id}/assignment", put(assign_task))
        .route("/api/v1/workers", get(list_workers).post(create_worker))
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

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
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

async fn list_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks = task_store(&state)?
        .list_tasks()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_store(&state)?
        .create_task_with_details(
            &request.title,
            &request.description,
            request.priority,
            &request.workspace,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(task)).into_response())
}

async fn update_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TaskDetailsUpdate>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_store(&state)?
        .update_task_details(parse_task_id(&task_id)?, &request)
        .map_err(|error| task_store_error(&error))?;
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
    let task = task_store(&state)?
        .transition_task(parse_task_id(&task_id)?, request.state)
        .map_err(|error| task_store_error(&error))?;
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
    let task = task_store(&state)?
        .assign_task(parse_task_id(&task_id)?, request.session_id)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
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
        .map(|profile| WorkerView {
            running: profile
                .active_session_id
                .is_some_and(|session_id| live.contains(&session_id)),
            runtime_error: errors.get(&profile.id).cloned(),
            profile,
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(workers)).into_response())
}

async fn create_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let position = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?
        .into_iter()
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
    Ok((
        StatusCode::CREATED,
        Json(WorkerView {
            profile,
            running: false,
            runtime_error: None,
        }),
    )
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
    Ok(Json(WorkerView {
        profile,
        running: false,
        runtime_error: None,
    })
    .into_response())
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
        return Ok(WorkerView {
            profile,
            running: true,
            runtime_error: None,
        });
    }
    if profile.provider != ProviderKind::ClaudeCode {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "provider_not_available",
            "this worker provider is not available in the current runtime",
        ));
    }
    let response = request_host(
        state,
        HostRequest::StartClaude {
            workspace: PathBuf::from(&profile.workspace),
            size,
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
    Ok(WorkerView {
        profile,
        running: true,
        runtime_error: None,
    })
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
    Ok(websocket
        .protocols([TERMINAL_WEBSOCKET_PROTOCOL])
        .max_message_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .max_frame_size(MAX_WEBSOCKET_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            serve_terminal_socket(socket, client, session_id).await;
        }))
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

fn task_store_error(error: &TaskStoreError) -> ApiError {
    match error {
        TaskStoreError::NotFound => {
            ApiError::new(StatusCode::NOT_FOUND, "task_not_found", error.to_string())
        }
        TaskStoreError::InvalidTitle
        | TaskStoreError::InvalidDescription
        | TaskStoreError::InvalidWorkspace
        | TaskStoreError::EmptyTaskDetailsUpdate => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_task", error.to_string())
        }
        TaskStoreError::InvalidTransition { .. } | TaskStoreError::CompletedTask => ApiError::new(
            StatusCode::CONFLICT,
            "task_transition_rejected",
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
        TaskStoreError::Io(_)
        | TaskStoreError::Sql(_)
        | TaskStoreError::LockPoisoned
        | TaskStoreError::UnsupportedSchemaVersion { .. }
        | TaskStoreError::IntegrityFailure(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "task_store_unavailable",
            "task persistence is temporarily unavailable",
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
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let matches = presented.len() == expected.len()
        && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()));
    if !matches {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_operator_token",
            "a valid operator bearer token is required",
        ));
    }
    Ok(())
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

        let listed = authorized_get(app, "/api/v1/tasks").await;
        let listed = response_json(listed).await;
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["title"], "Recover every terminal");
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
        let status_response = authorized_get(app, "/api/v1/runtime/terminal-host").await;
        assert_eq!(status_response.headers()[header::CACHE_CONTROL], "no-store");
        let status = response_json(status_response).await;
        assert_eq!(status["status"]["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(status["status"]["draining"], false);
        assert_eq!(status["status"]["running_sessions"], 0);
        server_task.abort();
        let _ = server_task.await;
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
            working_directory: workspace,
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
    async fn task_assignment_requires_and_releases_a_real_worker_session() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "sleep 5".into()],
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let store = TaskStore::in_memory().unwrap();
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
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let host_server = HostServer::bind(&socket, registry).unwrap();
        let host_task = tokio::spawn(host_server.run());
        let state = AppState::default().with_terminal_host(HostClient::new(&socket), "secret");
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

    async fn response_json(response: Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
