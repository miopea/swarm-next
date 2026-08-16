use std::{
    collections::HashSet,
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
use swarm_application::{
    AgentPrincipal, ApiaryService, ApplicationError, DecisionRequestInput, TaskService,
};
use swarm_domain::{
    ApiaryTaskId, DecisionRequestKind, DecisionUrgency, HiveId, JiraProjectBindingId,
    QueenActionClass, QueenAutomationOutcome, TaskId, TaskPriority, TaskState, WorkerId,
    WorkerRole,
};
use swarm_persistence::{
    JiraIssueSnapshot, MAX_TASK_ACTIVITY_NOTE_BYTES, TaskStore, TaskStoreError,
};
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
    jira: crate::jira::JiraReadinessProbe,
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
            jira: crate::jira::JiraReadinessProbe::default(),
        }
    }

    #[must_use]
    pub(crate) fn with_jira(mut self, jira: crate::jira::JiraReadinessProbe) -> Self {
        self.jira = jira;
        self
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
        jira: bridge.jira.clone(),
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
    jira: crate::jira::JiraReadinessProbe,
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
        let mut tools = vec![
            list_tasks_tool(),
            transition_task_tool(),
            list_jira_comments_tool(),
            comment_jira_task_tool(),
            list_decisions_tool(),
            request_decision_tool(),
        ];
        if self.principal.role == WorkerRole::Queen {
            tools.extend([
                list_workers_tool(),
                list_coordination_attention_tool(),
                create_task_tool(),
                assign_task_tool(),
                list_apiary_hives_tool(),
                list_apiary_tasks_tool(),
                create_apiary_task_tool(),
                claim_apiary_task_tool(),
                send_apiary_task_to_worker_tool(),
                transition_apiary_task_tool(),
                list_jira_projects_tool(),
                preview_jira_project_tool(),
                sync_jira_project_tool(),
                refresh_jira_project_tool(),
                finish_automation_run_tool(),
            ]);
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    #[allow(clippy::too_many_lines)]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        if self.principal.role == WorkerRole::Queen {
            let action = match request.name.as_ref() {
                "swarm_create_task" | "swarm_assign_task" | "swarm_transition_task" => {
                    QueenActionClass::Coordinate
                }
                "swarm_create_apiary_task"
                | "swarm_claim_apiary_task"
                | "swarm_send_apiary_task_to_worker"
                | "swarm_transition_apiary_task"
                | "swarm_comment_jira_task"
                | "swarm_sync_jira_project"
                | "swarm_refresh_jira_project" => QueenActionClass::ExternalSideEffect,
                _ => QueenActionClass::Advise,
            };
            if let Err(error) = self.require_automation_permission(action) {
                return Ok(
                    CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into(),
                );
            }
        }
        let result = match request.name.as_ref() {
            "swarm_list_tasks" => self
                .tasks
                .list_visible_tasks(self.principal)
                .and_then(|tasks| structured(json!({ "tasks": tasks }))),
            "swarm_transition_task" => self.transition_task(arguments).await,
            "swarm_list_jira_comments" => self.list_jira_comments(arguments).await,
            "swarm_comment_jira_task" => self.comment_jira_task(arguments),
            "swarm_list_workers" => self
                .tasks
                .list_workers(self.principal)
                .and_then(|workers| structured(json!({ "workers": workers }))),
            "swarm_list_coordination_attention" => {
                if self.principal.role == WorkerRole::Queen {
                    self.tasks
                        .store()
                        .current_coordinator_attention()
                        .map_err(ApplicationError::Store)
                        .and_then(|attention| {
                            structured(json!({
                                "attention": attention.into_iter().map(|item| json!({
                                    "action_id": item.action_id,
                                    "worker_id": item.worker_id,
                                    "worker_name": item.worker_name,
                                    "task_id": item.task_id,
                                    "task_title": item.task_title,
                                    "reason": item.reason,
                                    "observed_at": item.observed_at,
                                    "age_seconds": item.age_seconds,
                                })).collect::<Vec<_>>()
                            }))
                        })
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
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
            "swarm_list_apiary_tasks" => {
                if self.principal.role == WorkerRole::Queen {
                    ApiaryService::new(self.tasks.store().clone())
                        .visible_apiary_tasks()
                        .and_then(|tasks| structured(json!({ "tasks": tasks })))
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_list_apiary_hives" => {
                if self.principal.role == WorkerRole::Queen {
                    ApiaryService::new(self.tasks.store().clone())
                        .members()
                        .and_then(|hives| structured(json!({ "hives": hives })))
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_create_apiary_task" => {
                if self.principal.role == WorkerRole::Queen {
                    parse::<CreateApiaryTaskInput>(arguments).and_then(|input| {
                        ApiaryService::new(self.tasks.store().clone())
                            .create_apiary_task_for_hive(
                                &input.title,
                                &input.description,
                                input.priority,
                                input.home_hive_id,
                                crate::unix_timestamp(),
                            )
                            .and_then(structured)
                    })
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_claim_apiary_task" => {
                if self.principal.role == WorkerRole::Queen {
                    parse::<ApiaryTaskInput>(arguments).and_then(|input| {
                        let task_id = ApiaryTaskId::from_str(&input.task_id)
                            .map_err(|_| ApplicationError::NotAuthorized)?;
                        ApiaryService::new(self.tasks.store().clone())
                            .queue_federation_task_claim(task_id, crate::unix_timestamp())
                            .and_then(structured)
                    })
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_send_apiary_task_to_worker" => {
                if self.principal.role == WorkerRole::Queen {
                    parse::<AssignApiaryTaskInput>(arguments).and_then(|input| {
                        let task_id = ApiaryTaskId::from_str(&input.task_id)
                            .map_err(|_| ApplicationError::NotAuthorized)?;
                        let worker_id = WorkerId::from_str(&input.worker_id)
                            .map_err(|_| ApplicationError::NotAuthorized)?;
                        ApiaryService::new(self.tasks.store().clone())
                            .materialize_local_apiary_task_execution(
                                task_id,
                                worker_id,
                                crate::unix_timestamp(),
                            )
                            .and_then(structured)
                    })
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_transition_apiary_task" => {
                if self.principal.role == WorkerRole::Queen {
                    parse::<TransitionApiaryTaskInput>(arguments).and_then(|input| {
                        let task_id = ApiaryTaskId::from_str(&input.task_id)
                            .map_err(|_| ApplicationError::NotAuthorized)?;
                        ApiaryService::new(self.tasks.store().clone())
                            .queue_federation_task_transition(
                                task_id,
                                input.state,
                                crate::unix_timestamp(),
                            )
                            .and_then(structured)
                    })
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_list_jira_projects" => {
                if self.principal.role == WorkerRole::Queen {
                    self.tasks
                        .store()
                        .list_jira_project_bindings()
                        .map_err(Into::into)
                        .and_then(|projects| structured(json!({ "projects": projects })))
                } else {
                    Err(ApplicationError::NotAuthorized)
                }
            }
            "swarm_preview_jira_project" => self.preview_jira_project(arguments).await,
            "swarm_sync_jira_project" => self.sync_jira_project(arguments).await,
            "swarm_refresh_jira_project" => self.refresh_jira_project(arguments).await,
            "swarm_list_decisions" => self
                .tasks
                .list_visible_decisions(Some(self.principal))
                .and_then(|decisions| structured(json!({ "decisions": decisions }))),
            "swarm_request_decision" => {
                parse::<RequestDecisionInput>(arguments).and_then(|input| {
                    let task_id = input
                        .task_id
                        .as_deref()
                        .map(TaskId::from_str)
                        .transpose()
                        .map_err(|_| ApplicationError::NotAuthorized)?;
                    self.tasks
                        .create_decision(
                            self.principal,
                            &DecisionRequestInput {
                                task_id,
                                kind: input.kind,
                                urgency: input.urgency,
                                title: input.title,
                                reason: input.reason,
                                risk: input.risk,
                                evidence: input.evidence,
                                suggested_action: input.suggested_action,
                                allowed_actions: input.allowed_actions,
                                deadline: input.deadline,
                            },
                        )
                        .and_then(structured)
                })
            }
            "swarm_finish_automation_run" => {
                parse::<FinishAutomationRunInput>(arguments).and_then(|input| {
                    if self.principal.role != WorkerRole::Queen {
                        return Err(ApplicationError::NotAuthorized);
                    }
                    let changed = self
                        .tasks
                        .store()
                        .finish_queen_automation_run(
                            &input.run_id,
                            input.outcome,
                            crate::unix_timestamp(),
                        )
                        .map_err(ApplicationError::Store)?;
                    if !changed {
                        return Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(
                            "No matching active Queen automation run".into(),
                        )));
                    }
                    structured(json!({
                        "run_id": input.run_id,
                        "outcome": input.outcome,
                        "state": "completed"
                    }))
                })
            }
            _ => return Err(ErrorData::invalid_params("unknown Swarm tool", None)),
        };
        match result {
            Ok(result) => {
                if request.name.as_ref() != "swarm_list_tasks"
                    && request.name.as_ref() != "swarm_list_workers"
                    && request.name.as_ref() != "swarm_list_decisions"
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

impl AgentMcp {
    fn require_automation_permission(
        &self,
        action: QueenActionClass,
    ) -> Result<(), ApplicationError> {
        let permitted = self
            .tasks
            .store()
            .queen_automation_permits(action, crate::unix_timestamp())
            .map_err(ApplicationError::Store)?;
        if permitted {
            Ok(())
        } else {
            Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(
                "The current unattended Queen run is not authorized for that action. Ask the operator for a decision or finish the run as needs_operator.".into(),
            )))
        }
    }

    async fn transition_task(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<TransitionTaskInput>(arguments)?;
        let task_id =
            TaskId::from_str(&input.task_id).map_err(|_| ApplicationError::NotAuthorized)?;
        if self.principal.role != WorkerRole::Queen
            && !matches!(
                input.state,
                TaskState::Active | TaskState::Blocked | TaskState::Review
            )
        {
            return Err(ApplicationError::NotAuthorized);
        }
        let current = self
            .tasks
            .list_visible_tasks(self.principal)?
            .into_iter()
            .find(|task| task.id == task_id)
            .ok_or(ApplicationError::NotAuthorized)?;
        if !current.state.can_transition_to(input.state) {
            return Err(ApplicationError::Store(TaskStoreError::InvalidTransition {
                from: current.state,
                to: input.state,
            }));
        }
        if input.note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(ApplicationError::Store(
                TaskStoreError::InvalidTaskActivityNote,
            ));
        }
        let store = self.tasks.store();
        let task = self
            .tasks
            .transition_task(self.principal, task_id, input.state, &input.note)?;
        crate::deliver_jira_transition_batch(store, &self.jira, self.changed.as_ref()).await;
        structured(task)
    }

    async fn list_jira_comments(
        &self,
        arguments: Value,
    ) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<JiraTaskInput>(arguments)?;
        let task_id = self.visible_task_id(&input.task_id)?;
        let link = self
            .tasks
            .store()
            .jira_issue_link_for_task(task_id)?
            .ok_or(ApplicationError::NotAuthorized)?;
        let comments = self
            .jira
            .comments(&link.issue_key)
            .await
            .map_err(|error| ApplicationError::IntegrationUnavailable(error.to_string()))?;
        structured(json!({ "issue_key": link.issue_key, "comments": comments }))
    }

    fn comment_jira_task(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<CommentJiraTaskInput>(arguments)?;
        let task_id = self.visible_task_id(&input.task_id)?;
        self.tasks
            .store()
            .queue_jira_comment(task_id, &input.body)?;
        structured(json!({ "task_id": task_id, "state": "queued" }))
    }

    fn visible_task_id(&self, value: &str) -> Result<TaskId, ApplicationError> {
        let task_id = TaskId::from_str(value).map_err(|_| ApplicationError::NotAuthorized)?;
        self.tasks
            .list_visible_tasks(self.principal)?
            .into_iter()
            .any(|task| task.id == task_id)
            .then_some(task_id)
            .ok_or(ApplicationError::NotAuthorized)
    }

    async fn preview_jira_project(
        &self,
        arguments: Value,
    ) -> Result<CallToolResult, ApplicationError> {
        if self.principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        let input = parse::<PreviewJiraProjectInput>(arguments)?;
        let binding_id = JiraProjectBindingId::from_str(&input.binding_id)
            .map_err(|_| ApplicationError::NotAuthorized)?;
        let store = self.tasks.store();
        let binding = store.get_jira_project_binding(binding_id)?;
        let issues = self
            .jira
            .hive_intake_issues(&binding.project_id)
            .await
            .map_err(|error| ApplicationError::IntegrationUnavailable(error.to_string()))?;
        let imported_issue_ids = store
            .list_jira_issue_links(binding_id)?
            .into_iter()
            .map(|link| link.issue_id)
            .collect::<Vec<_>>();
        structured(json!({
            "project": binding,
            "issues": issues,
            "imported_issue_ids": imported_issue_ids,
        }))
    }

    async fn sync_jira_project(
        &self,
        arguments: Value,
    ) -> Result<CallToolResult, ApplicationError> {
        if self.principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        let input = parse::<SyncJiraProjectInput>(arguments)?;
        let binding_id = JiraProjectBindingId::from_str(&input.binding_id)
            .map_err(|_| ApplicationError::NotAuthorized)?;
        let store = self.tasks.store();
        let binding = store.get_jira_project_binding(binding_id)?;
        let issues = self
            .jira
            .hive_intake_issues(&binding.project_id)
            .await
            .map_err(|error| ApplicationError::IntegrationUnavailable(error.to_string()))?;
        let selected_ids = input
            .issue_ids
            .iter()
            .map(|id| id.trim().to_owned())
            .collect::<HashSet<_>>();
        if selected_ids.is_empty()
            || selected_ids.len() > 100
            || selected_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        {
            return Err(ApplicationError::IntegrationUnavailable(
                "choose between 1 and 100 Jira issue ids from the preview".to_owned(),
            ));
        }
        let mut selected_issues = issues
            .into_iter()
            .filter(|issue| selected_ids.contains(&issue.id))
            .collect::<Vec<_>>();
        if selected_issues.len() != selected_ids.len() {
            return Err(ApplicationError::IntegrationUnavailable(
                "one or more selected Jira issues are no longer available".to_owned(),
            ));
        }
        self.jira
            .claim_unassigned_issues(&mut selected_issues)
            .await
            .map_err(|error| ApplicationError::IntegrationUnavailable(error.to_string()))?;
        let snapshots = selected_issues
            .iter()
            .map(|issue| JiraIssueSnapshot {
                issue_id: &issue.id,
                issue_key: &issue.key,
                summary: &issue.summary,
                description: &issue.description,
                status_id: &issue.status_id,
                status_name: &issue.status_name,
                assignee_account_id: issue.assignee_account_id.as_deref(),
                assignee_name: issue.assignee_name.as_deref(),
                remote_updated_at: &issue.updated_at,
            })
            .collect::<Vec<_>>();
        let tasks = store.sync_jira_issues(binding_id, &snapshots)?;
        structured(json!({ "project": binding, "tasks": tasks }))
    }

    async fn refresh_jira_project(
        &self,
        arguments: Value,
    ) -> Result<CallToolResult, ApplicationError> {
        if self.principal.role != WorkerRole::Queen {
            return Err(ApplicationError::NotAuthorized);
        }
        let input = parse::<PreviewJiraProjectInput>(arguments)?;
        let binding_id = JiraProjectBindingId::from_str(&input.binding_id)
            .map_err(|_| ApplicationError::NotAuthorized)?;
        let store = self.tasks.store();
        let binding = store.get_jira_project_binding(binding_id)?;
        let imported_ids = store
            .list_jira_issue_links(binding_id)?
            .into_iter()
            .map(|link| link.issue_id)
            .collect::<HashSet<_>>();
        let issues = self
            .jira
            .issues(&binding.project_id)
            .await
            .map_err(|error| ApplicationError::IntegrationUnavailable(error.to_string()))?;
        let snapshots = issues
            .iter()
            .filter(|issue| imported_ids.contains(&issue.id))
            .map(|issue| JiraIssueSnapshot {
                issue_id: &issue.id,
                issue_key: &issue.key,
                summary: &issue.summary,
                description: &issue.description,
                status_id: &issue.status_id,
                status_name: &issue.status_name,
                assignee_account_id: issue.assignee_account_id.as_deref(),
                assignee_name: issue.assignee_name.as_deref(),
                remote_updated_at: &issue.updated_at,
            })
            .collect::<Vec<_>>();
        let tasks = store.sync_jira_issues(binding_id, &snapshots)?;
        structured(json!({
            "project": binding,
            "tasks": tasks,
            "imported_count": imported_ids.len(),
            "refreshed_count": snapshots.len(),
        }))
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
struct CreateApiaryTaskInput {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
    #[serde(default)]
    home_hive_id: Option<HiveId>,
}

#[derive(Deserialize)]
struct ApiaryTaskInput {
    task_id: String,
}

#[derive(Deserialize)]
struct AssignApiaryTaskInput {
    task_id: String,
    worker_id: String,
}

#[derive(Deserialize)]
struct TransitionApiaryTaskInput {
    task_id: String,
    state: TaskState,
}

#[derive(Deserialize)]
struct AssignTaskInput {
    task_id: String,
    worker_id: String,
}

#[derive(Deserialize)]
struct SyncJiraProjectInput {
    binding_id: String,
    issue_ids: Vec<String>,
}

#[derive(Deserialize)]
struct PreviewJiraProjectInput {
    binding_id: String,
}

#[derive(Deserialize)]
struct JiraTaskInput {
    task_id: String,
}

#[derive(Deserialize)]
struct CommentJiraTaskInput {
    task_id: String,
    body: String,
}

#[derive(Deserialize)]
struct TransitionTaskInput {
    task_id: String,
    state: TaskState,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct RequestDecisionInput {
    task_id: Option<String>,
    kind: DecisionRequestKind,
    #[serde(default)]
    urgency: DecisionUrgency,
    title: String,
    reason: String,
    #[serde(default)]
    risk: String,
    #[serde(default)]
    evidence: String,
    suggested_action: String,
    allowed_actions: Vec<String>,
    deadline: Option<i64>,
}

#[derive(Deserialize)]
struct FinishAutomationRunInput {
    run_id: String,
    outcome: QueenAutomationOutcome,
}
fn list_decisions_tool() -> Tool {
    tool(
        "swarm_list_decisions",
        "List decision requests visible to this agent. Queen sees the Hive inbox; workers see only requests they originated.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn request_decision_tool() -> Tool {
    tool(
        "swarm_request_decision",
        "Request operator judgment without interrupting another terminal. Use only when progress genuinely needs input, approval, credentials, conflict resolution, or help.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": ["string", "null"], "format": "uuid" },
                "kind": { "type": "string", "enum": ["input", "approval", "credentials", "conflict", "help"] },
                "urgency": { "type": "string", "enum": ["normal", "time_sensitive"], "default": "normal" },
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "reason": { "type": "string", "maxLength": 10000 },
                "risk": { "type": "string", "maxLength": 10000, "default": "" },
                "evidence": { "type": "string", "maxLength": 10000, "default": "" },
                "suggested_action": { "type": "string", "maxLength": 10000 },
                "allowed_actions": { "type": "array", "minItems": 1, "maxItems": 6, "uniqueItems": true, "items": { "type": "string", "minLength": 1, "maxLength": 80 } },
                "deadline": { "type": ["integer", "null"] }
            },
            "required": ["kind", "title", "reason", "suggested_action", "allowed_actions"],
            "additionalProperties": false
        }),
        false,
    )
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
        "Queen only: list stable worker profiles, operator-reviewed routing descriptions, repository workspaces, and active session bindings before assigning work.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn list_coordination_attention_tool() -> Tool {
    tool(
        "swarm_list_coordination_attention",
        "Queen only: list current deterministic coordination attention, including Active work that is durably unchanged while its loaded worker is resting. Recheck the task and worker before deciding whether to steer, wait, or ask the operator.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn finish_automation_run_tool() -> Tool {
    tool(
        "swarm_finish_automation_run",
        "Queen only: close the exact unattended review marker after coordinating authorized local work or requesting operator input. This does not authorize external side effects.",
        &json!({
            "type": "object",
            "properties": {
                "run_id": { "type": "string", "minLength": 1, "maxLength": 80 },
                "outcome": { "type": "string", "enum": ["completed", "needs_operator", "no_action"] }
            },
            "required": ["run_id", "outcome"],
            "additionalProperties": false
        }),
        false,
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

fn list_apiary_tasks_tool() -> Tool {
    tool(
        "swarm_list_apiary_tasks",
        "Queen only: list Swarm-generated work shared across this Apiary. Keeper is canonical; Jira issue content never traverses this tool.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn list_apiary_hives_tool() -> Tool {
    tool(
        "swarm_list_apiary_hives",
        "Queen only: list public Apiary Hive and operator identities before routing shared work. Never exposes remote workers, repositories, terminals, credentials, or provider sessions.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn create_apiary_task_tool() -> Tool {
    tool(
        "swarm_create_apiary_task",
        "Keeper Queen only: create Swarm-generated Apiary work, either unassigned or routed to one active Member Hive. Route only to a Hive returned by swarm_list_apiary_hives; never target a remote private worker or repository.",
        &json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "description": { "type": "string", "maxLength": 10000, "default": "" },
                "priority": { "type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal" },
                "home_hive_id": { "type": "string", "format": "uuid", "description": "Optional active Member Hive ID. Omit to leave the task available for any Member Hive to claim." }
            },
            "required": ["title"],
            "additionalProperties": false
        }),
        false,
    )
}

fn claim_apiary_task_tool() -> Tool {
    tool(
        "swarm_claim_apiary_task",
        "Member Queen only: queue a claim for one currently unassigned Keeper task. The command survives outages and Keeper resolves competing claims by revision.",
        &json!({
            "type": "object",
            "properties": { "task_id": { "type": "string", "format": "uuid" } },
            "required": ["task_id"],
            "additionalProperties": false
        }),
        false,
    )
}

fn send_apiary_task_to_worker_tool() -> Tool {
    tool(
        "swarm_send_apiary_task_to_worker",
        "Member Queen only: materialize Apiary work already owned by this Hive as one durable local task and assign it to one private worker returned by swarm_list_workers. Exact retries never duplicate work; Keeper never receives the worker or repository identity.",
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

fn transition_apiary_task_tool() -> Tool {
    tool(
        "swarm_transition_apiary_task",
        "Member Queen only: queue the next valid lifecycle transition for Apiary work owned by this Hive before it is sent to a private worker. After materialization, transition the linked local task; Swarm mirrors that worker lifecycle to Keeper in order. Keeper remains canonical and stale revisions become visible conflicts.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["active", "blocked", "review", "completed"] }
            },
            "required": ["task_id", "state"],
            "additionalProperties": false
        }),
        false,
    )
}

fn list_jira_projects_tool() -> Tool {
    tool(
        "swarm_list_jira_projects",
        "Queen only: list Jira projects connected as shared Hive work pools and inspect workflow readiness before planning Jira work.",
        &json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        true,
    )
}

fn preview_jira_project_tool() -> Tool {
    tool(
        "swarm_preview_jira_project",
        "Queen only: review the latest bounded Jira issues and which issue ids are already in this Hive before choosing work to import.",
        &json!({
            "type": "object",
            "properties": {
                "binding_id": { "type": "string", "format": "uuid" }
            },
            "required": ["binding_id"],
            "additionalProperties": false
        }),
        true,
    )
}

fn sync_jira_project_tool() -> Tool {
    tool(
        "swarm_sync_jira_project",
        "Queen only: import 1 to 100 explicitly selected Jira issues from a prior preview. Jira owns issue identity and mapped workflow; Swarm preserves worker assignment and local execution notes.",
        &json!({
            "type": "object",
            "properties": {
                "binding_id": { "type": "string", "format": "uuid" },
                "issue_ids": { "type": "array", "minItems": 1, "maxItems": 100, "uniqueItems": true, "items": { "type": "string", "minLength": 1, "maxLength": 128 } }
            },
            "required": ["binding_id", "issue_ids"],
            "additionalProperties": false
        }),
        false,
    )
}

fn refresh_jira_project_tool() -> Tool {
    tool(
        "swarm_refresh_jira_project",
        "Queen only: refresh Jira-owned identity, workflow, and assignee data for issues already imported into this Hive. Never imports new Jira work.",
        &json!({
            "type": "object",
            "properties": {
                "binding_id": { "type": "string", "format": "uuid" }
            },
            "required": ["binding_id"],
            "additionalProperties": false
        }),
        false,
    )
}

fn transition_task_tool() -> Tool {
    tool(
        "swarm_transition_task",
        "Move a task through its explicit lifecycle. Workers may report only Active, Blocked, or Review for their own assignment. Include a concise Blocked reason or Review handoff note; Queen receives it when not operator-engaged.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["draft", "ready", "active", "blocked", "review", "completed"] },
                "note": { "type": "string", "maxLength": 4000, "description": "Concise blocker reason, review handoff, or transition context" }
            },
            "required": ["task_id", "state"],
            "additionalProperties": false
        }),
        false,
    )
}

fn list_jira_comments_tool() -> Tool {
    tool(
        "swarm_list_jira_comments",
        "Read the Jira discussion for a linked task visible to this agent. Workers may read only their current assignment; Queen may read any Hive task.",
        &json!({
            "type": "object",
            "properties": { "task_id": { "type": "string", "format": "uuid" } },
            "required": ["task_id"],
            "additionalProperties": false
        }),
        true,
    )
}

fn comment_jira_task_tool() -> Tool {
    tool(
        "swarm_comment_jira_task",
        "Queue a durable progress update, question, evidence note, or handoff in the Jira discussion for a linked task visible to this agent.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "body": { "type": "string", "minLength": 1, "maxLength": 4000 }
            },
            "required": ["task_id", "body"],
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
    use axum::{Json, Router, routing::get};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use swarm_domain::{JiraProjectScope, JiraStatusMapping, ProviderKind, SharedWorkBackend};
    use swarm_persistence::JiraProjectBindingInput;
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
        assert!(queen_names.contains(&"swarm_list_apiary_tasks"));
        assert!(queen_names.contains(&"swarm_list_apiary_hives"));
        assert!(queen_names.contains(&"swarm_create_apiary_task"));
        assert!(queen_names.contains(&"swarm_claim_apiary_task"));
        assert!(queen_names.contains(&"swarm_send_apiary_task_to_worker"));
        assert!(queen_names.contains(&"swarm_transition_apiary_task"));
        assert!(queen_names.contains(&"swarm_list_jira_projects"));
        assert!(queen_names.contains(&"swarm_preview_jira_project"));
        assert!(queen_names.contains(&"swarm_sync_jira_project"));
        assert!(queen_names.contains(&"swarm_refresh_jira_project"));
        assert!(queen_names.contains(&"swarm_list_jira_comments"));
        assert!(queen_names.contains(&"swarm_comment_jira_task"));
        assert!(queen_names.contains(&"swarm_list_coordination_attention"));
        assert!(queen_names.contains(&"swarm_finish_automation_run"));
        assert!(!worker_names.contains(&"swarm_preview_jira_project"));
        assert!(!worker_names.contains(&"swarm_sync_jira_project"));
        assert!(!worker_names.contains(&"swarm_refresh_jira_project"));
        assert!(!worker_names.contains(&"swarm_list_apiary_tasks"));
        assert!(!worker_names.contains(&"swarm_list_apiary_hives"));
        assert!(!worker_names.contains(&"swarm_create_apiary_task"));
        assert!(!worker_names.contains(&"swarm_claim_apiary_task"));
        assert!(!worker_names.contains(&"swarm_send_apiary_task_to_worker"));
        assert!(!worker_names.contains(&"swarm_transition_apiary_task"));
        assert!(!worker_names.contains(&"swarm_list_coordination_attention"));
        assert!(!worker_names.contains(&"swarm_finish_automation_run"));
        assert_eq!(
            worker_names,
            [
                "swarm_list_tasks",
                "swarm_transition_task",
                "swarm_list_jira_comments",
                "swarm_comment_jira_task",
                "swarm_list_decisions",
                "swarm_request_decision"
            ]
        );

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
    async fn coordination_attention_tool_is_queen_read_only() {
        let (bridge, _, queen_id, worker_id, _) = setup();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let worker_token = bearer_from_path(&bridge.ensure_worker_config(worker_id).unwrap());
        let request = |token: &str| {
            mcp_request(
                Some(token),
                "tools/call",
                &json!({ "name": "swarm_list_coordination_attention", "arguments": {} }),
            )
        };

        let attention = response_json(handle(bridge.clone(), request(&queen_token)).await).await;
        assert!(attention["result"]["structuredContent"]["attention"].is_array());

        let denied = response_json(handle(bridge, request(&worker_token)).await).await;
        assert!(denied["result"]["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn unattended_queen_run_blocks_external_effects_and_closes_only_its_exact_marker() {
        let (bridge, store, queen_id, _, _) = setup();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(queen_id, session).unwrap();
        store.request_queen_automation_run(10).unwrap();
        let delivery = store.claim_queen_automation(11).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, 12)
            .unwrap();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());

        let blocked = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_comment_jira_task",
                        "arguments": { "task_id": TaskId::new().to_string(), "body": "Automated update" }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(blocked["result"]["isError"], true);
        assert!(
            blocked["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not authorized")
        );

        let finished = response_json(
            handle(
                bridge,
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_finish_automation_run",
                        "arguments": { "run_id": delivery.run_id, "outcome": "needs_operator" }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(finished["result"]["isError"], false);
        assert_eq!(
            finished["result"]["structuredContent"]["state"],
            "completed"
        );
        let status = store.queen_automation_status(13).unwrap();
        assert_eq!(status.outcome, Some(QueenAutomationOutcome::NeedsOperator));
    }

    #[tokio::test]
    async fn queen_worker_roster_includes_operator_reviewed_routing_description() {
        let (bridge, store, queen_id, worker_id, _) = setup();
        store
            .update_worker_profile(
                worker_id,
                None,
                Some("Owns petal rendering and its repository-scoped release checks."),
                None,
                None,
            )
            .unwrap();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let workers = response_json(
            handle(
                bridge,
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({ "name": "swarm_list_workers", "arguments": {} }),
                ),
            )
            .await,
        )
        .await;
        let petal = workers["result"]["structuredContent"]["workers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|profile| profile["id"] == worker_id.to_string())
            .unwrap();
        assert_eq!(
            petal["description"],
            "Owns petal rendering and its repository-scoped release checks."
        );
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

    #[tokio::test]
    async fn keeper_queen_creates_apiary_work_but_worker_cannot_elevate() {
        let (bridge, store, queen_id, worker_id, _) = setup();
        store
            .create_apiary_for_local_hive("Grand Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let worker_token = bearer_from_path(&bridge.ensure_worker_config(worker_id).unwrap());
        let hives = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({ "name": "swarm_list_apiary_hives", "arguments": {} }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(hives["result"]["isError"], false);
        assert_eq!(
            hives["result"]["structuredContent"]["hives"][0]["role"],
            "keeper"
        );
        assert!(!hives.to_string().contains(&worker_id.to_string()));

        let invalid_route = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_create_apiary_task",
                        "arguments": {
                            "title": "Do not leak a private worker boundary",
                            "home_hive_id": worker_id
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(invalid_route["result"]["isError"], true);
        assert!(store.list_visible_apiary_tasks().unwrap().is_empty());

        let arguments = json!({
            "name": "swarm_create_apiary_task",
            "arguments": {
                "title": "Coordinate the release",
                "description": "Keep both Hives aligned.",
                "priority": "high"
            }
        });

        let worker = response_json(
            handle(
                bridge.clone(),
                mcp_request(Some(&worker_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(worker["result"]["isError"], true);
        assert!(store.list_visible_apiary_tasks().unwrap().is_empty());

        let queen = response_json(
            handle(
                bridge.clone(),
                mcp_request(Some(&queen_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(queen["result"]["isError"], false);
        assert_eq!(
            queen["result"]["structuredContent"]["title"],
            "Coordinate the release"
        );
        assert_eq!(
            queen["result"]["structuredContent"]["home_hive_id"],
            Value::Null
        );

        let listed = response_json(
            handle(
                bridge,
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({ "name": "swarm_list_apiary_tasks", "arguments": {} }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(
            listed["result"]["structuredContent"]["tasks"][0]["title"],
            "Coordinate the release"
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn queen_previews_selectively_imports_and_transitions_jira_work() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let written = writes.clone();
        let jira = Router::new()
            .route(
                "/rest/api/3/search/jql",
                get(|| async {
                    Json(json!({
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
                                "summary": "Keep this issue in Jira",
                                "status": { "id": "3", "name": "In Progress" },
                                "assignee": null,
                                "updated": "2026-08-13T13:01:00.000+0000"
                            }
                        }]
                    }))
                }),
            )
            .route(
                "/rest/api/3/issue/WEB-42/transitions",
                get(|| async {
                    Json(json!({ "transitions": [
                        { "id": "41", "to": { "id": "4", "name": "In Review" } }
                    ] }))
                })
                .post(move |Json(body): Json<Value>| {
                    let written = written.clone();
                    async move {
                        assert_eq!(body["transition"]["id"], "41");
                        written.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, jira).await.unwrap() });

        let (bridge, store, queen_id, _, _) = setup();
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
        let bridge = bridge.with_jira(
            crate::jira::JiraReadinessProbe::configured(
                &format!("http://{address}"),
                "operator@example.test",
                "token",
            )
            .unwrap(),
        );
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());

        let preview = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_preview_jira_project",
                        "arguments": { "binding_id": binding.id.to_string() }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(preview["result"]["isError"], false);
        assert_eq!(
            preview["result"]["structuredContent"]["issues"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            preview["result"]["structuredContent"]["imported_issue_ids"],
            json!([])
        );

        let imported = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_sync_jira_project",
                        "arguments": {
                            "binding_id": binding.id.to_string(),
                            "issue_ids": ["20001"]
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(imported["result"]["isError"], false);
        let task_id = imported["result"]["structuredContent"]["tasks"][0]["id"]
            .as_str()
            .unwrap();
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        let transitioned = response_json(
            handle(
                bridge,
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_transition_task",
                        "arguments": {
                            "task_id": task_id,
                            "state": "review",
                            "note": "Ready for operator review"
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(transitioned["result"]["isError"], false);
        assert_eq!(
            transitioned["result"]["structuredContent"]["state"],
            "review"
        );
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        let link = store
            .jira_issue_link_for_task(TaskId::from_str(task_id).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(link.jira_status_id, "4");
        assert_eq!(link.jira_status_name, "In Review");
    }

    #[tokio::test]
    async fn worker_can_request_a_decision_and_queen_sees_the_typed_inbox() {
        let (bridge, store, queen_id, worker_id, _) = setup();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let worker_token = bearer_from_path(&bridge.ensure_worker_config(worker_id).unwrap());

        let created = response_json(
            handle(
                bridge.clone(),
                mcp_request(
                    Some(&worker_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_request_decision",
                        "arguments": {
                            "kind": "input",
                            "urgency": "normal",
                            "title": "Choose an implementation",
                            "reason": "Two valid paths remain",
                            "risk": "A wrong choice adds migration work",
                            "evidence": "Both prototypes pass",
                            "suggested_action": "Use the durable path",
                            "allowed_actions": ["durable", "minimal"]
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(created["result"]["isError"], false);
        assert_eq!(
            created["result"]["structuredContent"]["requesting_worker_id"],
            worker_id.to_string()
        );
        assert_eq!(store.list_decision_requests().unwrap().len(), 1);

        let listed = response_json(
            handle(
                bridge,
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({ "name": "swarm_list_decisions", "arguments": {} }),
                ),
            )
            .await,
        )
        .await;
        assert!(listed["result"]["structuredContent"].is_object());
        assert_eq!(
            listed["result"]["structuredContent"]["decisions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
