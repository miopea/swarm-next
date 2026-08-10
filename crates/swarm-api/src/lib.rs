mod attach;
mod terminal_socket;

use std::{path::PathBuf, str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use swarm_domain::WorkerSessionId;
use swarm_terminal::{HostClient, HostRequest, HostResponse, JournalLimits, TerminalSize};
use tokio::sync::Semaphore;

use attach::{ATTACH_GRANT_TTL, AttachGrantError, AttachGrantStore, MAX_ATTACH_GRANTS};
use terminal_socket::{
    MAX_WEBSOCKET_MESSAGE_BYTES, TERMINAL_GRANT_PROTOCOL_PREFIX, TERMINAL_WEBSOCKET_PROTOCOL,
    serve_terminal_socket,
};

const MAX_TERMINAL_WEBSOCKETS: usize = 32;

#[derive(Clone, Debug)]
pub struct AppState {
    terminal_limits: JournalLimits,
    terminal_host: Option<HostClient>,
    operator_token: Option<Arc<str>>,
    attach_grants: Arc<AttachGrantStore>,
    websocket_limit: Arc<Semaphore>,
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
    terminal: TerminalRuntimeLimits,
}

#[derive(Debug, Serialize)]
struct TerminalRuntimeLimits {
    journal_max_bytes: usize,
    journal_max_frames: usize,
    attach_grant_max_active: usize,
    websocket_max_active: usize,
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

async fn runtime_limits(State(state): State<Arc<AppState>>) -> Json<RuntimeLimitsResponse> {
    Json(RuntimeLimitsResponse {
        terminal: TerminalRuntimeLimits {
            journal_max_bytes: state.terminal_limits.max_bytes,
            journal_max_frames: state.terminal_limits.max_frames,
            attach_grant_max_active: MAX_ATTACH_GRANTS,
            websocket_max_active: MAX_TERMINAL_WEBSOCKETS,
        },
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
            after_sequence: u64::MAX,
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

fn terminal_client(state: &AppState) -> Result<&HostClient, ApiError> {
    state.terminal_host.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_unconfigured",
            "terminal host is not configured",
        )
    })
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
    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use swarm_terminal::{JournalLimits, ProviderCommand, SessionRegistry};
    use swarm_terminal_host::HostServer;
    use tempfile::TempDir;
    use tokio_tungstenite::{
        connect_async,
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

        let grant_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/terminal/sessions/{}/attach-grants",
                        session.id()
                    ))
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(grant_response.status(), StatusCode::OK);
        assert_eq!(grant_response.headers()[header::CACHE_CONTROL], "no-store");
        let grant_json = response_json(grant_response).await;
        let grant = grant_json["grant"].as_str().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let api_task = tokio::spawn(async move { axum::serve(listener, app).await });
        let websocket_url = format!(
            "ws://{address}/api/v1/terminal/sessions/{}/attach",
            session.id()
        );
        let mut request = websocket_url.clone().into_client_request().unwrap();
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
                r#"{"type":"resume","after_sequence":0}"#.into(),
            ))
            .await
            .unwrap();

        let initial = terminal_output_until(&mut websocket, "socket-ready").await;
        assert!(String::from_utf8_lossy(&initial).contains("socket-ready"));
        websocket
            .send(ClientMessage::Text(
                r#"{"type":"input","text":"hello\n"}"#.into(),
            ))
            .await
            .unwrap();
        let after_input = terminal_output_until(&mut websocket, "socket:hello").await;
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

    async fn terminal_output_until<S>(websocket: &mut S, expected: &str) -> Vec<u8>
    where
        S: futures_util::Stream<
                Item = Result<ClientMessage, tokio_tungstenite::tungstenite::Error>,
            > + Unpin,
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        loop {
            let message = tokio::time::timeout_at(deadline, websocket.next())
                .await
                .expect("timed out waiting for terminal WebSocket output")
                .expect("terminal WebSocket closed")
                .unwrap();
            if let ClientMessage::Binary(payload) = message {
                assert_eq!(payload[0], 1);
                output.extend_from_slice(&payload[9..]);
                if String::from_utf8_lossy(&output).contains(expected) {
                    return output;
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
