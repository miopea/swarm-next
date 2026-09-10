//! Clarification uses the existing exclusive coordinator and guarded transport.
use crate::{AppState, coordination_delivery, unix_timestamp};
use swarm_domain::ClarificationDeliveryOutcome;
use swarm_persistence::TaskStore;
use swarm_terminal::HostClient;

use crate::{ApiError, application_error, parse_decision_id, task_service};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AskClarificationRequest {
    id: swarm_domain::DecisionClarificationId,
    question: String,
}

pub(super) async fn reconcile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision): Path<String>,
    Json(request): Json<swarm_domain::ClarificationReconciliation>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_operator_credential(&state, &headers)?;
    if parse_decision_id(&decision)? != request.decision_id {
        return Err(ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "decision_mismatch",
            "Recovery must name the decision being viewed",
        ));
    }
    let saved = task_service(&state)?
        .reconcile_operator_clarification(&request, unix_timestamp())
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.coordination_wakeup.notify_one();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(saved)).into_response())
}

pub(super) async fn history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision): Path<String>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_operator_credential(&state, &headers)?;
    let history = task_service(&state)?
        .clarification_history(None, parse_decision_id(&decision)?)
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(history)).into_response())
}

pub(super) async fn ask(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(decision): Path<String>,
    Json(request): Json<AskClarificationRequest>,
) -> Result<Response, ApiError> {
    // General localhost authorization is not proof the human authored a question.
    crate::auth::authorize_operator_credential(&state, &headers)?;
    let saved = task_service(&state)?
        .ask_operator_clarification(
            request.id,
            parse_decision_id(&decision)?,
            &request.question,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    state.control_room_notify.notify_waiters();
    state.coordination_wakeup.notify_one();
    // The existing coordinator owns delivery. Return the durable receipt without
    // blocking this UI request on a busy terminal or spawning a detached sender.
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(saved)).into_response())
}

impl AppState {
    pub(super) async fn deliver_clarifications(&self, store: &TaskStore, client: &HostClient) {
        let claims = match store.claim_clarification_deliveries(unix_timestamp()) {
            Ok(claims) => claims,
            Err(error) => {
                tracing::warn!(message = %error, "clarification delivery queue could not be claimed");
                return;
            }
        };
        for (claim, submission) in
            coordination_delivery::submit_clarifications(store, client, claims).await
        {
            let outcome = match submission {
                Ok(coordination_delivery::TerminalSubmission::Acknowledged) => {
                    ClarificationDeliveryOutcome::Delivered
                }
                Ok(coordination_delivery::TerminalSubmission::Deferred(_)) => {
                    ClarificationDeliveryOutcome::DeferredBeforeWrite
                }
                // A rejected response can occur after a paste. Never guess that
                // retry is safe; an explicit reconciliation owns that choice.
                Ok(
                    coordination_delivery::TerminalSubmission::Rejected { .. }
                    | coordination_delivery::TerminalSubmission::Uncertain,
                )
                | Err(_) => ClarificationDeliveryOutcome::Uncertain,
            };
            match store.finish_clarification_delivery(&claim, outcome) {
                Ok(true) => self.control_room_notify.notify_waiters(),
                Ok(false) => {
                    tracing::warn!(clarification_id = %claim.clarification.id, "clarification claim no longer owns the result");
                }
                Err(error) => {
                    tracing::warn!(clarification_id = %claim.clarification.id, message = %error, "clarification result could not be persisted");
                }
            }
        }
    }
}
