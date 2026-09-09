use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{ApiError, AppState, authorize, task_store, unix_timestamp};

#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    limit: Option<u32>,
}

pub(super) async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let history = task_store(&state)?
        .queen_run_history(unix_timestamp(), query.limit.unwrap_or(100))
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "queen_history_unavailable",
                "Queen run history is unavailable; no zero-activity result was inferred",
            )
        })?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(history)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostClient, router};
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    fn app(store: TaskStore) -> axum::Router {
        router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/socket"), "secret")
                .with_task_store(store),
        )
    }

    fn request(authorized: bool) -> Request<Body> {
        let mut builder = Request::builder().uri("/api/v1/runtime/queen-history?limit=100");
        if authorized {
            builder = builder.header("authorization", "Bearer secret");
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn finished_history_is_private_and_content_free_on_ordinary_hives() {
        let store = TaskStore::in_memory().unwrap();
        let now = unix_timestamp();
        let queen = store
            .ensure_queen("/private/never-export-this-path")
            .unwrap();
        store
            .bind_worker_session(queen.id, swarm_domain::WorkerSessionId::new())
            .unwrap();
        store.set_queen_automation_enabled(true, now - 10).unwrap();
        store.request_queen_automation_run(now - 9).unwrap();
        let run = store.claim_queen_automation(now - 8).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, now - 7)
            .unwrap();
        store
            .finish_queen_automation_run_with_recovery(
                &run.run_id,
                swarm_domain::QueenAutomationOutcome::NoAction,
                now,
                &[],
                true,
                Some("1.6.0-dev-fixture"),
            )
            .unwrap();
        let app = app(store);
        assert_eq!(
            app.clone().oneshot(request(false)).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let response = app.oneshot(request(true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["retained_count"], 1);
        assert_eq!(payload["records"][0]["run_id"], run.run_id);
        assert_eq!(
            payload["records"][0]["finished_on_build"],
            "1.6.0-dev-fixture"
        );
        assert!(
            !String::from_utf8(bytes.to_vec())
                .unwrap()
                .contains("never-export-this-path")
        );
    }

    #[tokio::test]
    async fn missing_store_is_an_error_not_empty_history() {
        let app = router(
            AppState::default()
                .with_terminal_host(HostClient::new("/unreachable/socket"), "secret"),
        );
        let response = app.oneshot(request(true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert!(payload.get("records").is_none());
    }

    #[tokio::test]
    async fn empty_history_is_explicitly_bounded_not_invented_past_evidence() {
        let response = app(TaskStore::in_memory().unwrap())
            .oneshot(request(true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(payload["records"], serde_json::json!([]));
        assert_eq!(payload["retained_count"], 0);
        assert_eq!(payload["retention_days"], 30);
        assert_eq!(payload["max_retained"], 4096);
    }
}
