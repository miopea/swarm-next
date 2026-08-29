use std::sync::Arc;

use axum::extract::Path as AxumPath;
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

/// Whether this Hive can file a report anywhere, so the UI can stop implying it.
pub(super) async fn github_readiness(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let repository = state
        .github_feedback
        .as_ref()
        .map(crate::github_feedback::GithubFeedback::repository);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "configured": repository.is_some(),
            "repository": repository,
        })),
    )
        .into_response())
}

/// Files a saved report as a GitHub issue and records where it went.
///
/// SEPARATE FROM SAVING, deliberately. The report is written to this Hive
/// first and stays there whatever happens here, so a GitHub outage cannot lose
/// somebody's words — which is the failure the whole feature exists to end.
pub(super) async fn file_on_github(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(report_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let Some(github) = state.github_feedback.as_ref() else {
        // Says which. "Not configured" and "GitHub refused" send a reader to
        // completely different places, and collapsing them is how the original
        // defect stayed invisible for so long.
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "github_feedback_unconfigured",
            "this Hive has no GitHub credential, so reports stay on it",
        ));
    };
    let store = task_store(&state)?;
    let report = store
        .list_dogfood_reports(swarm_persistence::MAX_DOGFOOD_REPORTS)
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .find(|candidate| candidate.id == report_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "dogfood_report_not_found",
                "that report is not on this Hive",
            )
        })?;
    if let Some(existing) = report.github_issue_url.as_deref() {
        // Already filed. Returning it rather than opening a second issue: a
        // double tap must not scatter duplicates across the tracker.
        return Ok((
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "issue_url": existing, "created": false })),
        )
            .into_response());
    }
    let issue_url = github.file(&report).await.map_err(|error| match error {
        crate::github_feedback::GithubError::Refused(message) => {
            ApiError::new(StatusCode::BAD_GATEWAY, "github_refused", message)
        }
        crate::github_feedback::GithubError::Unreachable => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "github_unreachable",
            "GitHub could not be reached; the report is still saved on this Hive",
        ),
    })?;
    let recorded = store
        .record_dogfood_report_issue(&report_id, &issue_url)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "issue_url": recorded.github_issue_url, "created": true })),
    )
        .into_response())
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
