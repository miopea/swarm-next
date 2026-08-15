use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use swarm_domain::ControlRoomEventKind;
use swarm_terminal::{HostClient, HostRequest, HostResponse};

use crate::{ApiError, AppState, auth::authorize, host_unavailable, task_store_error};

pub(super) async fn request_host(
    state: &AppState,
    request: HostRequest,
) -> Result<HostResponse, ApiError> {
    let response = terminal_client(state)?
        .request(&request)
        .await
        .map_err(|error| host_unavailable(&error))?;
    if let HostResponse::Error { message, .. } = &response {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "terminal_operation_failed",
            message,
        ));
    }
    Ok(response)
}

pub(super) async fn authorized_request(
    state: &AppState,
    headers: &HeaderMap,
    request: HostRequest,
) -> Result<Json<HostResponse>, ApiError> {
    authorize(state, headers)?;
    Ok(Json(request_host(state, request).await?))
}

pub(super) async fn authorized_no_store_request(
    state: &AppState,
    headers: &HeaderMap,
    request: HostRequest,
) -> Result<Response, ApiError> {
    let Json(response) = authorized_request(state, headers, request).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)).into_response())
}

pub(super) fn terminal_client(state: &AppState) -> Result<&HostClient, ApiError> {
    state.terminal_host.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "terminal_host_unconfigured",
            "terminal host is not configured",
        )
    })
}

pub(super) fn record_session_event(state: &AppState) -> Result<(), ApiError> {
    if let Some(store) = &state.task_store {
        store
            .record_control_room_event(ControlRoomEventKind::SessionsChanged)
            .map_err(|error| task_store_error(&error))?;
    }
    state.control_room_notify.notify_waiters();
    Ok(())
}
