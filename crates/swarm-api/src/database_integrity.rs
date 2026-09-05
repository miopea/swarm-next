use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{ApiError, AppState};

pub(super) async fn check(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    crate::auth::authorize_operator_credential(&state, &headers)?;
    let verified = probe(&state).await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({"verified": verified})),
    )
        .into_response())
}

async fn probe(state: &AppState) -> Result<bool, ApiError> {
    let permit = state
        .database_probe_limit
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "database_probe_busy",
                "An integrity probe is already running",
            )
        })?;
    let store = crate::task_store(state)?.clone();
    tokio::task::spawn_blocking(move || {
        // Cancellation of the requester must not admit a competing detached probe.
        let _permit = permit;
        store.probe_database_integrity()
    })
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_probe_unavailable",
            "The integrity probe could not complete",
        )
    })?
    .map_err(|error| crate::task_store_error(&error))
}

/// One API-owned, non-overlapping probe. The binary signals and joins it during
/// shutdown; dropping the signal sender also closes this owner.
pub async fn monitor_database_integrity(
    state: AppState,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    let Some(store) = state.task_store.clone() else {
        return;
    };
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await; // Opening already checked; no duplicate startup scan.
    loop {
        tokio::select! {
            biased;
            _ = &mut stop => break,
            _ = interval.tick() => {}
        }
        let started = Instant::now();
        let result = probe(&state).await;
        if store.database_recovery_required() {
            tracing::error!(
                "Hive database integrity failed; new database-backed work is paused pending verified offline recovery"
            );
            break;
        }
        match result {
            Ok(true) => tracing::info!(
                elapsed_ms = started.elapsed().as_millis(),
                "Hive database integrity verified"
            ),
            Ok(false) => {
                tracing::debug!("Hive database integrity probe deferred while persistence is busy");
            }
            Err(_) => {
                tracing::warn!(
                    "Hive database integrity probe incomplete; corruption is not confirmed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_persistence::TaskStore;
    use tower::ServiceExt;

    #[tokio::test]
    async fn manual_probe_is_authenticated_bounded_and_does_not_contact_a_terminal_host() {
        let state = Arc::new(
            AppState::default()
                .with_task_store(TaskStore::in_memory().unwrap())
                .with_terminal_host(
                    swarm_terminal::HostClient::new("/no-probe-fixture-host.sock"),
                    "probe-fixture-token",
                ),
        );
        let app = axum::Router::new()
            .route("/check", axum::routing::post(check))
            .with_state(state.clone());
        let request = |authorized| {
            let mut request = axum::http::Request::builder().method("POST").uri("/check");
            if authorized {
                request = request.header(header::AUTHORIZATION, "Bearer probe-fixture-token");
            }
            request.body(axum::body::Body::empty()).unwrap()
        };
        assert_eq!(
            app.clone().oneshot(request(false)).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let held = state
            .database_probe_limit
            .clone()
            .try_acquire_owned()
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request(true)).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        drop(held);
        let response = app.oneshot(request(true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["verified"],
            true
        );
    }

    #[tokio::test]
    async fn shutdown_and_sender_loss_close_the_monitor_without_startup_probe() {
        for explicit in [true, false] {
            let store = TaskStore::in_memory().unwrap();
            let state = AppState::default().with_task_store(store.clone());
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let monitor = tokio::spawn(monitor_database_integrity(state, receiver));
            if explicit {
                let _ = sender.send(());
            } else {
                drop(sender);
            }
            tokio::time::timeout(Duration::from_secs(1), monitor)
                .await
                .unwrap()
                .unwrap();
            assert!(!store.database_recovery_required());
            store.create_task("Still writable", "/fixture").unwrap();
        }
    }
}
