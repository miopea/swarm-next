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
    terminal_host::request_host, unix_timestamp, worker_engine_build_id, worker_runtime,
};
use swarm_terminal::TerminalSize;

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

#[derive(Debug, Serialize)]
pub(super) struct RestartWorkersResponse {
    restarted_workers: usize,
}

/// Restarts the workers still running a superseded provider release.
///
/// Claude and Codex update themselves and a running process keeps executing the
/// release it started with, so an update installed while workers are up is not
/// running anywhere until each one restarts. This is the same stop-and-revive
/// the worker engine update performs, without replacing anything: the roster is
/// written down before a worker is stopped, so an interruption still gets the
/// workers back.
pub(super) async fn restart_superseded_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let guard = state.worker_lifecycle.lock().await;
    let HostResponse::ProviderCapabilities {
        claude_release,
        codex_release,
        ..
    } = request_host(&state, HostRequest::ProviderCapabilities).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected provider response",
        ));
    };
    let store = task_store(&state)?;
    let sessions = store
        .active_worker_sessions()
        .map_err(|error| task_store_error(&error))?;
    let superseded = sessions
        .into_iter()
        .filter(|session| {
            let release = match session.provider {
                swarm_domain::ProviderKind::ClaudeCode => claude_release.as_ref(),
                swarm_domain::ProviderKind::Codex => codex_release.as_ref(),
            };
            swarm_terminal::provider_release_superseded(release, session.started_at)
        })
        .map(|session| session.worker_id)
        .collect::<Vec<_>>();
    if superseded.is_empty() {
        return Ok(Json(RestartWorkersResponse {
            restarted_workers: 0,
        })
        .into_response());
    }
    store
        .record_worker_revival_intents(&superseded, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    for worker_id in &superseded {
        let Ok(profile) = store.get_worker_profile(*worker_id) else {
            continue;
        };
        let Some(session_id) = profile.active_session_id else {
            continue;
        };
        request_host(&state, HostRequest::Stop { session_id }).await?;
        store
            .release_worker_session(session_id)
            .map_err(|error| task_store_error(&error))?;
        store
            .release_session_assignments(session_id)
            .map_err(|error| task_store_error(&error))?;
    }
    state.control_room_notify.notify_waiters();
    // Released before reviving: starting a worker takes this same mutex, and
    // it is not reentrant.
    drop(guard);
    let restarted_workers = revive_loaded_workers(&state, &superseded).await;
    state.control_room_notify.notify_waiters();
    Ok(Json(RestartWorkersResponse { restarted_workers }).into_response())
}

pub(super) async fn request_development_reload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    // The control room's own button: the operator pressed it themselves.
    start_development_reload(&state, None).await?;
    Ok(StatusCode::ACCEPTED.into_response())
}

/// What was asked for, so a caller can tell whether the build that comes back
/// is the one it asked for.
pub(crate) struct StartedDevelopmentReload {
    pub(crate) source_revision: String,
    pub(crate) previous_version: String,
}

/// Asks the development reload service to rebuild and swap this Hive.
///
/// Shared by the control room's button and the agent tool, so a guard added to
/// one cannot go missing from the other.
pub(crate) async fn start_development_reload(
    state: &Arc<AppState>,
    requested_by: Option<&str>,
) -> Result<StartedDevelopmentReload, ApiError> {
    let _guard = state.development_reload.lock().await;
    let source = runtime::development_source_status(state);
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
        runtime::development_reload_state_for_source(state, Some(&source_revision)),
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
        // Who asked is written down. A reload the operator did not press must
        // be visible to them afterwards rather than discovered by the surface
        // changing under them — that is the condition on which the guard below
        // it was relaxed.
        format!(
            "state=requested\nrevision={source_revision}\nrequested_by={}\n",
            requested_by.unwrap_or("operator")
        ),
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
    Ok(StartedDevelopmentReload {
        source_revision,
        previous_version: build_version().to_owned(),
    })
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
    // Replacing the engine unloads every worker. Remember which ones the
    // operator had loaded so they can be brought back afterwards: a warned
    // maintenance action should cost a restart, not a roster the operator has
    // to wake one worker at a time.
    let loaded_worker_ids = loaded_workers(
        &task_store(state)?
            .list_worker_profiles()
            .map_err(|error| task_store_error(&error))?,
        &running
            .iter()
            .map(|session| session.session_id)
            .collect::<std::collections::HashSet<_>>(),
    );
    // Written down before anything is stopped, and fails the whole operation
    // if it cannot be: the card promises these workers back, and stopping them
    // with no durable record of who they were is how that promise was broken.
    task_store(state)?
        .record_worker_revival_intents(&loaded_worker_ids, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    stop_running_sessions(state, &running).await?;
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
            "the worker engine has not yet reported the expected release. The workers it unloaded are still recorded as owed a return and will be started once the engine reports in; check the roster in a moment.",
        )
    })?;
    // Not revived here. This runs under the worker lifecycle, and starting a
    // worker takes that same non-reentrant mutex, so reviving inside it would
    // deadlock the API against itself. The caller revives after releasing it,
    // and anything still owed is picked up by the supervisor.
    state.control_room_notify.notify_waiters();
    Ok(WorkerEngineMaintenanceResponse {
        previous_version: previous.host_version,
        current_version: current.host_version,
        stopped_sessions: running.len(),
        restarted_workers: 0,
    })
}

/// Stops every session the engine replacement is about to invalidate, and lets
/// go of the work each one owned.
async fn stop_running_sessions(
    state: &AppState,
    running: &[swarm_terminal::HostSessionSummary],
) -> Result<(), ApiError> {
    for session in running {
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
    Ok(())
}

/// The workers holding the sessions about to be stopped.
///
/// Matched by the exact session each profile currently holds, so a worker that
/// was already asleep is not woken by maintenance it was not part of, and a
/// session with no profile behind it revives nothing.
fn loaded_workers(
    profiles: &[swarm_domain::WorkerProfile],
    running_sessions: &std::collections::HashSet<swarm_domain::WorkerSessionId>,
) -> Vec<swarm_domain::WorkerId> {
    profiles
        .iter()
        .filter(|profile| {
            profile
                .active_session_id
                .is_some_and(|session_id| running_sessions.contains(&session_id))
        })
        .map(|profile| profile.id)
        .collect()
}

/// Brings back the workers a worker-engine replacement unloaded, and reports how
/// many returned.
///
/// One worker failing to start does not abandon the rest: the failure is
/// recorded against that worker, where the roster already shows it, and the
/// remaining workers are still revived.
async fn revive_loaded_workers(state: &AppState, worker_ids: &[swarm_domain::WorkerId]) -> usize {
    let mut restarted = 0;
    for worker_id in worker_ids {
        let already_running = task_store(state).ok().and_then(|store| {
            store
                .get_worker_profile(*worker_id)
                .ok()
                .map(|profile| profile.active_session_id.is_some())
        });
        if already_running == Some(true) {
            restarted += 1;
            if let Ok(store) = task_store(state) {
                let _ = store.clear_worker_revival_intent(*worker_id);
            }
            continue;
        }
        match worker_runtime::start_worker_process(state, *worker_id, TerminalSize::default()).await
        {
            Ok(_) => {
                restarted += 1;
                if let Ok(store) = task_store(state) {
                    let _ = store.clear_worker_revival_intent(*worker_id);
                }
            }
            Err(error) => {
                state
                    .worker_errors
                    .write()
                    .await
                    .insert(*worker_id, error.message.clone());
                tracing::warn!(worker_id = %worker_id, message = %error.message, "worker could not be revived after the worker engine was replaced");
            }
        }
    }
    restarted
}

pub(crate) fn worker_engine_update_required(status: &TerminalHostStatus) -> bool {
    status.host_build_id.as_deref().map_or_else(
        || status.host_version != build_version(),
        |host_build_id| host_build_id != worker_engine_build_id(),
    )
}

pub(crate) async fn host_status_snapshot(state: &AppState) -> Result<TerminalHostStatus, ApiError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, WorkerId, WorkerProfile, WorkerRole, WorkerSessionId};

    fn profile(active_session_id: Option<WorkerSessionId>) -> WorkerProfile {
        WorkerProfile {
            id: WorkerId::new(),
            hive_id: swarm_domain::HiveId::new(),
            name: "Worker".into(),
            description: String::new(),
            role: WorkerRole::Worker,
            provider: ProviderKind::ClaudeCode,
            workspace: "/repo".into(),
            autostart: false,
            position: 0,
            active_session_id,
            provider_conversation_id: None,
            has_session_history: false,
            engagement_expires_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn revives_only_the_workers_that_were_loaded() {
        let loaded_session = WorkerSessionId::new();
        let loaded = profile(Some(loaded_session));
        let sleeping = profile(None);
        let elsewhere = profile(Some(WorkerSessionId::new()));
        let running = std::iter::once(loaded_session).collect();

        let revive = loaded_workers(&[loaded.clone(), sleeping, elsewhere], &running);

        assert_eq!(revive, vec![loaded.id]);
    }

    #[test]
    fn revives_nothing_when_the_engine_held_no_workers() {
        let running = std::collections::HashSet::new();

        assert!(loaded_workers(&[profile(Some(WorkerSessionId::new()))], &running).is_empty());
    }
}
