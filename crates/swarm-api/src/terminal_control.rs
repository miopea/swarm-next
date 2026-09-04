use std::{path::PathBuf, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_domain::ProviderConversationId;
use swarm_terminal::{
    ClaudeConversationStart, HostRequest, HostResponse, TerminalSize, TerminalWriteProvenance,
};

use crate::{
    ApiError, AppState, parse_session_id, require_valid_size, task_store_error,
    terminal_host::{authorized_no_store_request, authorized_request, record_session_event},
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
pub(super) struct WriteAuditQuery {
    limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub(super) struct InputRequest {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthoredSubmissionRequest {
    id: swarm_domain::OperatorSubmissionId,
    text: String,
}

pub(super) async fn record_submission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<AuthoredSubmissionRequest>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_operator_credential(&state, &headers)?;
    let created = crate::task_store(&state)?
        .record_operator_submission(
            request.id,
            parse_session_id(&session_id)?,
            &request.text,
            crate::unix_timestamp(),
        )
        .map_err(|error| {
            ApiError::new(
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "operator_submission_not_recorded",
                error.to_string(),
            )
        })?;
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({
        "id":request.id, "created":created, "source":"operator_authored", "provider_consumption":"unconfirmed"
    }))).into_response())
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
    let bytes = request.text.into_bytes();
    authorized_request(
        &state,
        &headers,
        HostRequest::Write {
            session_id: parse_session_id(&session_id)?,
            provenance: TerminalWriteProvenance::operator(None, &bytes),
            bytes,
        },
    )
    .await
}

pub(super) async fn write_audit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WriteAuditQuery>,
) -> Result<Response, ApiError> {
    authorized_no_store_request(
        &state,
        &headers,
        HostRequest::WriteAudit {
            limit: query.limit.unwrap_or(100),
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
    crate::authorize(&state, &headers)?;
    let _guard = state.worker_lifecycle.lock().await;
    let session_id = parse_session_id(&session_id)?;
    if let Some(store) = &state.task_store {
        store
            .cancel_session_revival(session_id)
            .map_err(|error| task_store_error(&error))?;
    }
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

/// What the geometry ledger says about one terminal over a window.
///
/// A diagnostic, not a control-room feature. It exists because two devices
/// trading a terminal's size was diagnosed three times from screenshots and
/// code reading, two of those diagnoses were wrong, and nothing recorded who
/// asked for which size or whether they were granted it.
#[derive(serde::Deserialize)]
pub(super) struct GeometryContentionQuery {
    /// How far back to look, in seconds. Defaults to five minutes.
    seconds: Option<i64>,
}

pub(super) async fn geometry_contention(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<GeometryContentionQuery>,
) -> Result<Response, ApiError> {
    crate::authorize(&state, &headers)?;
    let session_id = parse_session_id(&session_id)?;
    let since = crate::unix_timestamp() - query.seconds.unwrap_or(300).clamp(1, 86_400);
    let measured = crate::task_store(&state)?
        .geometry_contention(session_id, since)
        .map_err(|error| crate::task_store_error(&error))?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "requests": measured.requests,
            "devices": measured.devices,
            "handovers": measured.handovers,
            "refused": measured.refused,
            "distinct_sizes": measured.distinct_sizes,
        })),
    )
        .into_response())
}

#[cfg(test)]
mod submission_tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::post};
    use serde_json::{Value, json};
    use swarm_domain::{OperatorSubmissionId, WorkerSessionId};
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    #[tokio::test]
    async fn authored_submission_http_requires_operator_and_never_contacts_the_pty() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store.ensure_queen("/workspace").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_terminal_host(
                swarm_terminal::HostClient::new("/nonexistent-submission-test/socket"),
                "operator-test-secret",
            );
        let router = Router::new()
            .route(
                "/sessions/{session_id}/submissions",
                post(record_submission),
            )
            .with_state(Arc::new(state));
        let id = OperatorSubmissionId::new();
        let body = json!({"id":id,"text":"Keep the exact scope.\nNo release."});
        let request = |authorized: bool, body: &Value| {
            let mut request = Request::post(format!("/sessions/{session}/submissions"))
                .header("host", "localhost")
                .header("content-type", "application/json");
            if authorized {
                request = request.header("authorization", "Bearer operator-test-secret");
            }
            request.body(Body::from(body.to_string())).unwrap()
        };
        let denied = router.clone().oneshot(request(false, &body)).await.unwrap();
        assert_eq!(denied.status(), axum::http::StatusCode::UNAUTHORIZED);
        assert!(
            store
                .authored_operator_submission(id, crate::unix_timestamp())
                .unwrap()
                .is_none()
        );
        for created in [true, false] {
            let accepted = router.clone().oneshot(request(true, &body)).await.unwrap();
            assert_eq!(accepted.status(), axum::http::StatusCode::OK);
            assert_eq!(accepted.headers()["cache-control"], "no-store");
            let bytes = axum::body::to_bytes(accepted.into_body(), 4096)
                .await
                .unwrap();
            let response: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(response["created"], created);
            assert_eq!(response["provider_consumption"], "unconfirmed");
            assert!(response.get("text").is_none());
        }
        let conflict = router
            .clone()
            .oneshot(request(true, &json!({"id":id,"text":"Changed scope"})))
            .await
            .unwrap();
        assert_eq!(
            conflict.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let forged = router
            .oneshot(request(
                true,
                &json!({"id":id,"text":"Changed scope","operator_id":"forged"}),
            ))
            .await
            .unwrap();
        assert_eq!(
            forged.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let source = store
            .authored_operator_submission(id, crate::unix_timestamp())
            .unwrap()
            .unwrap();
        assert_eq!(source.text, "Keep the exact scope.\nNo release.");
    }
}
