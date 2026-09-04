use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use swarm_terminal::{HistoryCursor, HostRequest, HostResponse};

use crate::{
    ApiError, AppState, authorize, parse_session_id,
    terminal_host::{authorized_no_store_request, request_host},
};

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    segment: Option<u64>,
    record: Option<u32>,
}

#[derive(Serialize)]
pub(super) struct LiveSessionsResponse {
    #[serde(rename = "type")]
    kind: &'static str,
    sessions: Vec<LiveSessionView>,
}

#[derive(Serialize)]
struct LiveSessionView {
    #[serde(flatten)]
    session: swarm_terminal::HostSessionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_outcome: Option<swarm_domain::ConversationRecoveryState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirmed_selection: Option<swarm_domain::ProviderConversationSelection>,
}

pub(super) async fn list_live_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<LiveSessionsResponse>, ApiError> {
    authorize(&state, &headers)?;
    let response = request_host(&state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    let ids = sessions
        .iter()
        .filter(|session| session.running)
        .map(|session| session.session_id)
        .collect::<Vec<_>>();
    let mut outcomes = state
        .task_store
        .as_ref()
        .map(|store| store.provider_recovery_outcomes(&ids))
        .transpose()
        .map_err(|error| crate::task_store_error(&error))?
        .unwrap_or_default();
    let candidates = sessions
        .iter()
        .filter(|session| session.running)
        .filter_map(|session| {
            session
                .provider_selection
                .map(|selection| (session.session_id, selection))
        })
        .collect::<Vec<_>>();
    let mut confirmed = state
        .task_store
        .as_ref()
        .map(|store| store.confirmed_provider_selections(&candidates))
        .transpose()
        .map_err(|error| crate::task_store_error(&error))?
        .unwrap_or_default();
    Ok(Json(LiveSessionsResponse {
        kind: "sessions",
        sessions: sessions
            .into_iter()
            .filter(|session| session.running)
            .map(|session| LiveSessionView {
                recovery_outcome: outcomes.remove(&session.session_id),
                confirmed_selection: confirmed.remove(&session.session_id),
                session,
            })
            .collect(),
    }))
}

pub(super) async fn diagnostics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::HistoryDiagnostics).await
}

pub(super) async fn list_retained_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized_no_store_request(&state, &headers, HostRequest::ListHistorySessions).await
}

pub(super) async fn read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Response, ApiError> {
    let cursor = match (query.segment, query.record) {
        (None, None) => None,
        (Some(segment), Some(record)) => Some(HistoryCursor { segment, record }),
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_history_cursor",
                "history cursor requires both segment and record",
            ));
        }
    };
    authorized_no_store_request(
        &state,
        &headers,
        HostRequest::ReadHistory {
            session_id: parse_session_id(&session_id)?,
            cursor,
        },
    )
    .await
}
