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
use swarm_domain::{ProviderKind, WorkerId, WorkerProfile};
use swarm_terminal::{HostRequest, ProviderActivity, TerminalSize};

use super::{
    ApiError, AppState, WorkerViewFacts, authorize, default_provider, default_terminal_columns,
    default_terminal_rows, parse_worker_id, provider_activity, require_valid_size, task_store,
    task_store_error,
    terminal_host::request_host,
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
    description: Option<String>,
    provider: Option<ProviderKind>,
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
    let live_ids = live.keys().copied().collect::<HashSet<_>>();
    let provider_activity = provider_activity::refresh(&state, &profiles, &live_ids).await;
    let awaiting_operator = task_store(&state)?
        .workers_awaiting_operator()
        .map_err(|error| task_store_error(&error))?;
    let unconfirmed = task_store(&state)?
        .workers_with_unconfirmed_delivery()
        .map_err(|error| task_store_error(&error))?;
    let errors = state.worker_errors.read().await;
    let scout_id = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?;
    let workers = profiles
        .into_iter()
        .map(|profile| {
            let running = profile
                .active_session_id
                .is_some_and(|session_id| live.contains_key(&session_id));
            let runtime_error = errors.get(&profile.id).cloned();
            let needs_operator = awaiting_operator.contains(&profile.id);
            let activity = profile
                .active_session_id
                .and_then(|session_id| provider_activity.get(&session_id).copied())
                .unwrap_or(ProviderActivity::Unknown);
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
                    provider_activity: activity,
                    system_role: is_scout.then_some("scout"),
                    last_output_at,
                    unconfirmed_delivery: unconfirmed.contains(&profile_id),
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

pub(super) async fn update_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
    Json(request): Json<UpdateWorkerRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let worker_id = parse_worker_id(&worker_id)?;
    let profile = task_store(&state)?
        .update_worker_profile(
            worker_id,
            request.name.as_deref(),
            request.description.as_deref(),
            request.provider,
            request.autostart,
        )
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

pub(super) async fn stop_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(worker_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _guard = state.worker_lifecycle.lock().await;
    let worker_id = parse_worker_id(&worker_id)?;
    let is_scout = task_store(&state)?
        .scout_worker_id()
        .map_err(|error| task_store_error(&error))?
        == Some(worker_id);
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
        WorkerViewFacts {
            system_role: is_scout.then_some("scout"),
            ..WorkerViewFacts::default()
        },
    ))
    .into_response())
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
