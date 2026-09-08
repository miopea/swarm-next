//! Local operator interface. BFG Admin owns public intake and central conversations.
use crate::{ApiError, AppState, authorize};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use swarm_application::{HiveSupportService, HiveSupportServiceError, SupportDestination};
use swarm_domain::SupportSubmissionInput;
use tokio::sync::{Notify, watch};

#[derive(Clone)]
pub(super) struct SupportRuntime {
    service: HiveSupportService,
    wake: Arc<Notify>,
    // 0 configured, 1 running, 2 stopped, 3 failed. Never restart a live owner.
    state: Arc<AtomicU8>,
}

impl AppState {
    pub(super) fn support_sender_failed(&self) -> bool {
        self.central_support
            .as_ref()
            .is_some_and(|runtime| runtime.state.load(Ordering::SeqCst) == 3)
    }
    /// Configures explicit central feedback without sending old local reports.
    ///
    /// # Errors
    /// Refuses invalid origins and missing Hive persistence.
    pub fn with_central_support(mut self, origin: &str) -> Result<Self, String> {
        let destination = SupportDestination::parse(origin).map_err(|error| error.to_string())?;
        let store = self
            .task_store
            .clone()
            .ok_or("central support requires Hive storage")?;
        self.central_support = Some(SupportRuntime {
            service: HiveSupportService::new(store, destination),
            wake: Arc::new(Notify::new()),
            state: Arc::new(AtomicU8::new(0)),
        });
        Ok(self)
    }

    /// Runtime owner calls once and joins this future on graceful shutdown.
    pub async fn run_support_sender(&self, stop: watch::Receiver<bool>) {
        let Some(runtime) = &self.central_support else {
            return;
        };
        if runtime
            .state
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let _guard = SenderGuard(runtime.state.clone());
        let result =
            crate::support_sender::run(runtime.service.clone(), stop, runtime.wake.clone()).await;
        runtime
            .state
            .store(if result.is_ok() { 2 } else { 3 }, Ordering::SeqCst);
        if result.is_err() {
            tracing::warn!("central support sender stopped; saved reports retained");
        }
    }
}

struct SenderGuard(Arc<AtomicU8>);
impl Drop for SenderGuard {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(1, 3, Ordering::SeqCst, Ordering::SeqCst);
    }
}

pub(super) async fn status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = crate::task_store(&state)?.clone();
    let permit = admission(&state)?;
    let deliveries = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        store.support_submission_statuses()
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?;
    let sender =
        state
            .central_support
            .as_ref()
            .map(|runtime| match runtime.state.load(Ordering::SeqCst) {
                0 => "configured",
                1 => "running",
                2 => "stopped",
                _ => "failed",
            });
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({
        "configured": state.central_support.is_some(), "sender": sender, "deliveries": deliveries,
    }))).into_response())
}

pub(super) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<SupportSubmissionInput>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let runtime = state.central_support.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "support_not_configured",
            "Central support is not configured.",
        )
    })?;
    let permit = admission(&state)?;
    let service = runtime.service.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.submit_reviewed(input, crate::unix_timestamp())
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|error| service_error(&error))?;
    runtime.wake.notify_one();
    Ok((StatusCode::ACCEPTED, [(header::CACHE_CONTROL, "no-store")], Json(serde_json::json!({
        "submission_key": saved.submission_key, "created_at": saved.created_at, "delivery": saved.delivery,
    }))).into_response())
}

fn unavailable() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "support_unavailable",
        "Support storage is unavailable; keep the original report for retry.",
    )
}

pub(super) async fn forget(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<swarm_domain::ForgetSupportReport>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let store = crate::task_store(&state)?.clone();
    let permit = admission(&state)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        HiveSupportService::forget_local_copy(&store, &request)
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|error| service_error(&error))?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response())
}

pub(super) async fn retry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<swarm_domain::SupportRetryRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let runtime = state.central_support.as_ref().ok_or_else(unavailable)?;
    let permit = admission(&state)?;
    let service = runtime.service.clone();
    let delivery = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        service.retry_once(&request, crate::unix_timestamp())
    })
    .await
    .map_err(|_| unavailable())?
    .map_err(|error| service_error(&error))?;
    runtime.wake.notify_one();
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "delivery": delivery })),
    )
        .into_response())
}

fn admission(state: &AppState) -> Result<tokio::sync::OwnedSemaphorePermit, ApiError> {
    state
        .support_admission
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "support_busy",
                "Support submission is busy; retry the same report.",
            )
        })
}

fn service_error(error: &HiveSupportServiceError) -> ApiError {
    use swarm_persistence::SupportOutboxError;
    match error {
        HiveSupportServiceError::Outbox(
            SupportOutboxError::StaleAttempt | SupportOutboxError::InvalidTransition,
        ) => ApiError::new(
            StatusCode::CONFLICT,
            "support_retry_changed",
            "Delivery changed; refresh its status before requesting another retry.",
        ),
        HiveSupportServiceError::Outbox(SupportOutboxError::NotFound) => ApiError::new(
            StatusCode::NOT_FOUND,
            "support_not_found",
            "This saved support report could not be found.",
        ),
        HiveSupportServiceError::InvalidSubmission(_)
        | HiveSupportServiceError::UnsupportedKind => ApiError::new(
            StatusCode::BAD_REQUEST,
            "support_invalid",
            "Check the email, subject and report text.",
        ),
        HiveSupportServiceError::Outbox(SupportOutboxError::Conflict) => ApiError::new(
            StatusCode::CONFLICT,
            "support_conflict",
            "This report identity already has different saved content or destination.",
        ),
        HiveSupportServiceError::Outbox(SupportOutboxError::Capacity) => ApiError::new(
            StatusCode::CONFLICT,
            "support_capacity",
            "The support outbox is full; existing reports are retained.",
        ),
        _ => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::Request,
    };
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    fn fixture() -> (AppState, TaskStore) {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default()
            .with_task_store(store.clone())
            .with_central_support("https://support.example.invalid")
            .unwrap();
        *state.operator_token.write().unwrap() = Some(Arc::from("fictional-test-token"));
        (state, store)
    }

    fn payload() -> serde_json::Value {
        serde_json::json!({"submission_key":"00000000-0000-0000-0000-000000000007",
            "kind":"bug_report", "email":"fictional@example.invalid", "name":null,
            "subject":"Fictional support", "body":"Private fictional content"})
    }

    async fn request(
        app: Router,
        method: &str,
        authorized: bool,
        body: serde_json::Value,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri("/api/v1/feedback/support")
            .header("content-type", "application/json");
        if authorized {
            builder = builder.header("authorization", "Bearer fictional-test-token");
        }
        app.oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn operator_only_submission_is_saved_once_and_never_claimed_by_the_request() {
        let (state, store) = fixture();
        let app = crate::router(state);
        assert_eq!(
            request(app.clone(), "POST", false, payload())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(store.support_submission_statuses().unwrap().is_empty());
        for _ in 0..2 {
            let response = request(app.clone(), "POST", true, payload()).await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let text = String::from_utf8(body.to_vec()).unwrap();
            assert!(!text.contains("fictional@example.invalid"));
            assert!(!text.contains("Private fictional content"));
        }
        let rows = store.support_submission_statuses().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].delivery.attempts, 0);
        let mut changed = payload();
        changed["body"] = "Changed content".into();
        assert_eq!(
            request(app, "POST", true, changed).await.status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn disabled_destination_retains_status_and_rejects_new_submission() {
        let (mut state, _) = fixture();
        request(crate::router(state.clone()), "POST", true, payload()).await;
        state.central_support = None;
        let app = crate::router(state);
        let response = request(app.clone(), "GET", true, serde_json::Value::Null).await;
        assert_eq!(response.status(), StatusCode::OK);
        let status: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(status["configured"], false);
        assert_eq!(status["deliveries"].as_array().unwrap().len(), 1);
        assert_eq!(
            request(app, "POST", true, payload()).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn saturated_storage_admission_refuses_reads_and_writes_without_saving() {
        let (state, store) = fixture();
        let _held = state
            .support_admission
            .clone()
            .acquire_many_owned(4)
            .await
            .unwrap();
        let app = crate::router(state);
        for method in ["GET", "POST"] {
            assert_eq!(
                request(app.clone(), method, true, payload()).await.status(),
                StatusCode::TOO_MANY_REQUESTS
            );
        }
        assert!(store.support_submission_statuses().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_destination_fields_and_oversized_reports_never_enter_the_outbox() {
        let (state, store) = fixture();
        let app = crate::router(state);
        let mut extra = payload();
        extra["destination"] = "https://attacker.example.invalid".into();
        assert_eq!(
            request(app.clone(), "POST", true, extra).await.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut large = payload();
        large["body"] = "x".repeat(128 * 1024).into();
        assert_eq!(
            request(app, "POST", true, large).await.status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(store.support_submission_statuses().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_sender_is_visible_in_health_without_exposing_report_content() {
        let (state, _) = fixture();
        let runtime = state.central_support.as_ref().unwrap();
        runtime.state.store(1, Ordering::SeqCst);
        {
            let _guard = SenderGuard(runtime.state.clone());
        }
        assert!(state.support_sender_failed());
        let response = crate::health(State(Arc::new(state))).await;
        let body = serde_json::to_string(&response.0).unwrap();
        assert!(body.contains("Central support"));
        assert!(body.contains("degraded"));
        assert!(!body.contains("fictional@example.invalid"));
    }

    #[tokio::test]
    async fn explicit_retry_requires_operator_and_repeated_command_grants_only_once() {
        let (state, store) = fixture();
        let app = crate::router(state);
        request(app.clone(), "POST", true, payload()).await;
        let key = "00000000-0000-0000-0000-000000000007".parse().unwrap();
        let mut attempt = None;
        for _ in 0..5 {
            attempt = store
                .claim_support_submission(key, 1)
                .unwrap()
                .delivery
                .attempt_id;
            store
                .settle_support_submission(
                    key,
                    attempt.unwrap(),
                    swarm_domain::SupportDeliveryState::Uncertain,
                    None,
                    2,
                )
                .unwrap();
        }
        let command = serde_json::json!({"submission_key":key,
            "retry_id":"00000000-0000-0000-0000-000000000099", "expected_attempt_id":attempt});
        for authorized in [false, true, true] {
            let mut builder = Request::builder()
                .method("POST")
                .uri("/api/v1/feedback/support/retry")
                .header("content-type", "application/json");
            if authorized {
                builder = builder.header("authorization", "Bearer fictional-test-token");
            }
            let response = app
                .clone()
                .oneshot(builder.body(Body::from(command.to_string())).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if authorized {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
        let row = store.support_submission(key).unwrap();
        assert_eq!(row.delivery.attempts, 5);
        assert!(row.delivery.manual_retry_pending);
        let claimed = store.claim_support_submission(key, 3).unwrap();
        assert_eq!(claimed.delivery.attempts, 6);
        assert!(!claimed.delivery.manual_retry_pending);
    }

    #[tokio::test]
    async fn local_removal_requires_operator_and_confirmed_receipt_even_when_disabled() {
        let (mut state, store) = fixture();
        request(crate::router(state.clone()), "POST", true, payload()).await;
        let key = "00000000-0000-0000-0000-000000000007".parse().unwrap();
        let message = "00000000-0000-0000-0000-000000000008";
        let command = serde_json::json!({"submission_key":key,"expected_message_id":message});
        state.central_support = None;
        let app = crate::router(state);
        let remove = |authorized: bool| {
            let mut builder = Request::builder()
                .method("DELETE")
                .uri("/api/v1/feedback/support/local-copy")
                .header("content-type", "application/json");
            if authorized {
                builder = builder.header("authorization", "Bearer fictional-test-token");
            }
            app.clone()
                .oneshot(builder.body(Body::from(command.to_string())).unwrap())
        };
        assert_eq!(
            remove(false).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(remove(true).await.unwrap().status(), StatusCode::CONFLICT);
        let attempt = store
            .claim_support_submission(key, 1)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        store
            .settle_support_submission(
                key,
                attempt,
                swarm_domain::SupportDeliveryState::Confirmed,
                Some(swarm_persistence::SupportReceipt {
                    submission_key: key.to_string(),
                    conversation_id: "00000000-0000-0000-0000-000000000009".into(),
                    message_id: message.into(),
                    created_at: 2,
                    deduplicated: false,
                }),
                2,
            )
            .unwrap();
        assert_eq!(remove(true).await.unwrap().status(), StatusCode::NO_CONTENT);
        assert_eq!(remove(true).await.unwrap().status(), StatusCode::NO_CONTENT);
        assert!(store.support_submission_statuses().unwrap().is_empty());
    }
}
