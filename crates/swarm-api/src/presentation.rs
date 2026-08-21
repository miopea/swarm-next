use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_persistence::{PresentationColorTheme, PresentationDeviceClass, PresentationPreferences};

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

#[derive(Debug, Deserialize)]
pub(super) struct SetStartSurfaceRequest {
    start_surface: String,
}

/// The screen Swarm opens on, for every device.
///
/// Not device-scoped, unlike everything else in this module. The operator asked
/// for one choice used everywhere, because a phone landing somewhere a desktop
/// would not is the problem being solved.
pub(super) async fn start_surface(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let start_surface = task_store(&state)?
        .start_surface()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "start_surface": start_surface })),
    )
        .into_response())
}

pub(super) async fn set_start_surface(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetStartSurfaceRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let start_surface = task_store(&state)?
        .set_start_surface(&request.start_surface)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(Json(serde_json::json!({ "start_surface": start_surface })).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct SetPresentationPreferencesRequest {
    color_theme: PresentationColorTheme,
    terminal_keys_visible: bool,
}

pub(super) async fn presentation_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_class): Path<PresentationDeviceClass>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preferences = task_store(&state)?
        .presentation_preferences(device_class)
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(preferences)).into_response())
}

pub(super) async fn set_presentation_preferences(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_class): Path<PresentationDeviceClass>,
    Json(request): Json<SetPresentationPreferencesRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let preferences = task_store(&state)?
        .set_presentation_preferences(
            PresentationPreferences {
                device_class,
                color_theme: request.color_theme,
                terminal_keys_visible: request.terminal_keys_visible,
                configured: true,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(preferences)).into_response())
}
