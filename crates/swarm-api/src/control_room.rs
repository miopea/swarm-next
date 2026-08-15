use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tokio::time::timeout;

use super::{ApiError, AppState, authorize, task_store, task_store_error};

#[derive(Debug, Deserialize)]
pub(super) struct ControlRoomEventsQuery {
    after: Option<i64>,
}

pub(super) async fn events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ControlRoomEventsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let after = query.after.unwrap_or(0).max(0);
    let notified = state.control_room_notify.notified();
    let mut page = task_store(&state)?
        .list_control_room_events(after)
        .map_err(|error| task_store_error(&error))?;
    if page.events.is_empty() && !page.reset_required {
        let _ = timeout(Duration::from_secs(20), notified).await;
        page = task_store(&state)?
            .list_control_room_events(after)
            .map_err(|error| task_store_error(&error))?;
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(page)).into_response())
}
