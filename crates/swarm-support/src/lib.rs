//! Separate central support adapter. Deliberately has no Hive/terminal routes.
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use swarm_application::{SupportService, SupportServiceError};
use swarm_domain::SupportSubmissionInput;
use swarm_persistence::SupportStoreError;
use tokio::sync::Semaphore;

/// A separately provisioned Admin credential, never a public submitter identity.
#[derive(Clone)]
pub struct AdminCredential([u8; 32]);

impl AdminCredential {
    /// # Errors
    /// Refuses missing, short, unbounded or whitespace-bearing configuration.
    pub fn new(token: &str) -> Result<Self, &'static str> {
        if !(32..=4096).contains(&token.len())
            || token.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(
                "support Admin credential must be a bounded nonempty secret of at least 32 bytes",
            );
        }
        Ok(Self(Sha256::digest(token.as_bytes()).into()))
    }

    fn accepts(&self, headers: &axum::http::HeaderMap) -> bool {
        if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
            return false;
        }
        let Some(token) = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return false;
        };
        if token.len() > 4096 {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        bool::from(self.0.ct_eq(&digest))
    }
}

const MAX_BODY_BYTES: usize = 128 * 1024;
const MAX_REQUESTS: usize = 8;
const REQUEST_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct AppState {
    service: SupportService,
    admission: Arc<Semaphore>,
    admin: Option<AdminCredential>,
}

/// Builds an isolated public intake surface; registration and replies are not enabled.
pub fn router(service: SupportService) -> Router {
    let state = AppState {
        service,
        admission: Arc::new(Semaphore::new(MAX_REQUESTS)),
        admin: None,
    };
    router_with_state(state)
}

/// Enable only privileged conversation reads; sends remain unimplemented.
pub fn router_with_admin(service: SupportService, admin: AdminCredential) -> Router {
    router_with_state(AppState {
        service,
        admission: Arc::new(Semaphore::new(MAX_REQUESTS)),
        admin: Some(admin),
    })
}

fn router_with_state(state: AppState) -> Router {
    let mut routes = Router::new().route("/api/support/v1/submissions", post(submit));
    if state.admin.is_some() {
        routes = routes
            .route(
                "/api/ops/admin/conversations",
                axum::routing::get(conversations),
            )
            .route(
                "/api/ops/admin/conversations/{id}",
                axum::routing::get(conversation),
            );
    }
    routes
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            bounded_request,
        ))
        .with_state(state)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationQuery {
    cursor: Option<String>,
}

fn now_millis() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

async fn conversations(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<ConversationQuery>,
    axum::Extension(permit): axum::Extension<Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Response {
    if !state
        .admin
        .as_ref()
        .is_some_and(|admin| admin.accepts(&headers))
    {
        return failure(StatusCode::UNAUTHORIZED, "support_admin_required");
    }
    let Some(now) = now_millis() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "support_clock_unavailable");
    };
    finish_read(
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            state.service.conversations(query.cursor.as_deref(), now)
        })
        .await,
    )
}

async fn conversation(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<ConversationQuery>,
    axum::Extension(permit): axum::Extension<Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Response {
    if !state
        .admin
        .as_ref()
        .is_some_and(|admin| admin.accepts(&headers))
    {
        return failure(StatusCode::UNAUTHORIZED, "support_admin_required");
    }
    let Some(now) = now_millis() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "support_clock_unavailable");
    };
    finish_read(
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            state
                .service
                .conversation(&id, query.cursor.as_deref(), now)
        })
        .await,
    )
}

fn finish_read<T: serde::Serialize>(
    result: Result<Result<T, SupportServiceError>, tokio::task::JoinError>,
) -> Response {
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(SupportServiceError::StaleHistory)) => {
            failure(StatusCode::CONFLICT, "support_history_changed_refresh")
        }
        Ok(Err(SupportServiceError::Store(SupportStoreError::InvalidCursor))) => {
            failure(StatusCode::BAD_REQUEST, "invalid_support_cursor")
        }
        Ok(Err(SupportServiceError::Store(SupportStoreError::NotFound))) => {
            failure(StatusCode::NOT_FOUND, "support_conversation_not_found")
        }
        Ok(Err(SupportServiceError::Store(SupportStoreError::HistoryCapacity))) => {
            failure(StatusCode::PAYLOAD_TOO_LARGE, "support_history_limit")
        }
        _ => failure(StatusCode::SERVICE_UNAVAILABLE, "support_read_unavailable"),
    }
}

fn failure(status: StatusCode, code: &'static str) -> Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

async fn bounded_request(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mut response = match state.admission.clone().try_acquire_owned() {
        Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE, "support_busy"),
        Ok(permit) => {
            // Transfer the same admission permit into blocking persistence, where
            // it remains held even if the request times out after committing.
            let mut request = request;
            request.extensions_mut().insert(Arc::new(permit));
            match tokio::time::timeout(REQUEST_DEADLINE, next.run(request)).await {
                Ok(response) => response,
                Err(_) => failure(
                    StatusCode::GATEWAY_TIMEOUT,
                    "outcome_unknown_retry_same_submission",
                ),
            }
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn submit(
    State(state): State<AppState>,
    axum::Extension(permit): axum::Extension<Arc<tokio::sync::OwnedSemaphorePermit>>,
    input: Result<Json<SupportSubmissionInput>, JsonRejection>,
) -> Response {
    let input = match input {
        Ok(Json(input)) => input,
        Err(error) => return failure(error.status(), "invalid_support_submission"),
    };
    let Ok(now) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())
        .and_then(|duration| i64::try_from(duration.as_millis()).map_err(|_| ()))
    else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "support_clock_unavailable");
    };
    let outcome = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        state.service.submit(input, now)
    })
    .await;
    match outcome {
        Ok(Ok(receipt)) => (
            if receipt.deduplicated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            },
            Json(receipt),
        )
            .into_response(),
        Ok(Err(SupportServiceError::Invalid(_))) => failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_support_submission",
        ),
        Ok(Err(SupportServiceError::Store(SupportStoreError::Conflict))) => {
            failure(StatusCode::CONFLICT, "submission_conflict")
        }
        Ok(Err(SupportServiceError::Store(SupportStoreError::Capacity))) => {
            failure(StatusCode::SERVICE_UNAVAILABLE, "support_capacity_reached")
        }
        _ => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "support_unavailable_retry_same_submission",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use std::num::NonZeroU32;
    use swarm_persistence::SupportStore;
    use tower::ServiceExt;

    fn fixture() -> (tempfile::TempDir, Router) {
        let directory = tempfile::tempdir().unwrap();
        let store = SupportStore::open(
            &directory.path().join("support.db"),
            NonZeroU32::new(1).unwrap(),
        )
        .unwrap();
        (directory, router(SupportService::new(store)))
    }

    fn request(body: String) -> Request {
        Request::builder()
            .method("POST")
            .uri("/api/support/v1/submissions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn saturated_admission_refuses_without_consuming_durable_capacity() {
        let directory = tempfile::tempdir().unwrap();
        let service = SupportService::new(
            SupportStore::open(
                &directory.path().join("support.db"),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap(),
        );
        let admission = Arc::new(Semaphore::new(MAX_REQUESTS));
        let held = admission
            .clone()
            .acquire_many_owned(u32::try_from(MAX_REQUESTS).unwrap())
            .await
            .unwrap();
        let app = router_with_state(AppState {
            service,
            admission,
            admin: None,
        });
        let refused = app
            .clone()
            .oneshot(request(report().to_string()))
            .await
            .unwrap();
        assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(refused.headers()[header::CACHE_CONTROL], "no-store");
        drop(held);
        assert_eq!(
            app.oneshot(request(report().to_string()))
                .await
                .unwrap()
                .status(),
            StatusCode::CREATED
        );
    }

    #[tokio::test]
    async fn concurrent_duplicate_http_submissions_share_one_durable_message() {
        let (_directory, app) = fixture();
        let mut requests = tokio::task::JoinSet::new();
        for _ in 0..4 {
            let app = app.clone();
            requests.spawn(async move {
                let response = app.oneshot(request(report().to_string())).await.unwrap();
                let status = response.status();
                let body: serde_json::Value = serde_json::from_slice(
                    &to_bytes(response.into_body(), MAX_BODY_BYTES)
                        .await
                        .unwrap(),
                )
                .unwrap();
                (status, body)
            });
        }
        let mut ids = std::collections::HashSet::new();
        let mut created = 0;
        while let Some(result) = requests.join_next().await {
            let (status, receipt) = result.unwrap();
            assert!(matches!(status, StatusCode::OK | StatusCode::CREATED));
            created += usize::from(status == StatusCode::CREATED);
            ids.insert(receipt["message_id"].as_str().unwrap().to_owned());
        }
        assert_eq!(created, 1);
        assert_eq!(ids.len(), 1);
    }

    fn report() -> serde_json::Value {
        serde_json::json!({"submission_key":"00000000-0000-0000-0000-000000000001",
            "kind":"bug_report","email":"fictional@example.invalid","subject":"Reconnect","body":"Fictional stale output"})
    }

    #[tokio::test]
    async fn admin_reads_require_separate_credential_and_preserve_customer_content() {
        let directory = tempfile::tempdir().unwrap();
        let service = SupportService::new(
            SupportStore::open(
                &directory.path().join("support.db"),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap(),
        );
        let token = "fictional-admin-secret-at-least-32-characters";
        let app = router_with_admin(service, AdminCredential::new(token).unwrap());
        let mut input = report();
        input["name"] = "Fictional Bee".into();
        let response = app
            .clone()
            .oneshot(request(input.to_string()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let receipt: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        let path = format!(
            "/api/ops/admin/conversations/{}",
            receipt["conversation_id"].as_str().unwrap()
        );
        for route in ["/api/ops/admin/conversations", path.as_str()] {
            for credential in [
                None,
                Some("Bearer public-submitter"),
                Some("Bearer fictional-wrong-admin-secret-000000"),
            ] {
                let mut request = Request::builder().uri(route);
                if let Some(credential) = credential {
                    request = request.header(header::AUTHORIZATION, credential);
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            }
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(route)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body: serde_json::Value = serde_json::from_slice(
                &to_bytes(response.into_body(), MAX_BODY_BYTES)
                    .await
                    .unwrap(),
            )
            .unwrap();
            let summary = if route == path {
                &body["conversation"]
            } else {
                &body["conversations"][0]
            };
            assert_eq!(summary["customer"]["email"], input["email"]);
            assert_eq!(summary["customer"]["name"], input["name"]);
            assert!(summary["customer"]["accountId"].is_null());
            if route == path {
                assert_eq!(body["messages"][0]["body"], input["body"]);
                assert_eq!(body["messages"][0]["role"], "customer");
                assert_eq!(body["aiDraftingSupported"], false);
                assert!(body["replyUnavailableReason"].as_str().is_some());
                assert_eq!(summary["revision"].as_str().unwrap().len(), 64);
            }
        }
    }

    #[test]
    fn admin_credential_rejects_ambiguous_headers_and_invalid_configuration() {
        assert!(AdminCredential::new("short").is_err());
        assert!(AdminCredential::new(&" ".repeat(32)).is_err());
        let token = "fictional-admin-secret-at-least-32-characters";
        let admin = AdminCredential::new(token).unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(admin.accepts(&headers));
        headers.append(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(!admin.accepts(&headers));
        headers.remove(header::AUTHORIZATION);
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        let rotated =
            AdminCredential::new("fictional-replacement-secret-at-least-32-characters").unwrap();
        assert!(!rotated.accepts(&headers));
    }

    #[tokio::test]
    async fn private_read_errors_do_not_disguise_stale_history_or_enable_sends() {
        let service = SupportService::new(
            SupportStore::open(
                std::path::Path::new(":memory:"),
                NonZeroU32::new(1).unwrap(),
            )
            .unwrap(),
        );
        let token = "fictional-admin-secret-at-least-32-characters";
        let app = router_with_admin(service, AdminCredential::new(token).unwrap());
        let response = app
            .clone()
            .oneshot(request(report().to_string()))
            .await
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        let id = receipt["conversation_id"].as_str().unwrap();
        for (path, expected) in [
            (
                "/api/ops/admin/conversations?cursor=invalid".to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                format!(
                    "/api/ops/admin/conversations/{id}?cursor=t1.{id}.{}.0",
                    "0".repeat(64)
                ),
                StatusCode::CONFLICT,
            ),
            (
                format!("/api/ops/admin/conversations/{id}?cursor=invalid"),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/api/ops/admin/conversations/00000000-0000-0000-0000-000000000099".to_owned(),
                StatusCode::NOT_FOUND,
            ),
            (
                format!("/api/ops/admin/conversations/{id}/deliveries"),
                StatusCode::NOT_FOUND,
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/ops/admin/conversations/{id}/replies"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn email_only_intake_and_retry_return_same_receipt_without_private_content() {
        let (_directory, app) = fixture();
        let first = app
            .clone()
            .oneshot(request(report().to_string()))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);
        assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
        let first: serde_json::Value =
            serde_json::from_slice(&to_bytes(first.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert!(first.get("email").is_none());
        assert!(first.get("body").is_none());
        let replay = app
            .clone()
            .oneshot(request(report().to_string()))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        let replay: serde_json::Value =
            serde_json::from_slice(&to_bytes(replay.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(first["conversation_id"], replay["conversation_id"]);
        assert_eq!(replay["deduplicated"], true);
        let mut changed = report();
        changed["body"] = "Different".into();
        assert_eq!(
            app.oneshot(request(changed.to_string()))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn invalid_or_oversized_intake_does_not_consume_capacity() {
        let (_directory, app) = fixture();
        let mut invalid = report();
        invalid["approved"] = true.into();
        assert_eq!(
            app.clone()
                .oneshot(request(invalid.to_string()))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            app.clone()
                .oneshot(request("x".repeat(MAX_BODY_BYTES + 1)))
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            app.oneshot(request(report().to_string()))
                .await
                .unwrap()
                .status(),
            StatusCode::CREATED
        );
    }

    #[tokio::test]
    async fn public_intake_does_not_expose_hive_or_conversation_reads() {
        let (_directory, app) = fixture();
        for path in [
            "/api/v1/workers",
            "/api/v1/tasks",
            "/api/ops/admin/conversations",
            "/api/support/v1/submissions",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ));
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
}
