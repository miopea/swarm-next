use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::{QueenAutonomyLevel, QueenAutonomyPolicy};
use swarm_persistence::AUTOMATIC_WAKE_BATCH_LIMIT;

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

#[derive(Debug, Deserialize)]
pub(super) struct SetQueenAutonomyPolicyRequest {
    at_hive: QueenAutonomyLevel,
    away: QueenAutonomyLevel,
    night_watch: QueenAutonomyLevel,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetQueenAutomationRequest {
    enabled: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CoordinatorStatusResponse {
    completed_actions: usize,
    queen_calls_avoided: usize,
    uncertain_actions: usize,
    queued_actions: usize,
    stale_attention_actions: usize,
    worker_exit_attention_actions: usize,
    unstarted_attention_actions: usize,
    last_action_at: Option<i64>,
    automatic_start_admission: super::runtime::CoordinatorStartAdmission,
    automatic_start_batch_limit: usize,
}

pub(super) async fn queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = task_store(&state)?
        .queen_autonomy_policy()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

pub(super) async fn set_queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetQueenAutonomyPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = task_store(&state)?
        .set_queen_autonomy_policy(
            QueenAutonomyPolicy {
                at_hive: request.at_hive,
                away: request.away,
                night_watch: request.night_watch,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

pub(super) async fn queen_automation_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

pub(super) async fn coordinator_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = state
        .coordinator_status()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CoordinatorStatusResponse {
            completed_actions: status.completed_actions,
            queen_calls_avoided: status.queen_calls_avoided,
            uncertain_actions: status.uncertain_actions,
            queued_actions: status.queued_actions,
            stale_attention_actions: status.stale_attention_actions,
            worker_exit_attention_actions: status.worker_exit_attention_actions,
            unstarted_attention_actions: status.unstarted_attention_actions,
            last_action_at: status.last_action_at,
            automatic_start_admission: state.coordinator_start_admission(),
            automatic_start_batch_limit: usize::from(AUTOMATIC_WAKE_BATCH_LIMIT),
        }),
    )
        .into_response())
}

pub(super) async fn set_queen_automation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetQueenAutomationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .set_queen_automation_enabled(request.enabled, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

pub(super) async fn run_queen_automation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .request_queen_automation_run(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}
