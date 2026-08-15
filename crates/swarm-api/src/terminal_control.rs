use std::{path::PathBuf, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use swarm_domain::ProviderConversationId;
use swarm_terminal::{ClaudeConversationStart, HostRequest, HostResponse, TerminalSize};

use crate::{
    ApiError, AppState, parse_session_id, require_valid_size, task_store_error,
    terminal_host::{authorized_request, record_session_event},
};

#[derive(Debug, Deserialize)]
pub(super) struct StartSessionRequest {
    workspace: PathBuf,
    rows: u16,
    columns: u16,
}

#[derive(Debug, Deserialize)]
pub(super) struct OutputQuery {
    after: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct InputRequest {
    text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResizeRequest {
    rows: u16,
    columns: u16,
}

pub(super) async fn start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<StartSessionRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    require_valid_size(request.rows, request.columns)?;
    let response = authorized_request(
        &state,
        &headers,
        HostRequest::StartClaude {
            workspace: request.workspace,
            size: TerminalSize::new(request.rows, request.columns),
            conversation: ClaudeConversationStart::New {
                session_id: ProviderConversationId::new(),
            },
            mcp_config: None,
            allow_outside_roots: false,
        },
    )
    .await?;
    record_session_event(&state)?;
    Ok(response)
}

pub(super) async fn read_output(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(
        &state,
        &headers,
        HostRequest::Read {
            session_id: parse_session_id(&session_id)?,
            after_sequence: query.after,
        },
    )
    .await
}

pub(super) async fn write_input(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<InputRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    authorized_request(
        &state,
        &headers,
        HostRequest::Write {
            session_id: parse_session_id(&session_id)?,
            bytes: request.text.into_bytes(),
        },
    )
    .await
}

pub(super) async fn resize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<ResizeRequest>,
) -> Result<Json<HostResponse>, ApiError> {
    require_valid_size(request.rows, request.columns)?;
    authorized_request(
        &state,
        &headers,
        HostRequest::Resize {
            session_id: parse_session_id(&session_id)?,
            size: TerminalSize::new(request.rows, request.columns),
        },
    )
    .await
}

pub(super) async fn stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<HostResponse>, ApiError> {
    let session_id = parse_session_id(&session_id)?;
    let response = authorized_request(&state, &headers, HostRequest::Stop { session_id }).await?;
    if let Some(store) = &state.task_store {
        store
            .release_worker_session(session_id)
            .map_err(|error| task_store_error(&error))?;
        store
            .release_session_assignments(session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    record_session_event(&state)?;
    Ok(response)
}
