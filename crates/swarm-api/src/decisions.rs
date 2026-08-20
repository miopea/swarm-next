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
    /// The chosen button. Empty when answering an interview, which has none.
    #[serde(default)]
    action: String,
    /// Answers keyed by question header. Present makes this an interview
    /// answer rather than a ruling.
    #[serde(default)]
    answers: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    note: String,
    /// Which control the operator used, so a disputed resolution can be traced
    /// to where it came in. Reported by the client and recorded as given; it
    /// answers "where did this arrive from", not "who was allowed to send it".
    #[serde(default)]
    surface: String,
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
    let service = task_service(&state)?;
    // Answers and an action are different resolutions of different record
    // shapes, and the store rejects each against the wrong one, so this only
    // has to route.
    let resolved = if request.answers.is_empty() {
        service.resolve_operator_decision(
            decision_id,
            &request.action,
            &request.note,
            &request.surface,
        )
    } else {
        service.answer_operator_decision(
            decision_id,
            &request.answers,
            &request.note,
            &request.surface,
        )
    }
    .map_err(application_error)?;
    tracing::info!(
        decision_id = %decision_id,
        action = %if request.answers.is_empty() { request.action.as_str() } else { "answered" },
        answered_questions = request.answers.len(),
        surface = %if request.surface.is_empty() { "unreported" } else { &request.surface },
        "operator resolved a decision"
    );
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
