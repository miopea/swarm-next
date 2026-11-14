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
    /// Exact rendered question snapshot. Missing is legacy, not confirmation
    /// that this client displayed option descriptions.
    #[serde(default)]
    questions: Option<Vec<swarm_domain::DecisionQuestion>>,
    #[serde(default)]
    note: String,
    /// Which control the operator used, so a disputed resolution can be traced
    /// to where it came in. Reported by the client and recorded as given; it
    /// answers "where did this arrive from", not "who was allowed to send it".
    #[serde(default)]
    surface: String,
    /// Images the operator sent with this answer (ADR 0174).
    #[serde(default)]
    images: Vec<crate::decision_images::SentImage>,
}

pub(super) async fn list_decisions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decisions = task_service(&state)?
        .operator_decision_inbox()
        .map_err(application_error)?;
    let ids: Vec<_> = decisions.iter().map(|entry| entry.decision.id).collect();
    let mut images = task_store(&state)?
        .decision_images(&ids)
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .fold(
            std::collections::HashMap::<_, Vec<_>>::new(),
            |mut by_decision, image| {
                by_decision
                    .entry(image.decision_id)
                    .or_default()
                    .push(image);
                by_decision
            },
        );
    let listed: Vec<_> = decisions
        .iter()
        .map(|entry| WithImages {
            images: images.remove(&entry.decision.id).unwrap_or_default(),
            entry,
        })
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(listed)).into_response())
}

/// A request as the operator's page reads it, with the images the operator
/// sent on it (ADR 0174). The images are added here rather than on the shared
/// record so the record a worker engine reads does not change.
#[derive(serde::Serialize)]
struct WithImages<'a, T> {
    #[serde(flatten)]
    entry: &'a T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<swarm_persistence::DecisionImage>,
}

/// One image the operator sent with an answer or question on this request.
pub(super) async fn decision_image(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((decision_id, image_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let decision_id = parse_decision_id(&decision_id)?;
    let image_id = image_id.parse::<uuid::Uuid>().map_err(|_| {
        crate::decision_images::decision_image_error(
            crate::decision_images::DecisionImageError::NotFound,
        )
    })?;
    crate::decision_images::image_response(&state, task_store(&state)?, decision_id, image_id).await
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
    let images =
        crate::decision_images::write_sent(&state, task_store(&state)?, &request.images).await?;
    // Answers and an action are different resolutions of different record
    // shapes, and the store rejects each against the wrong one, so this only
    // has to route.
    let resolved = if request.answers.is_empty() {
        service.resolve_operator_decision_with_images(
            decision_id,
            &request.action,
            &request.note,
            &request.surface,
            &images,
        )
    } else {
        service.answer_operator_decision_from_snapshot(
            decision_id,
            &request.answers,
            &request.note,
            &request.surface,
            request.questions.as_deref(),
            &images,
        )
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            crate::decision_images::discard(&state, &images).await;
            return Err(application_error(error));
        }
    };
    tracing::info!(
        decision_id = %decision_id,
        action = %if request.answers.is_empty() { request.action.as_str() } else { "answered" },
        answered_questions = request.answers.len(),
        images = images.len(),
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
    let store = task_store(&state)?;
    let decision = store
        .get_decision_request(decision_id)
        .map_err(|error| task_store_error(&error))?;
    // The page puts this in place of the card it answered, so it carries what
    // was sent with it, as the inbox listing does.
    let images = store
        .decision_images(&[decision_id])
        .map_err(|error| task_store_error(&error))?;
    Ok(Json(WithImages {
        entry: &decision,
        images,
    })
    .into_response())
}
