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
    QueenActionClass, QueenAutomationOutcome, QueenAutomationState, TaskId, TaskPriority,
    TaskState, WorkerId, WorkerRole,
};
use swarm_persistence::{
    JiraIssueSnapshot, MAX_TASK_ACTIVITY_NOTE_BYTES, QueenAutomationFinish, TaskStore,
    TaskStoreError,
};
use tokio::sync::Notify;
use tower::ServiceExt;

/// The MCP server name a worker sees.
///
/// Renaming this removes every running worker's Swarm access at once rather
/// than degrading it: a worker's tool schema is fixed when its session
/// connects, so the old name keeps being used until that session reconnects.
/// The config payload replaces the whole file, so a restarted worker gets this
/// name and no stale entry beside it.
const CONFIG_SERVER_NAME: &str = "swarm";
const MCP_BRIDGE_COMMAND: &str = "swarm-terminal-host";
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
                if !config_uses_stdio_bridge(&contents) {
                    let payload = self.worker_config_payload(&token)?;
                    write_private_atomic(&path, &payload)?;
                }
                return Ok(path);
            }
        }

        let token = generate_token()?;
        let digest = token_digest(&token);
        self.tasks
            .store()
            .replace_worker_agent_credential(worker_id, &digest)?;
        let payload = self.worker_config_payload(&token)?;
        write_private_atomic(&path, &payload)?;
        Ok(path)
    }

    fn worker_config_payload(&self, token: &str) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                CONFIG_SERVER_NAME: {
                    "type": "stdio",
                    "command": MCP_BRIDGE_COMMAND,
                    "args": ["mcp-proxy"],
                    "env": {
                        "SWARM_MCP_URL": self.mcp_url.as_ref(),
                        "SWARM_MCP_AUTHORIZATION": format!("Bearer {token}")
                    }
                }
            }
        }))
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

pub async fn handle(
    bridge: AgentBridge,
    state: Arc<crate::AppState>,
    request: Request<Body>,
) -> Response {
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
        state,
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
    /// Reaches the running installation, not just the task store: reloading
    /// this Hive needs its configured paths and its own build version.
    state: Arc<crate::AppState>,
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
            read_task_history_tool(),
            transition_task_tool(),
            list_jira_comments_tool(),
            comment_jira_task_tool(),
            record_deployment_tool(),
            record_no_deployment_tool(),
            draft_email_reply_tool(),
            create_task_tool(),
            list_decisions_tool(),
            request_decision_tool(),
        ];
        if self.may_reload_this_hive() {
            tools.push(reload_app_tool());
        }
        if self.principal.role == WorkerRole::Queen {
            tools.extend([
                list_workers_tool(),
                list_coordination_attention_tool(),
                assign_task_tool(),
                approve_no_deployment_tool(),
                retire_task_tool(),
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
            "swarm_read_task_history" => self.read_task_history(arguments),
            "swarm_reload_app" => self.reload_app(arguments).await,
            "swarm_approve_no_deployment" => self.approve_no_deployment(arguments),
            "swarm_retire_task" => self.retire_task(arguments),
            "swarm_transition_task" => self.transition_task(arguments).await,
            "swarm_list_jira_comments" => self.list_jira_comments(arguments).await,
            "swarm_comment_jira_task" => self.comment_jira_task(arguments),
            "swarm_record_deployment" => self.record_deployment(arguments),
            "swarm_record_no_deployment" => self.record_no_deployment(arguments),
            "swarm_draft_email_reply" => self.draft_email_reply(arguments),
            "swarm_list_workers" => self
                .tasks
                .list_workers(self.principal)
                .and_then(|workers| structured(json!({ "workers": workers }))),
            "swarm_list_coordination_attention" => {
                if self.principal.role == WorkerRole::Queen {
                    self.tasks
                        .store()
                        .current_coordinator_attention(crate::unix_timestamp())
                        .map_err(ApplicationError::Store)
                        .and_then(|attention| {
                            structured(json!({
                                "attention": attention.into_iter().map(|item| json!({
                                    "action_id": item.action_id,
                                    "kind": item.kind,
                                    "worker_id": item.worker_id,
                                    "worker_name": item.worker_name,
                                    "task_id": item.task_id,
                                    "task_title": item.task_title,
                                    "reason": item.reason,
                                    "observed_at": item.observed_at,
                                    "age_seconds": item.age_seconds,
                                })).collect::<Vec<_>>(),
                                // Briefings that are queued and not moving, and
                                // what each is waiting on. A dispatch that is
                                // never claimed is never attempted and so never
                                // refused, which left it invisible everywhere:
                                // thirteen of them sat six hours with attempts
                                // at zero and nothing anywhere saying why.
                                "held_briefings": self
                                    .tasks
                                    .store()
                                    .held_task_dispatches(crate::unix_timestamp())
                                    .unwrap_or_default(),
                                // Assigned to a worker that is not running.
                                // There is no briefing to hold and no session
                                // to hold it for, so this appears nowhere
                                // above: the task simply carries an owner and
                                // waits. Two sat like that for twenty-one
                                // minutes looking routed.
                                "unreachable_assignments": self
                                    .tasks
                                    .store()
                                    .work_assigned_to_a_worker_that_is_not_running()
                                    .unwrap_or_default(),
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
                    if self.principal.role == WorkerRole::Queen
                        && self
                            .tasks
                            .store()
                            .queen_automation_status(crate::unix_timestamp())
                            .map_err(ApplicationError::Store)?
                            .state
                            == QueenAutomationState::Running
                    {
                        if input.task_id.is_none() {
                            return Err(ApplicationError::Store(
                                TaskStoreError::IntegrityFailure(
                                    "An unattended review must create one decision per concrete task. Do not group unrelated tasks into one approval.".into(),
                                ),
                            ));
                        }
                        if input.questions.is_empty()
                            && !input
                                .allowed_actions
                                .iter()
                                .any(|action| action == &input.suggested_action)
                        {
                            return Err(ApplicationError::Store(
                                TaskStoreError::IntegrityFailure(
                                    "The suggested action must exactly match one button in allowed_actions during unattended review.".into(),
                                ),
                            ));
                        }
                    }
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
                                summary: input.summary,
                                reason: input.reason,
                                risk: input.risk,
                                evidence: input.evidence,
                                suggested_action: input.suggested_action,
                                allowed_actions: input.allowed_actions,
                                questions: input.questions,
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
                    let finish = self
                        .tasks
                        .store()
                        .finish_queen_automation_run(
                            &input.run_id,
                            input.outcome,
                            crate::unix_timestamp(),
                        )
                        .map_err(ApplicationError::Store)?;
                    // Say which of the reasons it was. The old text claimed no
                    // matching run existed, which was false whenever the marker
                    // had simply moved state, and left the caller retrying the
                    // same call because nothing in it suggested what to do.
                    if let Some(reason) = match &finish {
                        QueenAutomationFinish::Closed => None,
                        QueenAutomationFinish::WrongState { state } => Some(format!(
                            "This run is the current one but its marker is {state}, so there is nothing to close. A completed run needs no second finish; a queued or delivering one has not been handed to you yet."
                        )),
                        QueenAutomationFinish::DifferentRun { current } => Some(format!(
                            "That run is over. The current run is {current} — finish that one if it was delivered to you, and ignore the older prompt in your scrollback."
                        )),
                        QueenAutomationFinish::NoRun => Some(
                            "No Queen automation run has been recorded on this Hive.".to_owned(),
                        ),
                    } {
                        return Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(
                            reason,
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

    /// Whether this caller may restart the Hive it is talking to.
    ///
    /// Only the worker whose own workspace is the development checkout. A
    /// worker in another repository restarting this Hive would be restarting
    /// somebody else's control room to fix its own bug, and Queen coordinates
    /// rather than builds.
    fn may_reload_this_hive(&self) -> bool {
        let Some(checkout) = self.state.development_checkout_path.as_ref() else {
            return false;
        };
        if self.state.development_reload_request_path.is_none() {
            return false;
        }
        self.tasks
            .store()
            .get_worker_profile(self.principal.worker_id)
            .is_ok_and(|profile| {
                std::fs::canonicalize(&profile.workspace).is_ok_and(|workspace| {
                    std::fs::canonicalize(checkout.as_ref())
                        .is_ok_and(|checkout| workspace == checkout)
                })
            })
    }

    /// Rebuilds and restarts this Hive, or reports what the last request did.
    ///
    /// Split into request and status because the API cannot answer a call that
    /// restarts the API. The worker asks, the process is replaced, and the
    /// worker asks again — which is also the only version of this that proves
    /// anything, since the running build is read after the swap rather than
    /// predicted before it.
    async fn reload_app(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<ReloadAppInput>(arguments)?;
        if !self.may_reload_this_hive() {
            return Err(ApplicationError::NotAuthorized);
        }
        let source = crate::runtime::development_source_status(&self.state);
        if input.action == ReloadAppAction::Status {
            let running_revision = crate::runtime::build_source_revision();
            return structured(json!({
                "running_version": crate::build_version(),
                "running_revision": running_revision,
                "checkout_revision": source.as_ref().map(|status| status.revision.clone()),
                "checkout_dirty": source.as_ref().is_some_and(|status| status.dirty),
                "state": crate::runtime::development_reload_state_for_source(
                    &self.state,
                    source.as_ref().map(|status| status.revision.as_str()),
                ),
                "reload_available": source
                    .as_ref()
                    .is_some_and(|status| status.reload_available),
            }));
        }
        // Ruling, 2026-08-23: refuse while the operator is at the Hive rather
        // than queue until they leave. A reload the operator did not ask for,
        // arriving whenever they happen to step away, is worse than being told
        // to try again — and a queued one would fire into a control room that
        // has since been reopened.
        let now = crate::unix_timestamp();
        let presence = self
            .tasks
            .store()
            .operator_presence(now)
            .map_err(ApplicationError::Store)?;
        // Two questions, because presence answers only the first. Somebody can
        // be holding a terminal on a device that has stopped reporting as
        // present, and restarting under them is the thing this guard exists to
        // prevent.
        let holding_a_terminal = self
            .tasks
            .store()
            .operator_holds_any_terminal(now)
            .map_err(ApplicationError::Store)?;
        if presence.mode == swarm_domain::PresenceMode::AtHive || holding_a_terminal {
            return Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(
                "The operator is at the Hive or holding a terminal. Reloading restarts the control room and API they are using, so it is refused while they are here — commit the fix and ask again once they are away, or ask them to press Build and release."
                    .into(),
            )));
        }
        let started = crate::maintenance::start_development_reload(&self.state)
            .await
            .map_err(|error| {
                ApplicationError::Store(TaskStoreError::IntegrityFailure(error.message.clone()))
            })?;
        structured(json!({
            "requested": true,
            "expect_revision": started.source_revision,
            "previous_version": started.previous_version,
            "next": "The API restarts now, so this call cannot report the result. Poll swarm_reload_app with action=status until state is 'ready' or 'failed', then check running_revision against expect_revision before claiming the fix is running.",
        }))
    }

    fn retire_task(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<RetireTaskInput>(arguments)?;
        let task_id =
            TaskId::from_str(&input.task_id).map_err(|_| ApplicationError::NotAuthorized)?;
        self.tasks
            .retire_task(self.principal, task_id, &input.reason)?;
        structured(json!({ "task_id": input.task_id, "retired": true }))
    }

    fn approve_no_deployment(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<ApproveNoDeploymentInput>(arguments)?;
        let task_id =
            TaskId::from_str(&input.task_id).map_err(|_| ApplicationError::NotAuthorized)?;
        let evidence = self
            .tasks
            .approve_completion_exemption(self.principal, task_id)?;
        structured(json!({
            "task_id": input.task_id,
            "evidence": format!("{evidence:?}"),
            "completable": true,
        }))
    }

    fn read_task_history(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<ReadTaskHistoryInput>(arguments)?;
        let task_id =
            TaskId::from_str(&input.task_id).map_err(|_| ApplicationError::NotAuthorized)?;
        let page = self.tasks.read_task_history(
            self.principal,
            task_id,
            input.limit.unwrap_or(50).clamp(1, 200),
        )?;
        structured(json!({
            "task_id": input.task_id,
            "events": page.events,
            // Says so rather than letting a caller mistake a bounded read for
            // the whole history, which is the same failure this tool exists to
            // fix one level up.
            "truncated": page.truncated,
        }))
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

    /// Records where and what the worker deployed, as part of finishing.
    ///
    /// The worker is the actor that deployed and the only one holding the
    /// reference. Asking the operator for it afterwards makes the one person
    /// who cannot check it responsible for asserting it, and leaves the board
    /// showing a completion nobody has shown to be live.
    fn record_deployment(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<RecordDeploymentInput>(arguments)?;
        let task_id = self.visible_task_id(&input.task_id)?;
        let record = self.tasks.store().record_task_deployment(
            task_id,
            &input.environment,
            &input.reference,
            crate::unix_timestamp(),
        )?;
        structured(json!({
            "task_id": task_id,
            "environment": record.environment,
            "reference": record.reference,
        }))
    }

    /// Records that a task has nothing to deploy, with the argument for why.
    ///
    /// This does not close the task. It is a claim Queen approves, because a
    /// worker deciding its own work needs no evidence cannot also be the one
    /// who accepts that decision.
    fn record_no_deployment(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<RecordNoDeploymentInput>(arguments)?;
        let task_id = self.visible_task_id(&input.task_id)?;
        let evidence = self.tasks.store().claim_completion_exemption(
            task_id,
            &input.reason,
            Some(self.principal.worker_id),
            crate::unix_timestamp(),
        )?;
        structured(json!({
            "task_id": task_id,
            "evidence": format!("{evidence:?}"),
            "awaiting": "Queen's approval before this task can complete",
        }))
    }

    /// Writes the reply the person who emailed in will receive.
    ///
    /// Drafting only. Sending is an external effect and stays an explicit
    /// operator act, which is also what the operator asked for: they objected
    /// to writing the reply, not to approving it.
    fn draft_email_reply(&self, arguments: Value) -> Result<CallToolResult, ApplicationError> {
        let input = parse::<DraftEmailReplyInput>(arguments)?;
        let task_id = self.visible_task_id(&input.task_id)?;
        let reply = self
            .tasks
            .store()
            .prepare_email_reply(task_id, &input.body)?;
        structured(json!({
            "task_id": task_id,
            "state": reply.state.to_string(),
            "awaiting": "operator review and send",
            "recipients": reply.targets.len(),
        }))
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

/// Reads a tool's arguments, saying what was wrong with them when it cannot.
///
/// This used to report every malformed argument as an authorisation failure.
/// That is a different claim, and a misleading one: a caller told it is not
/// authorized does not retry with a corrected payload, it escalates or gives
/// up. A missing field cost a long investigation into permissions that were
/// never in question.
fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ApplicationError> {
    serde_json::from_value(value).map_err(|error| {
        ApplicationError::Store(TaskStoreError::IntegrityFailure(format!(
            "The arguments for this tool could not be read: {error}. Check the tool's schema; a field may be missing or the wrong type."
        )))
    })
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

#[derive(Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ReloadAppAction {
    Request,
    Status,
}

#[derive(Deserialize)]
struct ReloadAppInput {
    action: ReloadAppAction,
}

#[derive(Deserialize)]
struct RetireTaskInput {
    task_id: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveNoDeploymentInput {
    task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadTaskHistoryInput {
    task_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionTaskInput {
    task_id: String,
    state: TaskState,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct RecordDeploymentInput {
    task_id: String,
    environment: String,
    reference: String,
}

#[derive(Deserialize)]
struct RecordNoDeploymentInput {
    task_id: String,
    reason: String,
}

#[derive(Deserialize)]
struct DraftEmailReplyInput {
    task_id: String,
    body: String,
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
    /// One or two sentences saying what the operator is deciding and what
    /// turns on it. Required, and short: the reason, risk and evidence around
    /// it may each run to ten thousand characters, and the operator reads this
    /// first.
    ///
    /// Defaulted at this layer even though it is required, because a client
    /// that connected before the field existed holds a schema without it and
    /// strips it from the call. Rejecting at deserialization made every such
    /// worker unable to file any decision at all; the store still refuses an
    /// empty one, and says so in terms the caller can act on.
    #[serde(default)]
    summary: String,
    suggested_action: String,
    /// Empty when the record is an interview: a record is one or the other.
    #[serde(default)]
    allowed_actions: Vec<String>,
    /// Present makes this an interview. The operator answers questions instead
    /// of pressing a button the asker had to guess at.
    #[serde(default)]
    questions: Vec<swarm_domain::DecisionQuestion>,
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
        "Request one concrete operator judgment without interrupting another terminal. During Queen automation, create a separate request for each task; never combine a fleet review or unrelated tasks into one approval. Button actions must describe only that linked task.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": ["string", "null"], "format": "uuid", "description": "Required during Queen automation so the approval opens the exact task." },
                "kind": { "type": "string", "enum": ["input", "approval", "credentials", "conflict", "help"] },
                "urgency": { "type": "string", "enum": ["normal", "time_sensitive"], "default": "normal" },
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "summary": { "type": "string", "minLength": 1, "maxLength": 400, "description": "One or two sentences saying what the operator is deciding and what turns on it. This is what they read first and often the only part they read; reason, risk and evidence are the argument behind it, not a substitute for it." },
                "reason": { "type": "string", "maxLength": 10000 },
                "risk": { "type": "string", "maxLength": 10000, "default": "" },
                "evidence": { "type": "string", "maxLength": 10000, "default": "" },
                "suggested_action": { "type": "string", "maxLength": 80, "description": "The recommended button label. During Queen automation this must exactly match one allowed_actions value." },
                "questions": { "type": "array", "maxItems": 4, "description": "Ask instead of guessing. Each question offers 2 to 4 options and a unique header; the operator may still answer with something none of them offered. A record carries questions or allowed_actions, never both.", "items": { "type": "object", "properties": { "header": { "type": "string", "maxLength": 40 }, "question": { "type": "string", "maxLength": 600 }, "options": { "type": "array", "minItems": 2, "maxItems": 4, "items": { "type": "string", "maxLength": 200 } }, "multi_select": { "type": "boolean", "default": false } }, "required": ["header", "question", "options"], "additionalProperties": false } },
                "allowed_actions": { "type": "array", "minItems": 1, "maxItems": 6, "uniqueItems": true, "description": "Short, task-specific operator choices. Do not encode actions for other tasks.", "items": { "type": "string", "minLength": 1, "maxLength": 80 } },
                "deadline": { "type": ["integer", "null"] }
            },
            "required": ["kind", "title", "summary", "reason", "suggested_action"],
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

fn reload_app_tool() -> Tool {
    tool(
        "swarm_reload_app",
        "Rebuild and restart this Hive's App and API from the development checkout, so a fix does not wait for the operator to press a button. Only for the worker whose workspace IS that checkout. Refused while the operator is at the Hive: restarting the API takes the control room out from under whoever is using it. action=request starts a build and returns the revision it will produce; the API restarts, so it cannot answer again from the same call. Poll action=status afterwards and compare running_revision to the revision you were given before claiming anything was reloaded. Workers keep running across the reload; the terminal host is a separate service.",
        &json!({
            "type": "object",
            "properties": {
                "action": {
                    "enum": ["request", "status"],
                    "description": "request starts a build; status reports the running build and whether one is in flight."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
        false,
    )
}

fn retire_task_tool() -> Tool {
    tool(
        "swarm_retire_task",
        "Queen only: retire work that should not exist any more — superseded, duplicated, or ruled out by the operator. The task leaves the board and keeps its history; the reason is recorded and is what a later reader sees. Not for work that is merely waiting: that is Blocked. Active or Review work must be moved out of flight first.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "reason": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Why this should no longer exist. Recorded on the task."
                }
            },
            "required": ["task_id", "reason"],
            "additionalProperties": false
        }),
        false,
    )
}

fn approve_no_deployment_tool() -> Tool {
    tool(
        "swarm_approve_no_deployment",
        "Queen only: agree that a task genuinely had nothing to deploy, so it can be completed. A worker records the claim with swarm_record_no_deployment and cannot approve its own; read the handoff first, and reject by leaving it in review with a note instead.",
        &json!({
            "type": "object",
            "properties": { "task_id": { "type": "string" } },
            "required": ["task_id"],
            "additionalProperties": false
        }),
        false,
    )
}

fn read_task_history_tool() -> Tool {
    tool(
        "swarm_read_task_history",
        "Read one task's full history: every state change and the complete note written with it, including a worker's handoff. Outcome notifications carry only an excerpt of a handoff, so this is where the whole report is. Use it before accepting or rejecting finished work.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string" },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "How many entries to return, newest last. Defaults to 50."
                }
            },
            "required": ["task_id"],
            "additionalProperties": false
        }),
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
        "Queen only: list current deterministic coordination attention, including Ready work whose delivered brief did not start, Active work that is durably unchanged while its loaded worker is resting, and Active work whose worker exited. Also reports work assigned to a worker that is not running at all, which has no briefing anywhere because there is no session to put one in — start that worker or move the work. And briefings that are queued and not being delivered, and what each is waiting on — an operator using that terminal, or the worker already having Active work. A briefing held for either reason is working as intended and is not a task to chase. One waiting its turn names the earlier task it is queued behind in blocked_by; if that task has been Ready for a long time it is the thing to steer, because the whole queue behind it is stopped. Recheck the task and worker before deciding whether to steer, wait, or ask the operator.",
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
        "Create one durable draft task, for work that should survive this session. Any worker may record work it has found, in its own repository or another; the task lands as an unassigned draft, so this records work rather than routing it. Queen assigns. Do not use for casual operator steering.",
        &json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "description": { "type": "string", "maxLength": 10000, "default": "" },
                "priority": { "type": "string", "enum": ["low", "normal", "high", "urgent"], "default": "normal" },
                "workspace": { "type": "string", "minLength": 1, "maxLength": 4096, "description": "Absolute path of the repository the work belongs to, which is what routing reads. Use the target repository's path, not your own, when the work belongs elsewhere." }
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
        "Queen only: assign durable work to the stable worker whose workspace owns the task's repository. Sleeping workers are valid: assignment queues a guarded wake, and Queen must observe the live session before moving work to Active.",
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
        "Move a task through its explicit lifecycle. Workers may report only Active, Blocked, or Review for their own assignment. Queen must wake an assigned sleeping worker and observe its live session before moving Ready or Blocked work to Active. Include a concise Blocked reason or Review handoff note. Completed requires verification evidence, including release or handoff evidence when shipping was part of done.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["draft", "ready", "active", "blocked", "review", "completed"] },
                "note": { "type": "string", "maxLength": 4000, "description": "Concise blocker reason, review handoff, or completion verification evidence. Required for Completed." }
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

fn record_deployment_tool() -> Tool {
    tool(
        "swarm_record_deployment",
        "Record where the finished work is running, as part of completing a task. You deployed it and hold the reference; the operator cannot verify it for you, and until this exists the board shows a completion nobody has shown to be live. Required before an email reply can be drafted.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "environment": { "type": "string", "minLength": 1, "maxLength": 512, "description": "Where it is running, such as production or staging." },
                "reference": { "type": "string", "minLength": 1, "maxLength": 512, "description": "Anything a third party could use to confirm this is running. Nothing about the shape is required — a bare commit or a bare URL is accepted — but the more checkable the better. Example: \"budgetbug e99140c (PR #66 squash-merge), deploy run 32667983788 — /api/health returns sha e99140c matching origin/main, read 2026-08-23T21:49Z\"." }
            },
            "required": ["task_id", "environment", "reference"],
            "additionalProperties": false
        }),
        false,
    )
}

fn record_no_deployment_tool() -> Tool {
    tool(
        "swarm_record_no_deployment",
        "State that this task has nothing to deploy, and why. For work that genuinely does not ship — a spike, a document, an investigation that found no defect, a duplicate. This does not complete the task: Queen approves the claim, because you cannot both decide your own work needs no evidence and accept that decision. If something did ship, use swarm_record_deployment instead; a reason given here that turns out to be wrong is worse than no claim at all, because it reads as evidence.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "reason": { "type": "string", "minLength": 1, "maxLength": 500, "description": "Why there is nothing running to point at. Say what you did instead." }
            },
            "required": ["task_id", "reason"],
            "additionalProperties": false
        }),
        false,
    )
}

fn draft_email_reply_tool() -> Tool {
    tool(
        "swarm_draft_email_reply",
        "Write the reply for a task that came in by email, as part of finishing it. A person is waiting on that thread and finishing the work tells them nothing. Write for them: what changed, what they can do now, and no internal implementation detail. This drafts only — the operator reviews and sends. Requires the task to be completed and its deployment recorded.",
        &json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "format": "uuid" },
                "body": { "type": "string", "minLength": 1, "maxLength": 8000, "description": "Plain language for the person who wrote in, not a status report." }
            },
            "required": ["task_id", "body"],
            "additionalProperties": false
        }),
        false,
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

/// The name this config filed Swarm under.
///
/// Reads both, writes one. A config written before the product was renamed
/// still says `swarm-next`, and the token inside it is the only copy — refusing
/// to look there would lose every existing worker's credential at the moment of
/// migration, which is the moment it can least afford to be lost.
fn config_server_names() -> [&'static str; 2] {
    [CONFIG_SERVER_NAME, "swarm-next"]
}

fn config_value<'a>(config: &'a Value, suffix: &str) -> Option<&'a Value> {
    config_server_names()
        .into_iter()
        .find_map(|name| config.pointer(&format!("/mcpServers/{name}/{suffix}")))
}

fn token_from_config(contents: &str) -> Option<String> {
    let config: Value = serde_json::from_str(contents).ok()?;
    config_value(&config, "env/SWARM_MCP_AUTHORIZATION")
        .or_else(|| config_value(&config, "headers/Authorization"))?
        .as_str()?
        .strip_prefix("Bearer ")
        .filter(|token| token.len() == TOKEN_BYTES * 2)
        .map(str::to_owned)
}

fn config_uses_stdio_bridge(contents: &str) -> bool {
    let Ok(config) = serde_json::from_str::<Value>(contents) else {
        return false;
    };
    config_value(&config, "type").and_then(Value::as_str) == Some("stdio")
        && config_value(&config, "command").and_then(Value::as_str) == Some(MCP_BRIDGE_COMMAND)
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

    /// Tools Queen holds and a worker must never see. Recording work is not on
    /// this list: a worker files drafts, and only Queen routes them.
    const QUEEN_ONLY_TOOLS: &[&str] = &[
        // A worker records that its work had nothing to deploy; it must never
        // be able to approve its own claim.
        "swarm_approve_no_deployment",
        // Retiring work is a routing judgement, which is Queen's.
        "swarm_retire_task",
        "swarm_preview_jira_project",
        "swarm_sync_jira_project",
        "swarm_refresh_jira_project",
        "swarm_list_apiary_tasks",
        "swarm_list_apiary_hives",
        "swarm_create_apiary_task",
        "swarm_claim_apiary_task",
        "swarm_send_apiary_task_to_worker",
        "swarm_transition_apiary_task",
        "swarm_list_coordination_attention",
        "swarm_finish_automation_run",
        "swarm_assign_task",
        "swarm_list_jira_projects",
    ];

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

    /// An installation with no development checkout configured, which is every
    /// test that is not about reloading one.
    fn plain_state() -> Arc<crate::AppState> {
        Arc::new(crate::AppState::default())
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
        let response = handle(bridge, plain_state(), mcp_request(None, "tools/list", &json!({}))).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[test]
    fn valid_http_configs_upgrade_to_the_local_bridge_without_rotating_authority() {
        let (bridge, _, queen_id, _, _) = setup();
        let path = bridge.ensure_worker_config(queen_id).unwrap();
        let token = bearer_from_path(&path);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "mcpServers": {
                    "swarm": {
                        "type": "http",
                        "url": "http://127.0.0.1:8876/mcp",
                        "headers": { "Authorization": format!("Bearer {token}") }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(bridge.ensure_worker_config(queen_id).unwrap(), path);
        assert_eq!(bearer_from_path(&path), token);
        let upgraded: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            upgraded.pointer("/mcpServers/swarm/type"),
            Some(&Value::String("stdio".into()))
        );
        assert_eq!(
            upgraded.pointer("/mcpServers/swarm/command"),
            Some(&Value::String(MCP_BRIDGE_COMMAND.into()))
        );
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
                plain_state(),
                mcp_request(Some(&queen_token), "tools/list", &json!({})),
            )
            .await,
        )
        .await;
        let worker = response_json(
            handle(
                bridge.clone(),
                plain_state(),
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
        for name in QUEEN_ONLY_TOOLS {
            assert!(queen_names.contains(name), "Queen is missing {name}");
            assert!(!worker_names.contains(name), "a worker was offered {name}");
        }
        for name in [
            "swarm_create_task",
            "swarm_list_jira_comments",
            "swarm_comment_jira_task",
        ] {
            assert!(queen_names.contains(&name), "Queen is missing {name}");
        }
        assert_eq!(
            worker_names,
            [
                "swarm_list_tasks",
                // Reading a task's history is how a handoff is read in full. A
                // worker gets it for its own assignment; the visibility rule is
                // the same one the task list uses.
                "swarm_read_task_history",
                "swarm_transition_task",
                "swarm_list_jira_comments",
                "swarm_comment_jira_task",
                // A worker holds every half of its own work: where the fix is
                // running, the reply to whoever asked, and the follow-up it
                // found. Sending and routing stay above it.
                "swarm_record_deployment",
                "swarm_record_no_deployment",
                "swarm_draft_email_reply",
                "swarm_create_task",
                "swarm_list_decisions",
                "swarm_request_decision"
            ]
        );

        let listed = response_json(
            handle(
                bridge.clone(),
                plain_state(),
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
            plain_state(),
            mcp_request(Some(&queen_token), "tools/list", &json!({})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// A Hive wired to a development checkout, the worker whose workspace IS
    /// that checkout, and a worker in another repository.
    fn reloadable_hive() -> (
        AgentBridge,
        TaskStore,
        Arc<crate::AppState>,
        String,
        String,
        Vec<tempfile::TempDir>,
    ) {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let checkout = tempdir().unwrap();
        let developer = store
            .create_worker(
                "Swarm Next",
                ProviderKind::ClaudeCode,
                checkout.path().to_str().unwrap(),
                false,
                1,
            )
            .unwrap();
        let outsider = store
            .create_worker(
                "Platform",
                ProviderKind::ClaudeCode,
                "/workspace/platform",
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
        let runtime = tempdir().unwrap();
        let state = Arc::new(
            crate::AppState::default()
                .with_task_store(store.clone())
                .with_development_checkout_path(checkout.path().to_path_buf())
                .with_development_reload_paths(
                    runtime.path().join("development-reload.request"),
                    runtime.path().join("development-reload.status"),
                ),
        );
        let developer_token = bearer_from_path(&bridge.ensure_worker_config(developer.id).unwrap());
        let outsider_token = bearer_from_path(&bridge.ensure_worker_config(outsider.id).unwrap());
        (
            bridge,
            store,
            state,
            developer_token,
            outsider_token,
            vec![checkout, directory, runtime],
        )
    }

    async fn reload_call(
        bridge: &AgentBridge,
        state: &Arc<crate::AppState>,
        token: &str,
        action: &str,
    ) -> Value {
        response_json(
            handle(
                bridge.clone(),
                Arc::clone(state),
                mcp_request(
                    Some(token),
                    "tools/call",
                    &json!({
                        "name": "swarm_reload_app",
                        "arguments": { "action": action }
                    }),
                ),
            )
            .await,
        )
        .await
    }

    /// Restarting this Hive is for the worker that builds it, nobody else. A
    /// worker in another repository would be restarting somebody else's control
    /// room to fix its own bug.
    #[tokio::test]
    async fn only_the_worker_whose_workspace_is_the_checkout_may_reload_this_hive() {
        let (bridge, _store, state, developer_token, outsider_token, _keep) = reloadable_hive();
        let names = |token: String| {
            let bridge = bridge.clone();
            let state = Arc::clone(&state);
            async move {
                let listed = response_json(
                    handle(
                        bridge,
                        state,
                        mcp_request(Some(&token), "tools/list", &json!({})),
                    )
                    .await,
                )
                .await;
                listed["result"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            }
        };
        assert!(
            names(developer_token).await.iter().any(|name| name == "swarm_reload_app"),
            "the worker whose workspace is the checkout may reload it"
        );
        assert!(
            !names(outsider_token.clone()).await.iter().any(|name| name == "swarm_reload_app"),
            "a worker in another repository must not be offered this Hive's restart"
        );

        let denied = reload_call(&bridge, &state, &outsider_token, "status").await;
        assert!(
            denied["result"]["isError"].as_bool().unwrap_or(false) || denied["error"].is_object(),
            "and must be refused if it asks anyway, not answered: {denied}"
        );
    }

    /// The operator asked for self-reload so a fix is not gated on them
    /// pressing a button, then ruled that it must refuse while they are at the
    /// Hive rather than queue until they leave.
    #[tokio::test]
    async fn a_reload_is_refused_while_the_operator_is_here_and_never_queued() {
        let (bridge, store, state, developer_token, _outsider_token, _keep) = reloadable_hive();

        // Away: allowed to reach the mechanism. There is no build to make in a
        // bare checkout, so it may be refused on that ground — which is still
        // proof it got past the presence guard.
        store
            .set_manual_presence(Some(swarm_domain::PresenceMode::Away), 1_000)
            .unwrap();
        let away = reload_call(&bridge, &state, &developer_token, "request")
            .await
            .to_string();
        assert!(
            !away.contains("refused while they are here"),
            "a reload must not be refused on presence while the operator is away: {away}"
        );

        store
            .set_manual_presence(Some(swarm_domain::PresenceMode::AtHive), 1_000)
            .unwrap();
        let refused = reload_call(&bridge, &state, &developer_token, "request").await;
        assert!(
            refused.to_string().contains("refused while they are here"),
            "a reload must be refused while the operator is using the control room: {refused}"
        );

        // Away again, but somebody is holding a terminal: still refused. A
        // device can stop reporting as present while a person is still typing.
        store
            .set_manual_presence(Some(swarm_domain::PresenceMode::Away), 1_000)
            .unwrap();
        let worker = store.list_worker_profiles().unwrap();
        let holder = worker
            .iter()
            .find(|profile| profile.name == "Platform")
            .unwrap();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(holder.id, session).unwrap();
        store
            .renew_worker_engagement(
                session,
                Some(swarm_domain::PresenceDeviceId::new()),
                crate::unix_timestamp(),
                300,
            )
            .unwrap();
        let held = reload_call(&bridge, &state, &developer_token, "request").await;
        assert!(
            held.to_string().contains("holding a terminal"),
            "a reload must be refused while a terminal is held, whatever presence says: {held}"
        );
        store.release_worker_session(session).unwrap();

        // Status stays readable throughout: it changes nothing, and it is how
        // the worker closes the loop after a reload.
        let status = reload_call(&bridge, &state, &developer_token, "status").await;
        assert_eq!(
            status["result"]["structuredContent"]["running_version"],
            crate::build_version(),
            "status must report the build that is actually running: {status}"
        );
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

        let attention = response_json(handle(bridge.clone(), plain_state(), request(&queen_token)).await).await;
        assert!(attention["result"]["structuredContent"]["attention"].is_array());

        let denied = response_json(handle(bridge, plain_state(), request(&worker_token)).await).await;
        assert!(denied["result"]["isError"].as_bool().unwrap_or(false));
    }

    #[tokio::test]
    async fn queen_agent_cannot_activate_work_before_the_assigned_worker_is_loaded() {
        let (bridge, store, queen_id, worker_id, _) = setup();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let task = store
            .create_task("Wake before Active", "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();

        let assigned = response_json(
            handle(
                bridge.clone(),
                plain_state(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_assign_task",
                        "arguments": {
                            "task_id": task.id.to_string(),
                            "worker_id": worker_id.to_string()
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(assigned["result"]["isError"], false);

        let start = |bridge: AgentBridge| {
            handle(
                bridge,
                plain_state(),
                mcp_request(
                    Some(&queen_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_transition_task",
                        "arguments": {
                            "task_id": task.id.to_string(),
                            "state": "active",
                            "note": "Worker is ready"
                        }
                    }),
                ),
            )
        };
        let premature = response_json(start(bridge.clone()).await).await;
        assert_eq!(premature["result"]["isError"], true);
        assert!(
            premature["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("active session")
        );
        assert_eq!(store.get_task(task.id).unwrap().state, TaskState::Ready);

        store
            .bind_worker_session(worker_id, swarm_domain::WorkerSessionId::new())
            .unwrap();
        let active = response_json(start(bridge).await).await;
        assert_eq!(active["result"]["isError"], false);
        assert_eq!(active["result"]["structuredContent"]["state"], "active");
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
                plain_state(),
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
                plain_state(),
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
        // Queen reported needing the operator without filing anything for them
        // to answer, so the claim does not stand. Recorded as what actually
        // happened — a finished run with nothing outstanding — rather than as a
        // request the operator can neither find nor resolve.
        let status = store.queen_automation_status(13).unwrap();
        assert_eq!(status.outcome, Some(QueenAutomationOutcome::NoAction));
    }

    #[test]
    fn unreadable_arguments_say_what_is_wrong_rather_than_claiming_no_authority() {
        // Observed live: a worker filing a decision was told "this agent is not
        // authorized for that outcome". Its authority was never in question —
        // its client held a tool schema from before `summary` existed and
        // stripped the field, so deserialization failed and every failure here
        // was reported as an authorisation problem. A caller told it lacks
        // authority does not retry with a corrected payload.
        let refused = parse::<RequestDecisionInput>(json!({ "kind": "input" }));

        let message = match refused {
            Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(message))) => message,
            Err(other) => panic!("expected a readable argument error, got {other:?}"),
            Ok(_) => panic!("arguments with no title should not parse"),
        };
        assert!(message.contains("could not be read"));
        assert!(message.contains("schema"));
    }

    #[test]
    fn a_client_that_predates_the_summary_field_can_still_file() {
        // A client connected before a field was added holds a schema without it
        // and strips it from the call. Rejecting that at deserialization left
        // every already-running worker unable to file any decision at all, so
        // the field is tolerated here and refused by the store with a message
        // the caller can act on.
        let parsed = parse::<RequestDecisionInput>(json!({
            "kind": "input",
            "title": "Something to decide",
            "reason": "Because",
            "suggested_action": "Ship",
            "allowed_actions": ["Ship"],
        }))
        .expect("an older client's call is readable");

        assert_eq!(parsed.summary, "");
    }

    #[tokio::test]
    async fn unattended_queen_decisions_are_task_specific_with_exact_buttons() {
        let (bridge, store, queen_id, _, _) = setup();
        let now = crate::unix_timestamp();
        store
            .bind_worker_session(queen_id, swarm_domain::WorkerSessionId::new())
            .unwrap();
        store.request_queen_automation_run(now).unwrap();
        let delivery = store.claim_queen_automation(now + 1).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, now + 2)
            .unwrap();
        let task = store
            .create_task("Route one concrete task", "/workspace/petal")
            .unwrap();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let request = |arguments: serde_json::Value| {
            mcp_request(
                Some(&queen_token),
                "tools/call",
                &json!({ "name": "swarm_request_decision", "arguments": arguments }),
            )
        };

        let grouped = response_json(
            handle(
                bridge.clone(),
                plain_state(),
                request(json!({
                    "kind": "approval",
                    "title": "Approve all queued work",
                    "reason": "Several tasks are waiting",
                    "summary": "Whether to proceed, and what it costs if we do not.",
                    "suggested_action": "Dispatch all",
                    "allowed_actions": ["Dispatch all", "Hold all"]
                })),
            )
            .await,
        )
        .await;
        assert_eq!(grouped["result"]["isError"], true);
        assert!(
            grouped["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("one decision per concrete task")
        );

        let mismatched = response_json(
            handle(
                bridge.clone(),
                plain_state(),
                request(json!({
                    "task_id": task.id.to_string(),
                    "kind": "approval",
                    "title": "Route this task",
                    "reason": "The repository owner is known",
                    "summary": "Whether to proceed, and what it costs if we do not.",
                    "suggested_action": "Dispatch to Petal",
                    "allowed_actions": ["Approve", "Hold"]
                })),
            )
            .await,
        )
        .await;
        assert_eq!(mismatched["result"]["isError"], true);
        assert!(
            mismatched["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("exactly match one button")
        );

        let concrete = response_json(
            handle(
                bridge,
                plain_state(),
                request(json!({
                    "task_id": task.id.to_string(),
                    "kind": "approval",
                    "title": "Route this task to Petal",
                    "reason": "Petal owns the repository",
                    "summary": "Whether to proceed, and what it costs if we do not.",
                    "suggested_action": "Dispatch to Petal",
                    "allowed_actions": ["Dispatch to Petal", "Hold this task"]
                })),
            )
            .await,
        )
        .await;
        assert_eq!(concrete["result"]["isError"], false);
        assert_eq!(store.list_decision_requests().unwrap().len(), 1);
        assert_eq!(
            concrete["result"]["structuredContent"]["task_id"],
            task.id.to_string()
        );
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
                None,
            )
            .unwrap();
        let queen_token = bearer_from_path(&bridge.ensure_worker_config(queen_id).unwrap());
        let workers = response_json(
            handle(
                bridge,
                plain_state(),
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

    /// A worker asked to file follow-up work had no tool for it and stalled,
    /// so the work was simply lost. Recording is now open to any worker;
    /// routing is not. The draft it writes is inert until Queen assigns it,
    /// which is the boundary that actually matters.
    #[tokio::test]
    async fn a_worker_records_work_it_finds_but_cannot_route_it() {
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
                plain_state(),
                mcp_request(Some(&queen_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(queen["result"]["isError"], false);
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        let worker = response_json(
            handle(
                bridge.clone(),
                plain_state(),
                mcp_request(Some(&worker_token), "tools/call", &arguments),
            )
            .await,
        )
        .await;
        assert_eq!(worker["result"]["isError"], false);
        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 2);
        // Recorded, not routed: nothing a worker files is claimable until
        // someone readies and assigns it.
        assert!(tasks.iter().all(|task| task.state == TaskState::Draft));

        let elevated = response_json(
            handle(
                bridge,
                plain_state(),
                mcp_request(
                    Some(&worker_token),
                    "tools/call",
                    &json!({
                        "name": "swarm_assign_task",
                        "arguments": {
                            "task_id": tasks[0].id.to_string(),
                            "worker_id": worker_id.to_string()
                        }
                    }),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(elevated["result"]["isError"], true);
        assert!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .all(|task| task.assigned_worker_id.is_none())
        );
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                plain_state(),
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
                            "summary": "Whether to proceed, and what it costs if we do not.",
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
                plain_state(),
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
