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
use swarm_domain::{
    CheckGateLevel, CiObservation, IndependentCheckState, MustNotTouchReport,
    independent_check_state,
};

use super::{
    ApiError, AppState, application_error, authorize, parse_task_id, task_service, task_store,
    task_store_error, unix_timestamp,
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
/// Beside it, the worker's independent check of those commits (ADR 0171),
/// which is shown and never gates anything; null when the task built no code.
#[derive(Serialize)]
struct TaskCiView {
    level: CheckGateLevel,
    recorded_commits: usize,
    observation: Option<CiObservation>,
    independent_check: Option<IndependentCheckState>,
    /// The task's must-not-touch patterns against its commits (ADR 0172),
    /// also shown and never gating; null when the task has none.
    must_not_touch: Option<MustNotTouchReport>,
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
    let report = store
        .task_commit_report(task_id)
        .map_err(|error| task_store_error(&error))?;
    let observation = store
        .ci_observation(task_id)
        .map_err(|error| task_store_error(&error))?;
    let latest_check = store
        .latest_independent_check(task_id)
        .map_err(|error| task_store_error(&error))?;
    let must_not_touch = task_service(&state)?
        .must_not_touch_report(task_id)
        .map_err(application_error)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TaskCiView {
            level,
            recorded_commits: report.as_ref().map_or(0, |report| report.commits.len()),
            observation,
            independent_check: independent_check_state(report.as_ref(), latest_check.as_ref()),
            must_not_touch,
        }),
    )
        .into_response())
}
