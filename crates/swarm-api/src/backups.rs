use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{ApiError, AppState, authorize, task_store, task_store_error};

pub(super) async fn download_database(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let backup = tempfile::NamedTempFile::new().map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("a temporary backup could not be created: {error}"),
        )
    })?;
    task_store(&state)?
        .backup_to(backup.path())
        .map_err(|error| task_store_error(&error))?;
    let bytes = std::fs::read(backup.path()).map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_unavailable",
            format!("the completed backup could not be read: {error}"),
        )
    })?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=swarm-next-hive.sqlite3"),
    );
    Ok((response_headers, bytes).into_response())
}
