use std::{
    collections::VecDeque,
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
use swarm_domain::{ProviderKind, WorkerId, WorkerProfile};
use swarm_terminal::{HostRequest, ProviderActivity, TerminalSize};

use super::{
    ApiError, AppState, authorize, default_provider, default_terminal_columns,
    default_terminal_rows, parse_worker_id, provider_activity, require_valid_size, task_store,
    task_store_error, terminal_host::request_host,
    worker_runtime::{reconcile_worker_bindings, start_worker_process},
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
    autostart: Option<bool>,
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
    let provider_activity = provider_activity::refresh(&state, &profiles, &live).await;
    let awaiting_operator = task_store(&state)?
        .workers_awaiting_operator()
        .map_err(|error| task_store_error(&error))?;
    let errors = state.worker_errors.read().await;
    let workers = profiles
        .into_iter()
        .map(|profile| {
            let running = profile
                .active_session_id
                .is_some_and(|session_id| live.contains(&session_id));
            let runtime_error = errors.get(&profile.id).cloned();
            let needs_operator = awaiting_operator.contains(&profile.id);
            let activity = profile
                .active_session_id
                .and_then(|session_id| provider_activity.get(&session_id).copied())
                .unwrap_or(ProviderActivity::Unknown);
            worker_view(profile, running, needs_operator, runtime_error, activity)
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
    let profile = task_store(&state)?
        .create_worker(
            &request.name,
            request.provider,
            &workspace,
            request.autostart,
            position,
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        StatusCode::CREATED,
        Json(worker_view(
            profile,
            false,
            false,
            None,
            ProviderActivity::Unknown,
        )),
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

pub(super) async fn update_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<UpdateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .update_worker_profile(worker_id, request.name.as_deref(), request.autostart)
        .map_err(|error| task_store_error(&error))?;
    if request.autostart.is_some() {
        state.worker_errors.write().await.remove(&worker_id);
        state
            .worker_recovery_attempts
            .write()
            .await
            .remove(&worker_id);
    }
    let running = profile.active_session_id.is_some();
    state.control_room_notify.notify_waiters();
    Ok(Json(worker_view(
        profile,
        running,
        false,
        None,
        ProviderActivity::Unknown,
    ))
    .into_response())
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

pub(super) async fn stop_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
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
    state
        .worker_recovery_attempts
        .write()
        .await
        .remove(&worker_id);
    state.control_room_notify.notify_waiters();
    let profile = task_store(&state)?
        .get_worker_profile(worker_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(worker_view(
        profile,
        false,
        false,
        None,
        ProviderActivity::Unknown,
    ))
    .into_response())
}
