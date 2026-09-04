use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use swarm_domain::BrowserEvidenceHour;
use swarm_persistence::EvidenceError;

use crate::{ApiError, AppState, authorize, task_store, unix_timestamp};

#[derive(Deserialize)]
pub(super) struct EvidenceQuery {
    limit: Option<u32>,
}

fn authorize_development(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    authorize(state, headers)?;
    if state.development_reload_request_path.is_none() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "dogfood_disabled",
            "Developer Dogfood is available only on a development Hive",
        ));
    }
    Ok(())
}

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EvidenceQuery>,
) -> Result<Response, ApiError> {
    authorize_development(&state, &headers)?;
    let evidence = task_store(&state)?
        .browser_evidence(unix_timestamp(), query.limit.unwrap_or(100))
        .map_err(|error| evidence_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(evidence)).into_response())
}

pub(super) async fn record(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(evidence): Json<BrowserEvidenceHour>,
) -> Result<Response, ApiError> {
    authorize_development(&state, &headers)?;
    let outcome = task_store(&state)?
        .record_browser_evidence(&evidence, unix_timestamp())
        .map_err(|error| evidence_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "updated": outcome.updated, "pruned": outcome.pruned,
        })),
    )
        .into_response())
}

fn evidence_error(error: &EvidenceError) -> ApiError {
    match error {
        EvidenceError::Invalid => ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_browser_evidence",
            "Browser evidence is invalid or outside the upload window",
        ),
        EvidenceError::Conflict => ApiError::new(
            StatusCode::CONFLICT,
            "browser_evidence_conflict",
            "This capture conflicts with previously recorded evidence",
        ),
        EvidenceError::Corrupt | EvidenceError::Store(_) | EvidenceError::Sql(_) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "browser_evidence_unavailable",
            "Browser evidence could not be read or saved",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostClient, router};
    use axum::{body::Body, http::Request};
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    fn app(development: bool) -> axum::Router {
        let mut state = AppState::default()
            .with_terminal_host(HostClient::new("/unreachable/terminal.sock"), "secret")
            .with_task_store(TaskStore::in_memory().unwrap());
        if development {
            state = state
                .with_development_reload_paths("/unused/request".into(), "/unused/status".into());
        }
        router(state)
    }

    fn request(method: &str, authorized: bool, payload: String) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri("/api/v1/diagnostics/browser-evidence")
            .header("content-type", "application/json");
        if authorized {
            builder = builder.header("authorization", "Bearer secret");
        }
        builder.body(Body::from(payload)).unwrap()
    }

    fn payload() -> String {
        let timing = serde_json::json!({"count":0,"total_ms":0,"max_ms":0});
        serde_json::json!({"capture_id":"00000000-0000-0000-0000-000000000001",
            "build":"1.4.1-dev-abc", "hour":unix_timestamp()/3600*3600, "revision":1,
            "long_task":timing,"interaction":timing,"route":timing,
            "terminal_render":timing,"terminal_reconnect":timing})
        .to_string()
    }

    #[tokio::test]
    async fn access_requires_authentication_and_existing_development_mode() {
        for method in ["GET", "POST"] {
            assert_eq!(
                app(true)
                    .oneshot(request(method, false, payload()))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                app(false)
                    .oneshot(request(method, true, payload()))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }

    #[tokio::test]
    async fn evidence_is_private_bounded_and_retry_safe() {
        let app = app(true);
        for updated in [true, false] {
            let response = app
                .clone()
                .oneshot(request("POST", true, payload()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["updated"], updated);
        }
        let response = app
            .clone()
            .oneshot(request("POST", true, " ".repeat(4097)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let response = app
            .oneshot(request("GET", true, String::new()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
