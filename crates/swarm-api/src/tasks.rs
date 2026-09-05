use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_domain::{TaskDetailsUpdate, TaskId, TaskPriority, TaskState, WorkerId};
use swarm_persistence::{
    MAX_OPEN_TASKS_PER_ORDER, MAX_TASK_ACTIVITY_NOTE_BYTES, MAX_TASK_ACTIVITY_PAGE, TaskStoreError,
};

use super::{
    ApiError, AppState, application_error, authorize, parse_task_id, task_service, task_store,
    task_store_error,
};

#[derive(Debug, Deserialize)]
pub(super) struct CreateTaskRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: TaskPriority,
    workspace: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TransitionTaskRequest {
    state: TaskState,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AssignTaskRequest {
    worker_id: Option<WorkerId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrerequisiteRequest {
    prerequisite_id: TaskId,
    operation: swarm_domain::PrerequisiteOperation,
    reason: String,
}

pub(super) async fn change_prerequisite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<PrerequisiteRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let change = swarm_domain::TaskPrerequisiteChange {
        task_id: parse_task_id(&task_id)?,
        prerequisite_id: request.prerequisite_id,
        operation: request.operation,
        reason: request.reason,
    };
    let task = task_service(&state)?
        .change_operator_task_prerequisite(&change, crate::unix_timestamp())
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(task)).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskActivityQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReorderTasksRequest {
    task_ids: Vec<TaskId>,
}

pub(super) async fn list_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks = task_service(&state)?
        .list_board_tasks()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

/// Settled work, served separately from the board's polled list.
///
/// The board polls every 30 seconds and renders settled work inside a collapsed
/// panel, so shipping it on every poll cost about 3.4 MB a minute on the
/// operator's Hive to render a board whose actionable half is 99 rows. This is
/// fetched when that panel is opened instead.
pub(super) async fn list_settled_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks = task_service(&state)?
        .list_settled_tasks()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

pub(super) async fn create_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_service(&state)?
        .create_operator_task(
            &request.title,
            &request.description,
            request.priority,
            &request.workspace,
        )
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok((StatusCode::CREATED, Json(task)).into_response())
}

pub(super) async fn task_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(query): Query<TaskActivityQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let limit = query.limit.unwrap_or(30);
    validate_activity_limit(limit)?;
    let activity = task_store(&state)?
        .list_task_activity(parse_task_id(&task_id)?, limit)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(activity)).into_response())
}

pub(super) async fn recent_task_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TaskActivityQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let limit = query.limit.unwrap_or(100);
    validate_activity_limit(limit)?;
    let activity = task_store(&state)?
        .list_recent_task_activity(limit)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(activity)).into_response())
}

fn validate_activity_limit(limit: usize) -> Result<(), ApiError> {
    if !(1..=MAX_TASK_ACTIVITY_PAGE).contains(&limit) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_activity_limit",
            format!("task activity limit must be between 1 and {MAX_TASK_ACTIVITY_PAGE}"),
        ));
    }
    Ok(())
}

pub(super) async fn reorder_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ReorderTasksRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    if request.task_ids.len() > MAX_OPEN_TASKS_PER_ORDER {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_order",
            format!("task order cannot exceed {MAX_OPEN_TASKS_PER_ORDER} entries"),
        ));
    }
    let tasks = task_store(&state)?
        .reorder_open_tasks(&request.task_ids)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(tasks).into_response())
}

pub(super) async fn update_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TaskDetailsUpdate>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_service(&state)?
        .update_operator_task(parse_task_id(&task_id)?, &request)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(Json(task).into_response())
}

pub(super) async fn list_removed_tasks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let tasks = task_service(&state)?
        .list_removed_local_tasks()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(tasks)).into_response())
}

pub(super) async fn remove_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_service(&state)?
        .remove_operator_task(parse_task_id(&task_id)?)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) async fn restore_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task = task_service(&state)?
        .restore_operator_task(parse_task_id(&task_id)?)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    Ok(Json(task).into_response())
}

/// The operator agreeing that a task had nothing to deploy.
///
/// Queen approves these ordinarily. This exists so a Queen who is wedged, or
/// wrong, cannot leave finished work with no legal path to completed — which is
/// the state three tasks were in when this was written.
pub(super) async fn approve_completion_exemption(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let evidence = crate::task_store(&state)?
        .approve_completion_exemption(
            parse_task_id(&task_id)?,
            "operator",
            // The operator approving on the board IS the basis. They read the
            // task in front of them; there is no second party to cite.
            "Approved by the operator from the board.",
            crate::unix_timestamp(),
        )
        .map_err(|error| crate::task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(serde_json::json!({ "evidence": format!("{evidence:?}") })).into_response())
}

pub(super) async fn transition_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<TransitionTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let current = store
        .get_task(task_id)
        .map_err(|error| task_store_error(&error))?;
    if !current.state.can_transition_to(request.state) {
        return Err(task_store_error(&TaskStoreError::InvalidTransition {
            from: current.state,
            to: request.state,
        }));
    }
    if request.note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
        return Err(task_store_error(&TaskStoreError::InvalidTaskActivityNote));
    }
    let task = task_service(&state)?
        .transition_operator_task_with_note(task_id, request.state, &request.note)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_jira_transitions().await;
    Ok(Json(task).into_response())
}

pub(super) async fn assign_task(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Json(request): Json<AssignTaskRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let task = match request.worker_id {
        Some(worker_id) => task_service(&state)?.assign_operator_task(task_id, worker_id),
        None => task_service(&state)?.unassign_operator_task(task_id),
    }
    .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let task = task_store(&state)?
        .get_task(task.id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(task).into_response())
}
