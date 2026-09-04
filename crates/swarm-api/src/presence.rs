use std::{str::FromStr, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_domain::{PresenceDeviceClass, PresenceDeviceId, PresenceMode, PresenceObservationState};

use super::{ApiError, AppState, application_error, authorize, task_service, unix_timestamp};

#[derive(Debug, Deserialize)]
pub(super) struct SetPresenceRequest {
    manual_mode: Option<PresenceMode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NightWatchRequest {
    enabled: bool,
    timezone: String,
    start_minute: u16,
    end_minute: u16,
}

pub(super) async fn night_watch_configuration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let config = task_service(&state)?
        .night_watch_configuration()
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(config)).into_response())
}

pub(super) async fn set_night_watch_configuration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<NightWatchRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let config = swarm_persistence::NightWatchConfiguration::new(
        request.enabled,
        &request.timezone,
        request.start_minute,
        request.end_minute,
    )
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_night_watch_schedule",
            "choose a valid IANA time zone and different start/end times within the day",
        )
    })?;
    if task_service(&state)?
        .set_night_watch_configuration(&config)
        .map_err(application_error)?
    {
        state.control_room_notify.notify_waiters();
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(config)).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct PresenceObservationRequest {
    device_class: PresenceDeviceClass,
    state: PresenceObservationState,
    #[serde(default)]
    desktop_return: bool,
}

pub(super) async fn operator_presence(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let presence = task_service(&state)?
        .operator_presence(unix_timestamp())
        .map_err(application_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}

pub(super) async fn set_operator_presence(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetPresenceRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let (presence, changed) = task_service(&state)?
        .set_operator_presence(request.manual_mode, unix_timestamp())
        .map_err(application_error)?;
    if changed {
        state.control_room_notify.notify_waiters();
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}

pub(super) async fn observe_presence_device(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    Json(request): Json<PresenceObservationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let device_id = PresenceDeviceId::from_str(&device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "presence device ID must be a UUID",
        )
    })?;
    let (presence, changed) = task_service(&state)?
        .observe_operator_device(
            device_id,
            request.device_class,
            request.state,
            request.desktop_return,
            unix_timestamp(),
        )
        .map_err(application_error)?;
    if changed {
        state.control_room_notify.notify_waiters();
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(presence)).into_response())
}
