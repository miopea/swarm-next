use std::sync::Arc;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::attachments::AttachmentError;
use crate::{ApiError, AppState, attachment_error, authorize, task_store, task_store_error};

#[derive(Debug, Deserialize)]
pub(super) struct DogfoodReportsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateDogfoodReportRequest {
    expectation: String,
    observation: String,
    diagnostic_bundle: String,
    attachment_name: Option<String>,
}

#[derive(Serialize)]
struct DogfoodAttachmentResponse {
    name: String,
}

pub(super) async fn list_reports(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DogfoodReportsQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let reports = task_store(&state)?
        .list_dogfood_reports(query.limit.unwrap_or(20))
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(reports)).into_response())
}

pub(super) async fn create_report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDogfoodReportRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let report = task_store(&state)?
        .create_dogfood_report(
            &request.expectation,
            &request.observation,
            &request.diagnostic_bundle,
            request.attachment_name.as_deref(),
        )
        .map_err(|error| task_store_error(&error))?;
    Ok((StatusCode::CREATED, Json(report)).into_response())
}

pub(super) async fn upload_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let path = store
        .save(media_type, &body)
        .await
        .map_err(attachment_error)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attachment_path_unavailable",
                "private attachment name is not valid UTF-8",
            )
        })?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(DogfoodAttachmentResponse { name: name.into() }),
    )
        .into_response())
}

pub(super) async fn download_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let referenced = task_store(&state)?
        .dogfood_attachment_is_referenced(&name)
        .map_err(|error| task_store_error(&error))?;
    if !referenced {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "feedback_attachment_not_found",
            "the private report attachment was not found",
        ));
    }
    let store = state.attachment_store.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "attachment_store_unconfigured",
            "private attachment storage is not configured",
        )
    })?;
    let (bytes, media_type) = store.read(&name).await.map_err(attachment_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response_headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(media_type)
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename={name}"))
            .map_err(|_| attachment_error(AttachmentError::Unavailable))?,
    );
    Ok((response_headers, bytes).into_response())
}
