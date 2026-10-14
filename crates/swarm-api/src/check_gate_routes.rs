//! The check gate's routes (ADR 0115): this Hive's level, and CI as last read
//! for one task.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::{CheckGateLevel, CiObservation};

use super::{
    ApiError, AppState, authorize, parse_task_id, task_store, task_store_error, unix_timestamp,
};

/// The check gate in effect, and the level this Hive's Apiary sets, if any:
/// that one is locked here and only its Keeper can change it (ADR 0113).
#[derive(Serialize)]
struct CheckGateView {
    level: CheckGateLevel,
    set_by_apiary: Option<CheckGateLevel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetCheckGateRequest {
    level: CheckGateLevel,
}

pub(super) async fn check_gate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .check_gate()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CheckGateView {
            level: setting.level,
            set_by_apiary: setting.set_by_apiary,
        }),
    )
        .into_response())
}

pub(super) async fn set_check_gate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetCheckGateRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .set_check_gate_level(request.level, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CheckGateView {
            level: setting.level,
            set_by_apiary: setting.set_by_apiary,
        }),
    )
        .into_response())
}

/// What the check gate knows about one task: the level, how many commits the
/// task recorded (none means the gate never holds it), and CI as last read.
#[derive(Serialize)]
struct TaskCiView {
    level: CheckGateLevel,
    recorded_commits: usize,
    observation: Option<CiObservation>,
}

pub(super) async fn task_ci(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let level = store
        .check_gate()
        .map_err(|error| task_store_error(&error))?
        .level;
    let recorded_commits = store
        .task_commit_report(task_id)
        .map_err(|error| task_store_error(&error))?
        .map_or(0, |report| report.commits.len());
    let observation = store
        .ci_observation(task_id)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TaskCiView {
            level,
            recorded_commits,
            observation,
        }),
    )
        .into_response())
}
