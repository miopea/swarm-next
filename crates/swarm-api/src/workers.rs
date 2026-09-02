use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use swarm_domain::{
    CommitRepositoryState, CommitVerdict, PresenceDeviceId, ProviderConversationId, ProviderKind,
    TaskCommit, WorkerId, WorkerProfile,
};
use swarm_terminal::{HostRequest, ProviderActivity, TerminalSize};

use super::{
    ApiError, AppState, WorkerViewFacts, authorize, default_provider, default_terminal_columns,
    default_terminal_rows, parse_worker_id, provider_activity, require_valid_size, task_store,
    task_store_error,
    terminal_host::request_host,
    terminal_socket::VIEWING_ENGAGEMENT_LEASE_SECONDS,
    unix_timestamp,
    worker_runtime::{open_worker_shell, reconcile_worker_bindings, start_worker_process},
    worker_view,
};

#[derive(Debug, Deserialize)]
pub(super) struct CreateWorkerRequest {
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
pub(super) struct UpdateWorkerRequest {
    name: Option<String>,
    description: Option<String>,
    provider: Option<ProviderKind>,
    autostart: Option<bool>,
    /// Where this worker's repository now is. Validated exactly as it is when a
    /// worker is created, because a path that would not be accepted for a new
    /// worker is not one an existing worker should be moved to.
    workspace: Option<String>,
    #[serde(default)]
    allow_outside_roots: bool,
    /// The bee this worker wears. An empty string clears the choice and returns
    /// it to the mark derived from its id.
    mark: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StartWorkerRequest {
    #[serde(default = "default_terminal_rows")]
    rows: u16,
    #[serde(default = "default_terminal_columns")]
    columns: u16,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct WorkspaceView {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) kind: &'static str,
    pub(super) configured_worker_id: Option<WorkerId>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReorderWorkersRequest {
    worker_ids: Vec<WorkerId>,
}

pub(super) async fn list_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let live = reconcile_worker_bindings(&state).await?;
    let profiles = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    let live_ids = live.keys().copied().collect::<HashSet<_>>();
    let provider_activity = provider_activity::refresh(&state, &profiles, &live_ids).await;
    let held = task_store(&state)?
        .workers_holding_for_an_answer(crate::unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    let awaiting_operator = task_store(&state)?
        .workers_awaiting_operator()
        .map_err(|error| task_store_error(&error))?;
    let unconfirmed = task_store(&state)?
        .workers_with_unconfirmed_delivery()
        .map_err(|error| task_store_error(&error))?;
    let mut engaged = task_store(&state)?
        .engaged_devices_by_worker(crate::unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    let errors = state.worker_errors.read().await;
    let scout_id = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?;
    let waking = task_store(&state)?
        .workers_being_woken()
        .map_err(|error| task_store_error(&error))?;
    let workers = profiles
        .into_iter()
        .map(|profile| {
            let running = profile
                .active_session_id
                .is_some_and(|session_id| live.contains_key(&session_id));
            let runtime_error = errors.get(&profile.id).cloned();
            let needs_operator = awaiting_operator.contains(&profile.id);
            let signals = profile
                .active_session_id
                .and_then(|session_id| provider_activity.get(&session_id).copied());
            let activity = signals.map_or(ProviderActivity::Unknown, |seen| seen.activity);
            let background_work = signals.is_some_and(|seen| seen.background_work);
            let is_scout = scout_id == Some(profile.id);
            let profile_id = profile.id;
            let last_output_at = profile
                .active_session_id
                .and_then(|session_id| live.get(&session_id).copied())
                .flatten();
            worker_view(
                profile,
                WorkerViewFacts {
                    running,
                    awaiting_operator: needs_operator,
                    runtime_error,
                    // Only for a worker that is not running: why it is resting
                    // is meaningless for one that is.
                    rest_reason: (!running)
                        .then(|| {
                            task_store(&state)
                                .ok()
                                .and_then(|store| store.last_session_end_reason(profile_id).ok())
                                .flatten()
                        })
                        .flatten(),
                    provider_activity: activity,
                    // A wake queued or in flight for this worker. Read once for
                    // the whole roster rather than per worker, because this
                    // list is rendered on every control-room poll.
                    waking_since: waking.get(&profile_id).copied(),
                    background_work,
                    system_role: is_scout.then_some("scout"),
                    last_output_at,
                    held_for_answer: held.get(&profile_id).copied(),
                    unconfirmed_delivery: unconfirmed.contains(&profile_id),
                    engaged_device: engaged.remove(&profile_id),
                },
            )
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(workers)).into_response())
}

pub(super) async fn list_workspaces(
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

pub(super) async fn workspace_catalog(
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

pub(super) async fn resolve_workspace_path(
    state: &AppState,
    requested: &str,
    allow_outside_roots: bool,
) -> Result<PathBuf, ApiError> {
    let requested = Path::new(requested.trim());
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

#[derive(Deserialize)]
pub(super) struct BroadcastRequest {
    body: String,
}

/// The operator says one thing to every running worker.
///
/// Asked for on 2026-09-02, when doing it one terminal at a time was the only
/// option. It is a MESSAGE and not a stop: it defers while a worker is mid-turn
/// and arrives when the terminal is resting, so it cannot take a thread with it.
///
/// THE RESPONSE SAYS WHO IT COULD NOT REACH, and that is the part that matters.
/// Measured when this was built, 13 of 45 workers had a live session; the rest
/// are excluded from delivery rather than queued for it. A broadcast that
/// answered "sent" would let the operator believe 45 people were told, which is
/// worse than telling 13 by hand — that way they would at least know.
pub(super) async fn broadcast_to_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<BroadcastRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let broadcast = task_store(&state)?
        .broadcast_to_workers(&request.body, crate::unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    // Wakes the delivery loop rather than waiting for its next tick.
    state.control_room_notify.notify_waiters();
    Ok(Json(serde_json::json!({
        "broadcast_id": broadcast.id,
        "reached": broadcast.reached,
        "skipped": broadcast.skipped,
    }))
    .into_response())
}

pub(super) async fn create_worker(
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
    let description = repository_description_draft(Path::new(&workspace)).await?;
    let profile = task_store(&state)?
        .create_worker_with_description(
            &request.name,
            &description,
            request.provider,
            &workspace,
            request.autostart,
            position,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::CREATED,
        Json(worker_view(profile, WorkerViewFacts::default())),
    )
        .into_response())
}

pub(super) async fn reorder_workers(
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

/// The conversation a worker will resume at its NEXT start.
#[derive(Debug, Deserialize)]
pub(super) struct RepointConversationRequest {
    conversation_id: String,
}

/// Points a worker at a different provider conversation.
///
/// WHY THERE WAS NO ROUTE HERE BEFORE. The pin is assigned once, guarded on the
/// worker never having had a session, and nothing in the API wrote it. So a
/// worker on the wrong conversation could not be corrected through the product
/// at all — the operator fixed the live terminal with `/resume`, and the next
/// start resumed the pin and dragged the old thread back.
///
/// DOES NOT MOVE THE RUNNING TERMINAL, and the response says so rather than
/// leaving the caller to assume. A live session keeps writing wherever it
/// already is; this decides what the next start resumes. Refusing while the
/// worker runs would block the exact repair this exists for, because the
/// operator notices the wrong thread precisely by watching it run.
pub(super) async fn repoint_worker_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<RepointConversationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let conversation_id: ProviderConversationId =
        request.conversation_id.trim().parse().map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "conversation_id_invalid",
                "a provider conversation id must be a UUID",
            )
        })?;
    task_store(&state)?
        .repoint_provider_conversation(worker_id, &conversation_id)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "conversation_id": conversation_id.to_string(),
            // NAMED, because the caller cannot see it and the difference
            // matters: the repair they just made does not touch the terminal
            // they are looking at.
            "applies": "next start",
        })),
    )
        .into_response())
}

pub(super) async fn update_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<UpdateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let workspace = match request.workspace.as_deref() {
        Some(workspace) => Some(
            resolve_workspace_path(&state, workspace, request.allow_outside_roots)
                .await?
                .to_string_lossy()
                .into_owned(),
        ),
        None => None,
    };
    // Set on its own, and BEFORE the rest, so choosing a bee alone is a valid
    // request. update_worker_profile refuses a change with none of its fields
    // set — correctly, since renaming a worker to nothing is a mistake — and a
    // picker that sends only a mark would otherwise come back as an empty update.
    if let Some(mark) = request.mark.as_deref() {
        task_store(&state)?
            .set_worker_mark(worker_id, Some(mark))
            .map_err(|error| task_store_error(&error))?;
    }
    let only_the_mark = request.mark.is_some()
        && request.name.is_none()
        && request.description.is_none()
        && request.provider.is_none()
        && request.autostart.is_none()
        && workspace.is_none();
    let profile = if only_the_mark {
        task_store(&state)?
            .get_worker_profile(worker_id)
            .map_err(|error| task_store_error(&error))?
    } else {
        task_store(&state)?
            .update_worker_profile(
                worker_id,
                request.name.as_deref(),
                request.description.as_deref(),
                request.provider,
                request.autostart,
                workspace.as_deref(),
            )
            .map_err(|error| task_store_error(&error))?
    };
    if request.autostart.is_some() {
        state.worker_errors.write().await.remove(&worker_id);
        state
            .worker_recovery_attempts
            .write()
            .await
            .remove(&worker_id);
    }
    let running = profile.active_session_id.is_some();
    let is_scout = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?
        == Some(worker_id);
    state.control_room_notify.notify_waiters();
    Ok(Json(worker_view(
        profile,
        WorkerViewFacts {
            running,
            system_role: is_scout.then_some("scout"),
            ..WorkerViewFacts::default()
        },
    ))
    .into_response())
}

#[derive(Deserialize)]
pub(super) struct SpawnTemporaryRequest {
    provider: ProviderKind,
}

#[derive(Deserialize)]
pub(super) struct AdoptWorkerRequest {
    name: String,
}

/// Spawns a TEMPORARY worker beside this one, on another provider.
///
/// A throwaway sibling in the same workspace rather than a second session on
/// the parent: two providers under one worker would break the
/// one-session-per-worker assumption that sleep/wake, briefing delivery and MCP
/// credential scoping all rely on.
///
/// SHARING THE PARENT'S WORKSPACE IS THE POINT, so this deliberately does not
/// apply `create_worker`'s one-worker-per-repository guard. That guard exists to
/// stop two PERMANENT workers owning the same repository and disagreeing about
/// it; a temporary sibling is the case it was never about.
///
/// # Errors
/// Returns an error when unauthorized, the parent is unknown, or a name cannot
/// be found.
pub(super) async fn spawn_temporary_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<SpawnTemporaryRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let parent_id = parse_worker_id(&worker_id)?;
    let store = task_store(&state)?;
    let parent = store
        .get_worker_profile(parent_id)
        .map_err(|error| task_store_error(&error))?;
    let position = parent.position;
    // Names are UNIQUE, and a temporary worker is exactly the thing an operator
    // spawns twice in a row while comparing two answers. Try the readable name
    // first and fall back to a distinguishable one rather than refusing.
    let readable = format!("{} · {}", parent.name, provider_label(request.provider));
    let created = match store.create_temporary_worker(
        &readable,
        request.provider,
        &parent.workspace,
        position,
    ) {
        Ok(created) => created,
        Err(swarm_persistence::TaskStoreError::DuplicateWorkerName) => store
            .create_temporary_worker(
                &format!("{readable} {}", &WorkerId::new().to_string()[..4]),
                request.provider,
                &parent.workspace,
                position,
            )
            .map_err(|error| task_store_error(&error))?,
        Err(error) => return Err(task_store_error(&error)),
    };
    state.control_room_notify.notify_waiters();
    Ok(Json(created).into_response())
}

/// Adopts a temporary worker into the Hive under a permanent name.
///
/// A FLAG CHANGE, not a re-creation: it keeps its id, so its session history and
/// every board write it already made continue to point at the same worker.
///
/// # Errors
/// Returns an error when unauthorized, the worker is unknown or not temporary,
/// or the name is taken.
pub(super) async fn adopt_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<AdoptWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let adopted = task_store(&state)?
        .adopt_worker(worker_id, &request.name)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(adopted).into_response())
}

/// How a provider is named to an operator choosing one.
fn provider_label(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => "Claude",
        ProviderKind::Codex => "Codex",
        ProviderKind::Gemini => "Gemini",
        ProviderKind::Grok => "Grok",
        ProviderKind::OpenCode => "OpenCode",
        ProviderKind::Unsupported => "unsupported",
    }
}

pub(super) async fn remove_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _guard = state.worker_lifecycle.lock().await;
    let worker_id = parse_worker_id(&worker_id)?;
    task_store(&state)?
        .archive_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    state.worker_errors.write().await.remove(&worker_id);
    state
        .worker_recovery_attempts
        .write()
        .await
        .remove(&worker_id);
    state.control_room_notify.notify_waiters();
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Serialize)]
pub(super) struct WorkerDescriptionDraft {
    description: String,
    source: &'static str,
}

const SCOUT_ROUTING_DESCRIPTION: &str = "Scout owns deliberate cross-repository discovery and preparation across the projects root. Route work here when Queen needs repository mapping, coordinated changes spanning more than one repository, or worktree setup before repository workers receive their own scoped tasks. Ordinary repository work stays with that repository's worker.";

pub(super) async fn draft_worker_description(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let is_scout = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?
        == Some(profile.id);
    let description = if is_scout {
        SCOUT_ROUTING_DESCRIPTION.to_owned()
    } else {
        repository_description_draft(Path::new(&profile.workspace)).await?
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerDescriptionDraft {
            description,
            source: "repository_metadata",
        }),
    )
        .into_response())
}

pub(super) async fn improve_worker_description(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _permit = state
        .worker_description_improvement_limit
        .try_acquire()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "description_improvement_busy",
                "Another repository description is already being improved; try again when it finishes",
            )
        })?;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let is_scout = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?
        == Some(profile.id);
    let context = repository_description_context(Path::new(&profile.workspace), is_scout).await?;
    let description = super::worker_description_ai::improve_description(&context)
        .await
        .map_err(|error| description_ai_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerDescriptionDraft {
            description,
            source: "claude_review",
        }),
    )
        .into_response())
}

fn description_ai_error(error: &super::worker_description_ai::DescriptionAiError) -> ApiError {
    use super::worker_description_ai::DescriptionAiError;
    let status = match error {
        DescriptionAiError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        DescriptionAiError::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        DescriptionAiError::InvalidResponse | DescriptionAiError::Failed => StatusCode::BAD_GATEWAY,
    };
    ApiError::new(status, "description_improvement_failed", error.to_string())
}

const MAX_DESCRIPTION_SOURCE_BYTES: u64 = 64 * 1024;

async fn repository_description_draft(workspace: &Path) -> Result<String, ApiError> {
    let repository_name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("This repository");
    let package_json = read_description_source(workspace, "package.json").await;
    let cargo_toml = read_description_source(workspace, "Cargo.toml").await;
    let pyproject = read_description_source(workspace, "pyproject.toml").await;
    let readme = read_first_available(
        workspace,
        &[
            "README.md",
            "README.MD",
            "readme.md",
            "README.txt",
            "README",
        ],
    )
    .await;

    let package_metadata = package_json
        .as_deref()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok());
    let display_name = package_metadata
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(repository_name);
    let summary = package_metadata
        .as_ref()
        .and_then(|value| value.get("description"))
        .and_then(serde_json::Value::as_str)
        .and_then(clean_summary)
        .or_else(|| cargo_toml.as_deref().and_then(manifest_description))
        .or_else(|| pyproject.as_deref().and_then(manifest_description))
        .or_else(|| readme.as_deref().and_then(readme_summary));
    let implementation = if package_json.is_some() {
        "JavaScript or TypeScript application"
    } else if cargo_toml.is_some() {
        "Rust application or service"
    } else if pyproject.is_some() {
        "Python application or service"
    } else {
        "software repository"
    };
    let ownership = summary
        .unwrap_or_else(|| format!("the {display_name} product and its repository-owned behavior"));
    let ownership = ownership.trim().trim_end_matches(['.', '!', '?']);
    let draft = format!(
        "{display_name} owns {ownership}. This worker should receive changes to this {implementation}, including its product behavior, implementation, tests, and release configuration. Cross-repository coordination stays with Queen or Scout."
    );
    clean_summary(&draft).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "description_unavailable",
            "Swarm could not find usable repository metadata; enter a routing description manually",
        )
    })
}

async fn repository_description_context(
    workspace: &Path,
    is_scout: bool,
) -> Result<String, ApiError> {
    let repository_name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Repository");
    let local_draft = if is_scout {
        SCOUT_ROUTING_DESCRIPTION.to_owned()
    } else {
        repository_description_draft(workspace).await?
    };
    let package_json = read_description_source(workspace, "package.json").await;
    let cargo_toml = read_description_source(workspace, "Cargo.toml").await;
    let pyproject = read_description_source(workspace, "pyproject.toml").await;
    let readme = read_first_available(
        workspace,
        &[
            "README.md",
            "README.MD",
            "readme.md",
            "README.txt",
            "README",
        ],
    )
    .await;
    let package_description = package_json
        .as_deref()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
        .and_then(|value| {
            value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .and_then(|value| clean_summary(&value));
    let manifest_description = cargo_toml
        .as_deref()
        .and_then(manifest_description)
        .or_else(|| pyproject.as_deref().and_then(manifest_description));
    let readme_excerpt = readme.as_deref().and_then(readme_summary);
    let mut sections = vec![
        format!("Repository name: {repository_name}"),
        format!("Local deterministic draft: {local_draft}"),
    ];
    if is_scout {
        sections.push("Swarm role: Scout is the protected projects-root worker for deliberate cross-repository discovery, worktree preparation, and coordinated work that Queen later divides among repository workers.".to_owned());
    }
    if let Some(description) = package_description {
        sections.push(format!("Package description: {description}"));
    }
    if let Some(description) = manifest_description {
        sections.push(format!("Manifest description: {description}"));
    }
    if let Some(excerpt) = readme_excerpt {
        sections.push(format!("README excerpt: {excerpt}"));
    }
    Ok(sections.join("\n"))
}

async fn read_first_available(workspace: &Path, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(content) = read_description_source(workspace, name).await {
            return Some(content);
        }
    }
    None
}

async fn read_description_source(workspace: &Path, name: &str) -> Option<String> {
    let path = workspace.join(name);
    let metadata = tokio::fs::symlink_metadata(&path).await.ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_DESCRIPTION_SOURCE_BYTES
    {
        return None;
    }
    tokio::fs::read_to_string(path).await.ok()
}

fn manifest_description(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "description")
            .then(|| value.trim().trim_matches(['\'', '"']))
            .and_then(clean_summary)
    })
}

fn readme_summary(content: &str) -> Option<String> {
    let mut paragraph = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if line.starts_with('#')
            || line.starts_with("![")
            || line.starts_with("[![")
            || line.starts_with('<')
            || line.starts_with("---")
        {
            continue;
        }
        paragraph.push(line);
        if paragraph.join(" ").len() >= 600 {
            break;
        }
    }
    clean_summary(&paragraph.join(" "))
}

pub(super) fn clean_summary(value: &str) -> Option<String> {
    let normalized = value
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() || normalized.chars().any(char::is_control) {
        return None;
    }
    let end = normalized
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| *end <= 2_000)
        .last()
        .unwrap_or(0);
    (end > 0).then(|| normalized[..end].trim().to_owned())
}

pub(super) async fn start_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
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

/// Opens a scratch shell in a worker's workspace.
///
/// Deliberately NOT a worker lifecycle operation. It does not wake the worker,
/// does not touch its state, and does not clear its errors the way `start_worker`
/// does — it borrows the workspace path and nothing else. The session it returns
/// is unbound, so the roster never shows it.
///
/// Ungated on purpose: anyone holding the operator token can already create a
/// worker that runs arbitrary code, so a shell exposes no capability the token
/// did not already carry. It makes it direct rather than new.
///
/// # Errors
/// Returns an error when unauthorized, the size is invalid, the worker is
/// unknown, or the terminal host refuses.
pub(super) async fn open_shell(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<StartWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    require_valid_size(request.rows, request.columns)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let session_id = open_worker_shell(
        &state,
        worker_id,
        TerminalSize::new(request.rows, request.columns),
    )
    .await?;
    Ok(Json(serde_json::json!({ "session_id": session_id.to_string() })).into_response())
}

pub(super) async fn stop_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    // The operator pressing Stop records no reason, and that is honest: an
    // absent reason means "not recorded" rather than a guess.
    let view = stand_worker_down(&state, worker_id, None).await?;
    Ok(Json(view).into_response())
}

/// Stands a worker down: stops its session, releases what that session held,
/// and clears its error state.
///
/// Extracted so the operator's stop button and Queen's sleep tool are the SAME
/// operation rather than two that drift. The guard about whether a given caller
/// MAY stop this worker belongs to the caller, not here — the operator may stop
/// anything, and Queen is refused while the worker holds Active work.
///
/// Releasing the session's assignments is what makes the worker wakeable again
/// afterwards: the task keeps its assigned worker, only the session binding
/// ends, so a later assignment queues a fresh guarded wake.
pub(super) async fn stand_worker_down(
    state: &Arc<AppState>,
    worker_id: WorkerId,
    ended: Option<(&str, &str)>,
) -> Result<crate::WorkerView, ApiError> {
    let _guard = state.worker_lifecycle.lock().await;
    let is_scout = task_store(state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?
        == Some(worker_id);
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    if let Some(session_id) = profile.active_session_id {
        request_host(state, HostRequest::Stop { session_id }).await?;
        task_store(state)?
            .release_worker_session_because(session_id, ended)
            .map_err(|error| task_store_error(&error))?;
        task_store(state)?
            .release_session_assignments(session_id)
            .map_err(|error| task_store_error(&error))?;
        // ONE SESSION, ONE OFFER. The classifier reads a settings file at
        // process start and reports nothing back, so exactly-once cannot be
        // enforced where the command actually runs. What IS enforceable is
        // that a grant reaches one session and never a second, and this is
        // where that session ends.
        //
        // Spending them is not conditional on the stop succeeding cleanly: a
        // grant that survives a messy shutdown is the standing rule the
        // operator refused, and erring toward spent errs toward asking again.
        match task_store(state)?.consume_command_grants(worker_id) {
            Ok(0) => {}
            Ok(spent) => tracing::info!(spent, "approved-command grants spent with the session"),
            Err(error) => tracing::warn!(%error, "could not spend approved-command grants"),
        }
    }
    state.worker_errors.write().await.remove(&worker_id);
    state
        .worker_recovery_attempts
        .write()
        .await
        .remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(worker_view(
        profile,
        WorkerViewFacts {
            system_role: is_scout.then_some("scout"),
            ..WorkerViewFacts::default()
        },
    ))
}

#[cfg(test)]
mod description_tests {
    use super::*;

    #[tokio::test]
    async fn repository_draft_prefers_bounded_manifest_metadata() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(
            directory.path().join("package.json"),
            r#"{"name":"meadow","description":"Manages customer gardens and seasonal plans."}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            directory.path().join("README.md"),
            "# Ignore this\n\nA less precise summary.",
        )
        .await
        .unwrap();

        let draft = repository_description_draft(directory.path())
            .await
            .unwrap();
        assert!(draft.starts_with("meadow owns Manages customer gardens and seasonal plans"));
        assert!(draft.contains("JavaScript or TypeScript application"));
        assert!(draft.contains("Queen or Scout"));
    }

    #[tokio::test]
    async fn repository_draft_uses_readme_without_following_markdown_badges() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(
            directory.path().join("README.md"),
            "# Clover\n\n[![build](badge.svg)](ci)\n\nClover coordinates durable worker sessions for software teams.\n\n## Setup\nDo not include this.",
        )
        .await
        .unwrap();

        let draft = repository_description_draft(directory.path())
            .await
            .unwrap();
        assert!(draft.contains("coordinates durable worker sessions for software teams"));
        assert!(!draft.contains("badge.svg"));
        assert!(!draft.contains("Do not include this"));
    }

    #[tokio::test]
    async fn claude_context_contains_only_bounded_metadata_not_manifest_scripts() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(
            directory.path().join("package.json"),
            r#"{"name":"clover","description":"Coordinates garden work.","scripts":{"private":"do-not-send"}}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            directory.path().join("README.md"),
            "# Clover\n\nA friendly garden coordination tool.\n\nSECRET SECOND PARAGRAPH",
        )
        .await
        .unwrap();

        let context = repository_description_context(directory.path(), false)
            .await
            .unwrap();
        assert!(context.contains("Coordinates garden work"));
        assert!(context.contains("friendly garden coordination tool"));
        assert!(!context.contains("do-not-send"));
        assert!(!context.contains("SECRET SECOND PARAGRAPH"));
        assert!(context.len() < super::super::worker_description_ai::MAX_CONTEXT_BYTES);
    }

    #[tokio::test]
    async fn scout_context_preserves_cross_repository_routing_role() {
        let directory = tempfile::tempdir().unwrap();
        let context = repository_description_context(directory.path(), true)
            .await
            .unwrap();

        assert!(context.contains("Swarm role: Scout"));
        assert!(context.contains("cross-repository"));
        assert!(context.contains(SCOUT_ROUTING_DESCRIPTION));
        assert!(!context.contains("Cross-repository coordination stays with Queen or Scout"));
    }
}

/// Reports the repository state of one worker.
///
/// Scoped to a single worker on purpose. The context bar that shows this is
/// per-selected-worker, so the read is too: a roster of thirty-two workers
/// never turns into thirty-two subprocesses on refresh.
///
/// # Errors
/// Returns an error when the worker is unknown or its repository cannot be read.
pub(super) async fn worker_repository(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<WorkerId>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    let status = repository_status(&profile.workspace).await;
    let body = status.as_deref().map(parse_repository_status);
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// Runs one bounded porcelain status. A repository that is missing, is not a
/// Git checkout, or takes too long reports nothing rather than failing the
/// worker view that asked.
async fn repository_status(workspace: &str) -> Option<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(workspace)
            .args(["status", "--porcelain", "--branch"])
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Checks reported SHAs against the repository, ONCE, and says what it found.
///
/// The verdict is a snapshot taken now. It is never recomputed: this repository
/// squash-merges and rebases as a matter of routine, so a SHA that was real when
/// reported routinely stops existing later. Re-checking would turn green
/// evidence red weeks after the fact for work that was perfectly correct, and a
/// check that fails on correct input teaches its reader to ignore it.
///
/// A WORKSPACE THAT IS NOT A CHECKOUT IS NOT AN ERROR. Work in a directory
/// nobody put under version control still has to be able to close, so that
/// reports `NotARepository` with every commit `Unchecked` — which is a
/// different answer from `Missing`, and deliberately so.
pub(super) async fn verify_reported_commits(
    workspace: &str,
    shas: &[String],
) -> (CommitRepositoryState, Vec<TaskCommit>) {
    if git(workspace, &["rev-parse", "--git-dir"]).await.is_none() {
        let unchecked = shas
            .iter()
            .map(|sha| TaskCommit {
                sha: sha.clone(),
                verdict: CommitVerdict::Unchecked,
                subject: String::new(),
                changed_paths: Vec::new(),
            })
            .collect();
        return (CommitRepositoryState::NotARepository, unchecked);
    }
    let mut verified = Vec::with_capacity(shas.len());
    for sha in shas {
        verified.push(verify_one(workspace, sha).await);
    }
    (CommitRepositoryState::Read, verified)
}

async fn verify_one(workspace: &str, sha: &str) -> TaskCommit {
    // THE TYPE IS ASKED FOR, not just existence. A tag or a tree whose name a
    // worker pasted is not a commit, and reporting it as present would put a
    // non-commit into a record the next step reads as one.
    let kind = git(workspace, &["cat-file", "-t", sha]).await;
    if kind.as_deref().map(str::trim) != Some("commit") {
        return TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Missing,
            subject: String::new(),
            changed_paths: Vec::new(),
        };
    }
    let subject = git(workspace, &["show", "--no-patch", "--format=%s", sha])
        .await
        .unwrap_or_default()
        .trim()
        .to_owned();
    let changed_paths = git(workspace, &["show", "--pretty=format:", "--name-only", sha])
        .await
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();
    // REACHABLE, not merely present. `cat-file` still finds a commit that a
    // rebase orphaned, right up until it is collected — so a dangling SHA would
    // otherwise read exactly like a live one.
    let reached = git(
        workspace,
        &[
            "for-each-ref",
            "--count=1",
            "--format=%(refname)",
            "--contains",
            sha,
        ],
    )
    .await
    .is_some_and(|refs| !refs.trim().is_empty());
    TaskCommit {
        sha: sha.to_owned(),
        verdict: if reached {
            CommitVerdict::Present
        } else {
            CommitVerdict::Unreachable
        },
        subject,
        changed_paths,
    }
}

/// One bounded git invocation. Anything that fails, is not a checkout, or takes
/// too long answers `None` rather than failing the report that asked.
async fn git(workspace: &str, args: &[&str]) -> Option<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(workspace)
            .args(args)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// What `git status --porcelain --branch` says about a worker's repository.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct RepositoryState {
    /// The checked-out branch, or `None` while `HEAD` is detached.
    branch: Option<String>,
    detached: bool,
    /// Paths differing from `HEAD`, staged or not.
    changed_paths: usize,
}

/// Reads one repository's state from a single porcelain status.
///
/// Parsed rather than shelled out to twice: the branch header and the change
/// list come from the same invocation, so the two can never disagree, and a
/// worker costs one subprocess rather than two.
fn parse_repository_status(status: &str) -> RepositoryState {
    let mut branch = None;
    let mut detached = false;
    let mut changed_paths = 0;
    for line in status.lines() {
        let Some(header) = line.strip_prefix("## ") else {
            if !line.trim().is_empty() {
                changed_paths += 1;
            }
            continue;
        };
        if header.starts_with("HEAD (no branch)") {
            detached = true;
            continue;
        }
        // `main...origin/main [ahead 1]` names the local branch first.
        let name = header
            .split("...")
            .next()
            .unwrap_or(header)
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if !name.is_empty() {
            branch = Some(name.to_owned());
        }
    }
    RepositoryState {
        branch,
        detached,
        changed_paths,
    }
}

#[cfg(test)]
mod repository_tests {
    use super::*;

    #[test]
    fn reads_a_clean_branch() {
        let state = parse_repository_status("## main...origin/main");

        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!(state.changed_paths, 0);
        assert!(!state.detached);
    }

    #[test]
    fn counts_every_differing_path_whether_staged_or_not() {
        let state = parse_repository_status(
            "## work/thing...origin/work/thing [ahead 2]\n M src/lib.rs\nA  src/new.rs\n?? notes.md\n",
        );

        assert_eq!(state.branch.as_deref(), Some("work/thing"));
        assert_eq!(state.changed_paths, 3);
    }

    #[test]
    fn reports_a_branch_with_no_upstream() {
        assert_eq!(
            parse_repository_status("## local-only").branch.as_deref(),
            Some("local-only")
        );
    }

    #[test]
    fn names_no_branch_while_head_is_detached() {
        let state = parse_repository_status("## HEAD (no branch)\n M src/lib.rs");

        assert!(state.detached);
        assert_eq!(state.branch, None);
        assert_eq!(state.changed_paths, 1);
    }

    #[test]
    fn treats_an_empty_status_as_a_repository_with_nothing_to_say() {
        assert_eq!(
            parse_repository_status(""),
            RepositoryState {
                branch: None,
                detached: false,
                changed_paths: 0,
            }
        );
    }
}

/// Claims a worker for this device without sending it anything.
///
/// [ADR 0049]. A phone showing "On another desktop" named a device the operator
/// may have walked away from and offered nothing to do about it; the only
/// remedy was to type into the worker, which sends real input to a real
/// provider. Reclaiming a screen and instructing an agent are not the same act.
///
/// Granted, not negotiated: engagement identifies a device, but every device
/// here belongs to one operator, so a second device asking is that person
/// saying where they now are rather than two parties contending.
///
/// The lease is shorter than typing earns and viewing does not renew it.
/// Engagement holds back a worker's coordination, so a claim that costs nothing
/// to make must not silence a worker for as long as demonstrated presence does.
/// Typing converts it to a full lease through the path that already exists.
/// Terminal geometry is untouched: ADR 0045 gives resize authority to the
/// device actually typing, and this device is not.
pub(super) async fn claim_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((worker_id, device_id)): AxumPath<(String, String)>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "device_id must be a UUID",
        )
    })?;
    let store = task_store(&state)?;
    let session_id = store
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?
        .active_session_id
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "worker_not_running",
                "a sleeping worker has no session to claim",
            )
        })?;
    store
        .renew_worker_engagement(
            session_id,
            Some(device_id),
            unix_timestamp(),
            VIEWING_ENGAGEMENT_LEASE_SECONDS,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Verification tests that drive a REAL repository.
///
/// A mocked git would let every one of these pass while the command strings are
/// wrong, which is the whole failure this repository keeps writing down. These
/// build a checkout in a temp directory and ask the same binary production asks.
#[cfg(test)]
mod commit_verification_tests {
    use super::*;
    use std::process::Command;

    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn repository_with_one_commit() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path();
        git_in(path, &["init", "--quiet", "--initial-branch=main"]);
        git_in(path, &["config", "user.email", "worker@example.test"]);
        git_in(path, &["config", "user.name", "Worker"]);
        std::fs::create_dir_all(path.join("docs")).expect("docs dir");
        std::fs::write(path.join("docs/note.md"), "a note\n").expect("write");
        git_in(path, &["add", "docs/note.md"]);
        git_in(path, &["commit", "--quiet", "-m", "docs: write a note"]);
        let sha = git_in(path, &["rev-parse", "HEAD"]);
        (dir, sha)
    }

    #[tokio::test]
    async fn a_real_commit_is_present_and_carries_the_paths_it_touched() {
        let (dir, sha) = repository_with_one_commit();
        let (state, commits) =
            verify_reported_commits(dir.path().to_str().unwrap(), std::slice::from_ref(&sha)).await;

        assert_eq!(state, CommitRepositoryState::Read);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].verdict, CommitVerdict::Present);
        assert_eq!(commits[0].sha, sha);
        assert_eq!(commits[0].subject, "docs: write a note");
        // The paths are the point: they are what makes "docs-only" derivable
        // later instead of asserted.
        assert_eq!(commits[0].changed_paths, vec!["docs/note.md".to_owned()]);
    }

    #[tokio::test]
    async fn a_sha_that_does_not_exist_is_missing() {
        let (dir, _) = repository_with_one_commit();
        let invented = "0123456789abcdef0123456789abcdef01234567";
        let (state, commits) =
            verify_reported_commits(dir.path().to_str().unwrap(), &[invented.to_owned()]).await;

        assert_eq!(state, CommitRepositoryState::Read);
        assert_eq!(commits[0].verdict, CommitVerdict::Missing);
        assert!(commits[0].changed_paths.is_empty());
    }

    /// A rebase orphans a commit and `cat-file` still finds it.
    ///
    /// This is the case existence alone gets wrong: the object survives until
    /// it is collected, so a dangling SHA reads exactly like a live one unless
    /// something asks whether a ref still reaches it.
    #[tokio::test]
    async fn an_orphaned_commit_is_unreachable_rather_than_present() {
        let (dir, first) = repository_with_one_commit();
        let path = dir.path();
        std::fs::write(path.join("docs/second.md"), "another\n").expect("write");
        git_in(path, &["add", "docs/second.md"]);
        git_in(path, &["commit", "--quiet", "-m", "docs: and another"]);
        let orphaned = git_in(path, &["rev-parse", "HEAD"]);
        git_in(path, &["reset", "--hard", "--quiet", &first]);

        let (_, commits) =
            verify_reported_commits(path.to_str().unwrap(), std::slice::from_ref(&orphaned)).await;

        assert_eq!(
            commits[0].verdict,
            CommitVerdict::Unreachable,
            "an orphaned commit still exists as an object; it must not read as present"
        );
    }

    #[tokio::test]
    async fn a_workspace_that_is_not_a_repository_is_unchecked_rather_than_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (state, commits) =
            verify_reported_commits(dir.path().to_str().unwrap(), &["abc1234".to_owned()]).await;

        assert_eq!(state, CommitRepositoryState::NotARepository);
        // Unchecked, NOT Missing. Missing is an answer about the repository;
        // this is the absence of a repository to answer. Work in a directory
        // nobody version-controlled still has to be able to close.
        assert_eq!(commits[0].verdict, CommitVerdict::Unchecked);
    }

    #[tokio::test]
    async fn reporting_nothing_reads_the_repository_and_records_no_commits() {
        let (dir, _) = repository_with_one_commit();
        let (state, commits) = verify_reported_commits(dir.path().to_str().unwrap(), &[]).await;
        assert_eq!(state, CommitRepositoryState::Read);
        assert!(commits.is_empty());
    }
}
