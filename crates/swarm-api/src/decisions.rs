use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    ApiError, AppState, application_error, authorize, parse_decision_id, task_service, task_store,
    task_store_error,
};

#[derive(Debug, Deserialize)]
pub(super) struct ResolveDecisionRequest {
    action: String,
    #[serde(default)]
    note: String,
}

pub(super) async fn list_decisions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decisions = task_service(&state)?
        .list_visible_decisions(None)
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(decisions)).into_response())
}

pub(super) async fn resolve_decision(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision_id): Path<String>,
    Json(request): Json<ResolveDecisionRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    task_service(&state)?
        .resolve_operator_decision(decision_id, &request.action, &request.note)
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let decision = task_store(&state)?
        .get_decision_request(decision_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(decision).into_response())
}
