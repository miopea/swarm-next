use std::{
    fmt::Write as _,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use swarm_application::{AgentPrincipal, ApplicationError, TaskService};
use swarm_domain::{TaskId, TaskPriority, TaskState, WorkerId, WorkerRole};
use swarm_persistence::{TaskStore, TaskStoreError};
use tokio::sync::Notify;
use tower::ServiceExt;

const CONFIG_SERVER_NAME: &str = "swarm-next";
const TOKEN_BYTES: usize = 32;

#[derive(Clone)]
pub struct AgentBridge {
    config_root: Arc<PathBuf>,
    mcp_url: Arc<str>,
    tasks: TaskService,
    changed: Arc<Notify>,
}

impl AgentBridge {
    #[must_use]
    pub fn new(
        store: TaskStore,
        config_root: PathBuf,
        mcp_url: impl Into<Arc<str>>,
        changed: Arc<Notify>,
    ) -> Self {
        Self {
            config_root: Arc::new(config_root),
            mcp_url: mcp_url.into(),
            tasks: TaskService::new(store),
            changed,
        }
    }

    /// Ensures one private provider config and durable digest exist for a worker.
    ///
    /// # Errors
    /// Returns an error when secure random generation, persistence, or private file I/O fails.
    pub fn ensure_worker_config(&self, worker_id: WorkerId) -> Result<PathBuf, AgentBridgeError> {
        std::fs::create_dir_all(self.config_root.as_ref())?;
        set_private_directory(self.config_root.as_ref())?;
        let path = self.config_root.join(format!("{worker_id}.json"));
        if let Ok(contents) = std::fs::read_to_string(&path)
            && let Some(token) = token_from_config(&contents)
        {
            let digest = token_digest(&token);
            if self
                .tasks
                .store()
                .authenticate_worker_agent(&digest)?
                .is_some_and(|profile| profile.id == worker_id)
            {
                return Ok(path);
            }
        }

        let token = generate_token()?;
        let digest = token_digest(&token);
        self.tasks
            .store()
            .replace_worker_agent_credential(worker_id, &digest)?;
        let payload = serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                CONFIG_SERVER_NAME: {
                    "type": "http",
                    "url": self.mcp_url.as_ref(),
                    "headers": { "Authorization": format!("Bearer {token}") }
                }
            }
        }))?;
        write_private_atomic(&path, &payload)?;
        Ok(path)
    }

    fn authenticate(&self, headers: &HeaderMap) -> Result<AgentPrincipal, AgentBridgeError> {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| !value.is_empty())
            .ok_or(AgentBridgeError::Unauthorized)?;
        self.tasks
            .store()
            .authenticate_worker_agent(&token_digest(token))?
            .as_ref()
            .map(AgentPrincipal::from)
            .ok_or(AgentBridgeError::Unauthorized)
    }
}

pub async fn handle(bridge: AgentBridge, request: Request<Body>) -> Response {
    let principal = match bridge.authenticate(request.headers()) {
        Ok(principal) => principal,
        Err(AgentBridgeError::Unauthorized) => {
            return (
                StatusCode::UNAUTHORIZED,
                [
                    (header::WWW_AUTHENTICATE, "Bearer"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
            )
                .into_response();
        }
        Err(error) => {
            tracing::error!(message = %error, "agent authentication failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    let handler = AgentMcp {
        tasks: bridge.tasks.clone(),
        principal,
        changed: bridge.changed.clone(),
    };
    let service: StreamableHttpService<AgentMcp, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(handler.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None),
    );
    match service.oneshot(request).await {
        Ok(response) => response.into_response(),
        Err(error) => match error {},
    }
}

#[derive(Clone)]
struct AgentMcp {
    tasks: TaskService,
    principal: AgentPrincipal,
    changed: Arc<Notify>,
}

impl ServerHandler for AgentMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use Swarm for durable Hive task coordination. Worker authority is limited to its current assignment; Queen coordinates the local roster and queue."
                    .to_owned(),
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools = vec![list_tasks_tool(), transition_task_tool()];
        if self.principal.role == WorkerRole::Queen {
            tools.extend([list_workers_tool(), create_task_tool(), assign_task_tool()]);
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let result = match request.name.as_ref() {
            "swarm_list_tasks" => self
                .tasks
                .list_visible_tasks(self.principal)
                .and_then(|tasks| structured(json!({ "tasks": tasks }))),
            "swarm_transition_task" => parse::<TransitionTaskInput>(arguments).and_then(|input| {
                self.tasks
                    .transition_task(
                        self.principal,
                        TaskId::from_str(&input.task_id)
                            .map_err(|_| ApplicationError::NotAuthorized)?,
                        input.state,
                    )
                    .and_then(structured)
            }),
            "swarm_list_workers" => self
                .tasks
                .list_workers(self.principal)
                .and_then(|workers| structured(json!({ "workers": workers }))),
            "swarm_create_task" => parse::<CreateTaskInput>(arguments).and_then(|input| {
                self.tasks
                    .create_task(
                        self.principal,
                        &input.title,
                        &input.description,
                        input.priority,
                        &input.workspace,
                    )
                    .and_then(structured)
            }),
            "swarm_assign_task" => parse::<AssignTaskInput>(arguments).and_then(|input| {
                let task_id = TaskId::from_str(&input.task_id)
                    .map_err(|_| ApplicationError::NotAuthorized)?;
                let worker_id = WorkerId::from_str(&input.worker_id)
                    .map_err(|_| ApplicationError::NotAuthorized)?;
                self.tasks
                    .assign_task(self.principal, task_id, worker_id)
                    .and_then(structured)
            }),
            _ => return Err(ErrorData::invalid_params("unknown Swarm tool", None)),
        };
        match result {
            Ok(result) => {
                if request.name.as_ref() != "swarm_list_tasks"
                    && request.name.as_ref() != "swarm_list_workers"
                {
                    self.changed.notify_waiters();
                }
                Ok(result.into())
            }
            Err(error) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into())
            }
        }
    }
}

fn structured<T: serde::Serialize>(value: T) -> Result<CallToolResult, ApplicationError> {
    serde_json::to_value(value)
        .map(CallToolResult::structured)
        .map_err(|error| {
            ApplicationError::Store(TaskStoreError::IntegrityFailure(error.to_string()))
        })
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ApplicationError> {
    serde_json::from_value(value).map_err(|_| ApplicationError::NotAuthorized)
}

#[derive(Deserialize)]
struct CreateTaskInput {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
    workspace: String,
}

#[derive(Deserialize)]
struct AssignTaskInput {
    task_id: String,
    worker_id: String,
}

#[derive(Deserialize)]
struct TransitionTaskInput {
    task_id: String,
    state: TaskState,
}

fn list_tasks_tool() -> Tool {
    tool(
        "swarm_list_tasks",
        "List durable tasks visible to this agent. Queen sees the Hive queue; a worker sees only its current assignment.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn list_workers_tool() -> Tool {
    tool(
        "swarm_list_workers",
        "Queen only: list stable worker profiles, repository workspaces, and active session bindings before assigning work.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn create_task_tool() -> Tool {
    tool(
        "swarm_create_task",
        "Queen only: create one durable draft task. Use for work that should survive sessions; do not use for casual operator steering.",
        &json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "description": { "type": "string", "maxLength": 10000, "default": "" },
                "priority": { "type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal" },
                "workspace": { "type": "string", "minLength": 1, "maxLength": 4096 }
            },
            "required": ["title", "workspace"],
            "additionalProperties": false
        }),
        false,
    )
}

fn assign_task_tool() -> Tool {
    tool(
        "swarm_assign_task",
        "Queen only: assign a durable task to a worker's currently active session. Choose a worker whose workspace owns the task's repository.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "worker_id": { "type": "string", "format": "uuid" }
            },
            "required": ["task_id", "worker_id"],
            "additionalProperties": false
        }),
        false,
    )
}

fn transition_task_tool() -> Tool {
    tool(
        "swarm_transition_task",
        "Move a task through its explicit lifecycle. Workers may report only Active, Blocked, or Review for their own assignment; Queen may approve valid transitions including Completed.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["draft", "ready", "active", "blocked", "review", "completed"] }
            },
            "required": ["task_id", "state"],
            "additionalProperties": false
        }),
        false,
    )
}

fn tool(name: &'static str, description: &'static str, schema: &Value, read_only: bool) -> Tool {
    let input_schema = schema.as_object().cloned().unwrap_or_else(Map::new);
    Tool::new(name, description, input_schema).with_annotations(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(false)
            .open_world(false),
    )
}

fn token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn generate_token() -> Result<String, AgentBridgeError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| AgentBridgeError::Random(error.to_string()))?;
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").expect("writing to an in-memory String cannot fail");
    }
    Ok(token)
}

fn token_from_config(contents: &str) -> Option<String> {
    let config: Value = serde_json::from_str(contents).ok()?;
    config
        .pointer("/mcpServers/swarm-next/headers/Authorization")?
        .as_str()?
        .strip_prefix("Bearer ")
        .filter(|token| token.len() == TOKEN_BYTES * 2)
        .map(str::to_owned)
}

fn write_private_atomic(path: &Path, payload: &[u8]) -> Result<(), std::io::Error> {
    let temporary = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(payload)?;
    file.sync_all()?;
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(temporary, path)
}

fn set_private_directory(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AgentBridgeError {
    #[error("agent request is unauthorized")]
    Unauthorized,
    #[error("secure random generation failed: {0}")]
    Random(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Store(#[from] TaskStoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use swarm_domain::ProviderKind;
    use tempfile::tempdir;

    fn setup() -> (
        AgentBridge,
        TaskStore,
        WorkerId,
        WorkerId,
        tempfile::TempDir,
    ) {
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
        let directory = tempdir().unwrap();
        let bridge = AgentBridge::new(
            store.clone(),
            directory.path().to_path_buf(),
            "http://127.0.0.1:8876/mcp",
            Arc::new(Notify::new()),
        );
        (bridge, store, queen.id, worker.id, directory)
    }

    fn bearer_from_path(path: &Path) -> String {
        let config = std::fs::read_to_string(path).unwrap();
        token_from_config(&config).unwrap()
    }

    fn mcp_request(token: Option<&str>, method: &str, params: &Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::HOST, "127.0.0.1:8876")
            .header(header::ACCEPT, "application/json, text/event-stream");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        builder
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": method,
                    "params": params
                })
                .to_string(),
            ))
            .unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let status = response.status();
        let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            panic!(
                "invalid MCP response: {status} {content_type:?} {error}: {}",
                String::from_utf8_lossy(&bytes)
            )
        })
    }
    #[tokio::test]
    async fn endpoint_fails_closed_without_a_scoped_worker_credential() {
        let (bridge, _, _, _, _) = setup();
        let response = handle(bridge, mcp_request(None, "tools/list", &json!({}))).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[tokio::test]
    async fn discovery_is_role_scoped_and_credentials_survive_bridge_recreation() {
        let (bridge, store, queen_id, worker_id, directory) = setup();
        let queen_path = bridge.ensure_worker_config(queen_id).unwrap();
        let worker_path = bridge.ensure_worker_config(worker_id).unwrap();
        let queen_token = bearer_from_path(&queen_path);
        let worker_token = bearer_from_path(&worker_path);

        let queen = response_json(
            handle(
                bridge.clone(),
                mcp_request(Some(&queen_token), "tools/list", &json!({})),
            )
            .await,
        )
        .await;
        let worker = response_json(
            handle(
                bridge.clone(),
                mcp_request(Some(&worker_token), "tools/list", &json!({})),
            )
            .await,
        )
        .await;
        let queen_names = queen["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        let worker_names = worker["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(queen_names.contains(&"swarm_create_task"));
        assert!(queen_names.contains(&"swarm_assign_task"));
        assert_eq!(worker_names, ["swarm_list_tasks", "swarm_transition_task"]);

        let listed = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({ "name": "swarm_list_tasks", "arguments": {} }),
                ),
            )
            .await,
        )
        .await;
        assert!(listed["result"]["structuredContent"].is_object());
        assert!(listed["result"]["structuredContent"]["tasks"].is_array());

        let reopened = AgentBridge::new(
            store,
            directory.path().to_path_buf(),
            "http://127.0.0.1:8876/mcp",
            Arc::new(Notify::new()),
        );
        assert_eq!(reopened.ensure_worker_config(queen_id).unwrap(), queen_path);
        let response = handle(
            reopened,
            mcp_request(Some(&queen_token), "tools/list", &json!({})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn queen_can_create_work_but_worker_cannot_elevate_via_tool_name() {
        let (bridge, store, queen_id, worker_id, _) = setup();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let worker_token = bearer_from_path(&bridge.ensure_worker_config(worker_id).unwrap());
        let arguments = json!({
            "name": "swarm_create_task",
            "arguments": {
                "title": "Scoped bridge",
                "workspace": "/workspace/petal"
            }
        });

        let queen = response_json(
            handle(
                bridge.clone(),
                mcp_request(Some(&queen_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(queen["result"]["isError"], false);
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        let worker = response_json(
            handle(
                bridge,
                mcp_request(Some(&worker_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(worker["result"]["isError"], true);
        assert_eq!(store.list_tasks().unwrap().len(), 1);
    }
}
