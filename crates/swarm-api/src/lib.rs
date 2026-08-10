use std::{path::PathBuf, str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use swarm_domain::WorkerSessionId;
use swarm_terminal::{HostClient, HostRequest, HostResponse, JournalLimits, TerminalSize};

#[derive(Clone, Debug)]
pub struct AppState {
    terminal_limits: JournalLimits,
    terminal_host: Option<HostClient>,
    operator_token: Option<Arc<str>>,
}

impl AppState {
    #[must_use]
    pub const fn new(terminal_limits: JournalLimits) -> Self {
        Self {
            terminal_limits,
            terminal_host: None,
            operator_token: None,
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
    terminal_journal_max_bytes: usize,
    terminal_journal_max_frames: usize,
}

#[derive(Debug, Deserialize)]
struct StartSessionRequest {
    workspace: PathBuf,
    rows: u16,
    columns: u16,
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
struct OutputQuery {
    #[serde(default)]
    after: u64,
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
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/runtime/limits", get(runtime_limits))
        .route(
            "/api/v1/terminal/sessions",
            get(list_sessions).post(start_session),
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
        .with_state(Arc::new(state))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn runtime_limits(State(state): State<Arc<AppState>>) -> Json<RuntimeLimitsResponse> {
    Json(RuntimeLimitsResponse {
        terminal_journal_max_bytes: state.terminal_limits.max_bytes,
        terminal_journal_max_frames: state.terminal_limits.max_frames,
    })
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(&state, &headers, HostRequest::ListSessions).await
}

async fn start_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<StartSessionRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    require_non_zero_size(request.rows, request.columns)?;
    authorized_request(
        &state,
        &headers,
        HostRequest::StartClaude {
            workspace: request.workspace,
            size: TerminalSize::new(request.rows, request.columns),
        },
    )
    .await
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
    require_non_zero_size(request.rows, request.columns)?;
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
    authorized_request(
        &state,
        &headers,
        HostRequest::Stop {
            session_id: parse_session_id(&session_id)?,
        },
    )
    .await
}

async fn authorized_request(
    state: &AppState,
    headers: &HeaderMap,
    request: HostRequest,
) -> Result<Json<HostResponse>, ApiError> {
    authorize(state, headers)?;
    let client = state.terminal_host.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_unconfigured",
            "terminal host is not configured",
        )
    })?;
    let response = client.request(&request).await.map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_unavailable",
            error.to_string(),
        )
    })?;
    if let HostResponse::Error { message, .. } = &response {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "terminal_operation_failed",
            message,
        ));
    }
    Ok(Json(response))
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

fn require_non_zero_size(rows: u16, columns: u16) -> Result<(), ApiError> {
    if rows == 0 || columns == 0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_terminal_size",
            "terminal dimensions must be non-zero",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, time::Duration};

    use axum::{body::Body, http::Request};
    use serde_json::Value;
    use swarm_terminal::{JournalLimits, ProviderCommand, SessionRegistry};
    use swarm_terminal_host::HostServer;
    use tempfile::TempDir;
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
        assert_eq!(json["terminal_journal_max_bytes"], 2048);
        assert_eq!(json["terminal_journal_max_frames"], 64);
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
