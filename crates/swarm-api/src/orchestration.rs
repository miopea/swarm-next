use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_domain::{QueenAutonomyLevel, QueenAutonomyPolicy};

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

#[derive(Debug, Deserialize)]
pub(super) struct SetQueenAutonomyPolicyRequest {
    at_hive: QueenAutonomyLevel,
    away: QueenAutonomyLevel,
    night_watch: QueenAutonomyLevel,
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
