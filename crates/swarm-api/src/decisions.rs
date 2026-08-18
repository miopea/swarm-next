use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_terminal::TerminalSize;

use super::{
    ApiError, AppState, application_error, authorize, parse_decision_id, task_service, task_store,
    task_store_error, worker_runtime,
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
    let resolved = task_service(&state)?
        .resolve_operator_decision(decision_id, &request.action, &request.note)
        .map_err(application_error)?;
    // Answering a worker is an explicit operator action. Wake that exact worker
    // so the durable reply does not sit indefinitely behind a sleeping process.
    // A failed wake remains visible on the worker and the queued reply stays
    // durable for a later retry; the recorded operator decision is never lost.
    if let Err(error) = worker_runtime::start_worker_process(
        &state,
        resolved.requesting_worker_id,
        TerminalSize::default(),
    )
    .await
    {
        state
            .worker_errors
            .write()
            .await
            .insert(resolved.requesting_worker_id, error.message.clone());
        tracing::warn!(
            decision_id = %decision_id,
            worker_id = %resolved.requesting_worker_id,
            message = %error.message,
            "decision requester could not be woken after operator resolution"
        );
    }
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let decision = task_store(&state)?
        .get_decision_request(decision_id)
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(decision).into_response())
}
