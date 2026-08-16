use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use swarm_domain::ControlRoomEventKind;
use swarm_terminal::{HostRequest, HostResponse, TerminalHostStatus};
use tokio::time::{sleep, timeout};

use crate::{
    ApiError, AppState, authorize, build_version, runtime, task_store, task_store_error,
    terminal_host::request_host, unix_timestamp, worker_engine_build_id,
};

#[derive(Debug, Serialize)]
pub(super) struct WorkerEngineMaintenanceResponse {
    previous_version: String,
    current_version: String,
    stopped_sessions: usize,
    restarted_workers: usize,
}

pub(super) async fn maintain_worker_engine(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let guard = state.worker_lifecycle.lock().await;
    let result = maintain_worker_engine_locked(&state).await;
    if let Ok(maintenance) = &result
        && maintenance.previous_version != maintenance.current_version
    {
        if let Err(error) = task_store(&state).and_then(|store| {
            store
                .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
                .map(|_| ())
                .map_err(|error| task_store_error(&error))
        }) {
            tracing::warn!(message = %error.message, "worker-engine update could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    drop(guard);

    // This runs on both success and failure. A failed package trigger therefore
    // revives autostart workers on the still-current host instead of leaving a
    // partially stopped Hive behind.
    state.supervise_workers().await;
    let mut response = result?;
    response.restarted_workers = task_store(&state)?
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .filter(|worker| worker.active_session_id.is_some())
        .count();
    Ok(Json(response).into_response())
}

pub(super) async fn request_development_reload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _guard = state.development_reload.lock().await;
    let source = runtime::development_source_status(&state);
    if source.as_ref().is_some_and(|status| !status.aligned) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "development_source_mismatch",
            "the configured development checkout does not contain the deployed source",
        ));
    }
    if source
        .as_ref()
        .is_some_and(|status| !status.reload_available)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "development_reload_not_needed",
            "the configured development checkout has no product changes to reload",
        ));
    }
    let source_revision = source.map_or_else(|| "unknown".into(), |source| source.revision);
    if matches!(
        runtime::development_reload_state_for_source(&state, Some(&source_revision)),
        "requested" | "building"
    ) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "development_reload_in_progress",
            "a development build is already in progress",
        ));
    }
    let request_path = state
        .development_reload_request_path
        .as_ref()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "development_reload_unavailable",
                "this installation is not connected to a development checkout",
            )
        })?;
    let status_path = state
        .development_reload_status_path
        .as_ref()
        .expect("development reload paths are configured together");
    std::fs::write(
        status_path.as_ref(),
        format!("state=requested\nrevision={source_revision}\n"),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "development_reload_unavailable",
            format!("the development reload status could not be recorded: {error}"),
        )
    })?;
    std::fs::write(
        request_path.as_ref(),
        format!(
            "requested_at={}\nsource_version={}\n",
            unix_timestamp(),
            build_version()
        ),
    )
    .map_err(|error| {
        let _ = std::fs::write(
            status_path.as_ref(),
            format!("state=failed\nrevision={source_revision}\n"),
        );
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "development_reload_unavailable",
            format!("the development reload request could not be recorded: {error}"),
        )
    })?;
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn maintain_worker_engine_locked(
    state: &AppState,
) -> Result<WorkerEngineMaintenanceResponse, ApiError> {
    let request_path = state.maintenance_request_path.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_engine_maintenance_unavailable",
            "this installation does not expose managed worker-engine maintenance",
        )
    })?;
    let previous = host_status_snapshot(state).await?;
    if !worker_engine_update_required(&previous) {
        return Ok(WorkerEngineMaintenanceResponse {
            previous_version: previous.host_version.clone(),
            current_version: previous.host_version,
            stopped_sessions: 0,
            restarted_workers: 0,
        });
    }
    let HostResponse::Sessions { sessions } =
        request_host(state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected session response",
        ));
    };
    let running = sessions
        .into_iter()
        .filter(|session| session.running)
        .collect::<Vec<_>>();
    for session in &running {
        request_host(
            state,
            HostRequest::Stop {
                session_id: session.session_id,
            },
        )
        .await?;
        task_store(state)?
            .release_worker_session(session.session_id)
            .map_err(|error| task_store_error(&error))?;
        task_store(state)?
            .release_session_assignments(session.session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    state.control_room_notify.notify_waiters();
    std::fs::write(
        request_path.as_ref(),
        format!(
            "requested_at={}\ntarget_version={}\n",
            unix_timestamp(),
            build_version()
        ),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_engine_maintenance_unavailable",
            format!("the managed maintenance request could not be recorded: {error}"),
        )
    })?;

    let updated = timeout(state.maintenance_timeout, async {
        loop {
            sleep(Duration::from_millis(200)).await;
            if let Ok(status) = host_status_snapshot(state).await
                && !worker_engine_update_required(&status)
                && !status.draining
            {
                return status;
            }
        }
    })
    .await;
    let _ = std::fs::remove_file(request_path.as_ref());
    let current = updated.map_err(|_| {
        ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "worker_engine_maintenance_timed_out",
            "the worker engine did not report the expected release; configured workers were revived on the available host",
        )
    })?;
    Ok(WorkerEngineMaintenanceResponse {
        previous_version: previous.host_version,
        current_version: current.host_version,
        stopped_sessions: running.len(),
        restarted_workers: 0,
    })
}

fn worker_engine_update_required(status: &TerminalHostStatus) -> bool {
    status.host_build_id.as_deref().map_or_else(
        || status.host_version != build_version(),
        |host_build_id| host_build_id != worker_engine_build_id(),
    )
}

async fn host_status_snapshot(state: &AppState) -> Result<TerminalHostStatus, ApiError> {
    let HostResponse::HostStatus { status } = request_host(state, HostRequest::HostStatus).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected status response",
        ));
    };
    Ok(status)
}
