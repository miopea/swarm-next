use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;
use swarm_terminal::{HistoryCursor, HostRequest, HostResponse};

use crate::{
    ApiError, AppState, authorize, authorized_no_store_request, parse_session_id, request_host,
};

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    segment: Option<u64>,
    record: Option<u32>,
}

pub(super) async fn list_live_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HostResponse>, ApiError> {
    authorize(&state, &headers)?;
    let response = request_host(&state, HostRequest::ListSessions).await?;
    let HostResponse::Sessions { sessions } = response else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected response",
        ));
    };
    Ok(Json(HostResponse::Sessions {
        sessions: sessions
            .into_iter()
            .filter(|session| session.running)
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
