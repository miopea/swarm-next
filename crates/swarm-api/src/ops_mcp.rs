use std::{collections::HashSet, path::Path, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use swarm_application::{OpsTicketError, OpsTicketService};
use swarm_domain::{OpsAppBinding, OpsIntegrationScope, OpsTicketInput};
use swarm_persistence::TaskStoreError;
use tokio::io::AsyncReadExt;
use tower::ServiceExt;

const MAX_CONFIG_BYTES: u64 = 65_536;
const MAX_REQUEST_BYTES: usize = 524_288;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrationsFile {
    integrations: Vec<IntegrationConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrationConfig {
    integration_id: String,
    token_sha256: String,
    bindings: Vec<OpsAppBinding>,
    #[serde(default)]
    disabled: bool,
}

fn digest_bytes(value: &str) -> Result<[u8; 32], StatusCode> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    }
    Ok(digest)
}

async fn authenticate(path: &Path, token: &str) -> Result<OpsIntegrationScope, StatusCode> {
    if !(32..=128).contains(&token.len()) || token.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let config: IntegrationsFile =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if config.integrations.len() > 8 {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let presented: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut ids = HashSet::new();
    let mut digests = HashSet::new();
    let mut authenticated = None;
    for entry in config.integrations {
        let digest = digest_bytes(&entry.token_sha256)?;
        if !ids.insert(entry.integration_id.clone()) || !digests.insert(digest) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let scope = OpsIntegrationScope {
            integration_id: entry.integration_id,
            bindings: entry.bindings,
        };
        let first = scope
            .bindings
            .first()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        scope
            .workspace_for(&first.app_id)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if bool::from(presented.ct_eq(&digest)) && !entry.disabled {
            authenticated = Some(scope);
        }
    }
    authenticated.ok_or(StatusCode::UNAUTHORIZED)
}

pub(crate) async fn handle(
    State(state): State<Arc<crate::AppState>>,
    request: Request<Body>,
) -> Response {
    let Some(path) = state.ops_integrations_path.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(permit) = state.ops_mcp_limit.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let Some(token) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let scope = match authenticate(path, token).await {
        Ok(scope) => scope,
        Err(status) => return status.into_response(),
    };
    let Some(store) = state.task_store.clone() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (parts, body) = request.into_parts();
    let body = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        to_bytes(body, MAX_REQUEST_BYTES),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let handler = OpsMcp {
        service: OpsTicketService::new(store),
        scope,
        changed: state.control_room_notify.clone(),
        permit: Arc::new(permit),
    };
    let service: StreamableHttpService<OpsMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(handler.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_allowed_hosts(crate::agent::allowed_mcp_hosts(&state)),
    );
    let mut response = match service
        .oneshot(Request::from_parts(parts, Body::from(body)))
        .await
    {
        Ok(response) => response.into_response(),
        Err(error) => match error {},
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

#[derive(Clone)]
struct OpsMcp {
    service: OpsTicketService,
    scope: OpsIntegrationScope,
    changed: Arc<tokio::sync::Notify>,
    permit: Arc<tokio::sync::OwnedSemaphorePermit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgressInput {
    app_id: String,
    request_id: String,
}

fn tool(name: &'static str, description: &'static str, schema: &Value, read_only: bool) -> Tool {
    Tool::new(
        name,
        description,
        schema.as_object().cloned().unwrap_or_default(),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(false)
            .open_world(false),
    )
}

fn result(value: Value) -> CallToolResponse {
    CallToolResult::structured(value).into()
}
fn refusal(error: &OpsTicketError) -> CallToolResponse {
    let (code, retryable) = match error {
        OpsTicketError::InvalidCommand(_) => ("invalid_command", false),
        OpsTicketError::Store(TaskStoreError::OpsTicketConflict) => ("submission_conflict", false),
        OpsTicketError::Store(TaskStoreError::NotFound) => ("not_found", false),
        OpsTicketError::Store(_) => ("unavailable", true),
    };
    let mut outcome =
        CallToolResult::structured(json!({"ok":false,"code":code,"retryable":retryable}));
    outcome.is_error = Some(true);
    outcome.into()
}

impl OpsMcp {
    async fn execute(
        &self,
        name: &str,
        mut arguments: Value,
    ) -> Result<CallToolResponse, ErrorData> {
        // A rotated or misconfigured credential must not silently file the same
        // source key under a different integration identity.
        let expected = arguments
            .as_object_mut()
            .and_then(|args| args.remove("integration_id"));
        if expected.as_ref().and_then(Value::as_str) != Some(self.scope.integration_id.as_str()) {
            return Err(ErrorData::invalid_params(
                "Integration identity does not match this credential",
                None,
            ));
        }
        let service = self.service.clone();
        let scope = self.scope.clone();
        // A disconnected caller must not release the work budget while its
        // blocking transaction continues. The job retains the same permit.
        let permit = self.permit.clone();
        match name {
            "ops_submit_ticket" => {
                let input: OpsTicketInput = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("Invalid ticket fields", None))?;
                let outcome = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    service.submit(&scope, input)
                })
                .await
                .map_err(|_| {
                    ErrorData::internal_error(
                        "Ticket service unavailable; retry the same request",
                        None,
                    )
                })?;
                match outcome {
                    Ok(receipt) => {
                        self.changed.notify_waiters();
                        Ok(result(json!({"ok":true,"ticket":receipt})))
                    }
                    Err(error) => Ok(refusal(&error)),
                }
            }
            "ops_ticket_progress" => {
                let input: ProgressInput = serde_json::from_value(arguments)
                    .map_err(|_| ErrorData::invalid_params("Invalid progress fields", None))?;
                if input.app_id.len() > 128 || input.request_id.len() > 128 {
                    return Err(ErrorData::invalid_params("Identifier too long", None));
                }
                let outcome = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    service.progress(&scope, &input.app_id, &input.request_id)
                })
                .await
                .map_err(|_| ErrorData::internal_error("Progress service unavailable", None))?;
                match outcome {
                    Ok(progress) => Ok(result(json!({"ok":true,"progress":{
                        "task_id":progress.task.id,"title":progress.task.title,"state":progress.task.state,
                        "updated_at":progress.task.updated_at,"deployment_recorded":progress.task.deployment_recorded,
                        "closed_on_evidence":progress.task.closed_on_evidence,"closed_unverifiable":progress.task.closed_unverifiable,
                        "activity":{"truncated":progress.activity.truncated,"events":progress.activity.events.into_iter().map(|event| json!({
                            "sequence":event.sequence,"kind":event.kind,"from_state":event.from_state,"to_state":event.to_state,
                            "note":event.note,"occurred_at":event.occurred_at,"actor_kind":event.actor_kind
                        })).collect::<Vec<_>>()},"deployments":progress.deployments
                    }}))),
                    Err(error) => Ok(refusal(&error)),
                }
            }
            _ => Err(ErrorData::invalid_params("Unknown Ops tool", None)),
        }
    }
}

impl ServerHandler for OpsMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Submit reviewed Ops requests as inert drafts; read their progress. No worker, terminal, assignment, state change or customer-send operations are available.")
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![
            tool(
                "ops_submit_ticket",
                "Submit a reviewed request once. Identical retries return the original task; changed retries conflict.",
                &json!({
                    "type":"object","additionalProperties":false,
                    "required":["integration_id","app_id","request_id","conversation_id","title","description","priority"],
                    "properties":{"integration_id":{"type":"string","maxLength":128},"app_id":{"type":"string","maxLength":128},"request_id":{"type":"string","maxLength":128},
                        "conversation_id":{"type":"string","maxLength":128},"title":{"type":"string","maxLength":240},
                        "description":{"type":"string","maxLength":64000},"priority":{"type":"string","enum":["low","normal","high","urgent"]}}
                }),
                false,
            ),
            tool(
                "ops_ticket_progress",
                "Read bounded progress for this integration's request. Closure does not prove deployment.",
                &json!({
                    "type":"object","additionalProperties":false,"required":["integration_id","app_id","request_id"],
                    "properties":{"integration_id":{"type":"string","maxLength":128},"app_id":{"type":"string","maxLength":128},"request_id":{"type":"string","maxLength":128}}
                }),
                true,
            ),
        ]))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.execute(
            &request.name,
            Value::Object(request.arguments.unwrap_or_default()),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "fixture-credential-for-ops-console-only-000000000000000000000000";
    fn config(disabled: bool) -> Value {
        json!({"integrations":[{"integration_id":"console-one","token_sha256":format!("{:x}",Sha256::digest(TOKEN.as_bytes())),
            "bindings":[{"app_id":"app-one","workspace":"/work/one"}],"disabled":disabled}]})
    }
    fn setup() -> (Arc<crate::AppState>, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ops.json");
        std::fs::write(&path, config(false).to_string()).unwrap();
        let state = crate::AppState::default()
            .with_task_store(swarm_persistence::TaskStore::in_memory().unwrap())
            .with_ops_integrations_path(path);
        (Arc::new(state), directory)
    }
    fn request(token: &str, method: &str, params: &Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/mcp/ops")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::HOST, "127.0.0.1")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
            ))
            .unwrap()
    }
    async fn json_response(response: Response) -> Value {
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
    }
    async fn call(state: &Arc<crate::AppState>, name: &str, mut arguments: Value) -> Value {
        if arguments.get("integration_id").is_none() {
            arguments["integration_id"] = json!("console-one");
        }
        json_response(
            handle(
                State(state.clone()),
                request(
                    TOKEN,
                    "tools/call",
                    &json!({"name":name,"arguments":arguments}),
                ),
            )
            .await,
        )
        .await
    }
    fn input() -> Value {
        json!({"app_id":"app-one","request_id":"request-one","conversation_id":"feedback:1",
            "title":"Calendar export","description":"Reviewed scope","priority":"normal"})
    }

    #[tokio::test]
    async fn ops_mcp_exposes_only_intake_and_progress_with_retry_stable_receipts() {
        let (state, _directory) = setup();
        let listed = json_response(
            crate::router(state.as_ref().clone())
                .oneshot(request(TOKEN, "tools/list", &json!({})))
                .await
                .unwrap(),
        )
        .await;
        let names: Vec<_> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["ops_submit_ticket", "ops_ticket_progress"]);
        let first = call(&state, "ops_submit_ticket", input()).await;
        let again = call(&state, "ops_submit_ticket", input()).await;
        assert_eq!(first["result"]["structuredContent"]["ok"], true);
        let first_id = &first["result"]["structuredContent"]["ticket"]["task_id"];
        assert!(first_id.is_string());
        assert_eq!(
            &again["result"]["structuredContent"]["ticket"]["task_id"],
            first_id
        );
        assert_eq!(
            again["result"]["structuredContent"]["ticket"]["replayed"],
            true
        );
        let mut changed = input();
        changed["description"] = json!("Different reviewed scope");
        let rejected = call(&state, "ops_submit_ticket", changed).await;
        assert_eq!(rejected["result"]["isError"], true);
        assert_eq!(
            rejected["result"]["structuredContent"]["code"],
            "submission_conflict"
        );
        let progress = call(
            &state,
            "ops_ticket_progress",
            json!({"app_id":"app-one","request_id":"request-one"}),
        )
        .await;
        let progress = &progress["result"]["structuredContent"]["progress"];
        assert_eq!(progress["state"], "draft");
        assert_eq!(progress["deployment_recorded"], false);
        assert!(progress.get("workspace").is_none());
        assert!(progress.get("description").is_none());
        let denied = call(&state, "swarm_assign_task", json!({})).await;
        assert!(denied.get("error").is_some());
        let denied = call(
            &state,
            "ops_ticket_progress",
            json!({"app_id":"other-app","request_id":"request-one"}),
        )
        .await;
        assert_eq!(denied["result"]["structuredContent"]["code"], "not_found");
    }

    #[tokio::test]
    async fn ops_mcp_revocation_and_malformed_scope_are_effective_on_the_next_request() {
        let (state, directory) = setup();
        let path = directory.path().join("ops.json");
        let wrong = handle(
            State(state.clone()),
            request(&"x".repeat(64), "tools/list", &json!({})),
        )
        .await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        std::fs::write(&path, config(true).to_string()).unwrap();
        assert_eq!(
            handle(
                State(state.clone()),
                request(TOKEN, "tools/list", &json!({}))
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut duplicate = config(false);
        let entry = duplicate["integrations"][0].clone();
        duplicate["integrations"]
            .as_array_mut()
            .unwrap()
            .push(entry);
        std::fs::write(&path, duplicate.to_string()).unwrap();
        assert_eq!(
            handle(
                State(state.clone()),
                request(TOKEN, "tools/list", &json!({}))
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        std::fs::write(&path, json!({"integrations":[]}).to_string()).unwrap();
        assert_eq!(
            handle(
                State(state.clone()),
                request(TOKEN, "tools/list", &json!({}))
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn ops_mcp_refuses_caller_workspace_and_requires_a_valid_configured_host() {
        let (state, _directory) = setup();
        let mut mistaken_identity = input();
        mistaken_identity["integration_id"] = json!("another-console");
        assert!(
            call(&state, "ops_submit_ticket", mistaken_identity)
                .await
                .get("error")
                .is_some()
        );
        let mut supplied = input();
        supplied["workspace"] = json!("/unapproved");
        assert!(
            call(&state, "ops_submit_ticket", supplied)
                .await
                .get("error")
                .is_some()
        );
        let mut supplied = input();
        supplied["app_id"] = json!("other-app");
        let rejected = call(&state, "ops_submit_ticket", supplied).await;
        assert_eq!(
            rejected["result"]["structuredContent"]["code"],
            "invalid_command"
        );
        let mut evil = request(TOKEN, "tools/list", &json!({}));
        evil.headers_mut().insert(
            header::HOST,
            header::HeaderValue::from_static("untrusted.example"),
        );
        assert_eq!(
            handle(State(state), evil).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn ops_mcp_bounds_requests_and_concurrency_and_defaults_to_disabled() {
        let (state, _directory) = setup();
        let mut oversized = request(TOKEN, "tools/list", &json!({}));
        *oversized.body_mut() = Body::from(vec![b'x'; MAX_REQUEST_BYTES + 1]);
        assert_eq!(
            handle(State(state.clone()), oversized).await.status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let first = state.ops_mcp_limit.clone().try_acquire_owned().unwrap();
        let second = state.ops_mcp_limit.clone().try_acquire_owned().unwrap();
        assert_eq!(
            handle(
                State(state.clone()),
                request(TOKEN, "tools/list", &json!({}))
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        drop(first);
        drop(second);
        assert_eq!(
            handle(State(state), request(TOKEN, "tools/list", &json!({})))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            handle(
                State(Arc::new(crate::AppState::default())),
                request(TOKEN, "tools/list", &json!({}))
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
}
