use std::{path::PathBuf, sync::Arc, time::Duration};

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
    ApiError, AppState, authorize, build_version, reload_backup, runtime, task_store,
    task_store_error, terminal_host::request_host, unix_timestamp, worker_engine_build_id,
    worker_runtime,
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

/// Package-owned replacement records its return set while the engine is drained.
/// This endpoint grants no authority to stop work and never stops a session.
pub(super) async fn prepare_worker_engine_return(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let _guard = state.worker_lifecycle.lock().await;
    if !host_status_snapshot(&state).await?.draining {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "worker_engine_not_draining",
            "drain the worker engine before recording its return set",
        ));
    }
    let HostResponse::Sessions { sessions } =
        request_host(&state, HostRequest::ListSessions).await?
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
        .map(|session| session.session_id)
        .collect();
    let store = task_store(&state)?;
    let profiles = store
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    let workers = loaded_workers(&profiles, &running);
    store
        .record_worker_revival_intents(&workers, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "recorded_workers": workers.len() })),
    )
        .into_response())
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
                // No installed release to compare a session against, so it can
                // never be superseded by one. It cannot be running either.
                //
                // The alpha three are here for a different reason: nothing
                // probes their releases, so there is no version to compare and
                // an alpha worker is never reported as running a superseded
                // build. That is a missing feature stated honestly rather than
                // a comparison against a value invented here.
                swarm_domain::ProviderKind::Gemini
                | swarm_domain::ProviderKind::Grok
                | swarm_domain::ProviderKind::OpenCode
                | swarm_domain::ProviderKind::Unsupported => None,
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

/// How many live sessions can actually call what this build serves.
///
/// THREE ANSWERS, NOT TWO, and the third is the one that was missing. A session
/// caches its tool list when its MCP client connects and never asks again, so a
/// changed tool surface reaches nobody until the session reconnects. The count
/// that existed only compared RECORDED revisions — and the record is in memory,
/// so an API restart empties it and every surviving session became invisible
/// rather than unknown.
///
/// Measured 2026-09-02: the API restarted at 11:19 serving revision 11, 13
/// sessions started at 02:18 were still live, and the stale count read zero.
/// None of those sessions could pass `delivers_whole_task`, the field that had
/// just shipped to stop a partial delivery closing a whole ticket.
///
/// The unconfirmed count is therefore reported separately and never folded into
/// the current one.
/// The operator's requirement was exactly this: "We need a way to notify if we
/// don't know."
pub(super) async fn tool_surface_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let sessions = task_store(&state)?
        .active_worker_sessions()
        .map_err(|error| task_store_error(&error))?;
    let store = task_store(&state)?;
    let recorded = state.agent_tool_surfaces.read().await;
    let serving = crate::agent::AGENT_TOOL_SURFACE_REVISION;
    let (mut matching, mut behind, mut unconfirmed) = (0_usize, 0_usize, 0_usize);
    for session in &sessions {
        // Keyed the way the agent surface records it: by SESSION, falling back
        // to the worker id when a session is not yet bound.
        let key = store
            .get_worker_profile(session.worker_id)
            .ok()
            .and_then(|profile| profile.active_session_id)
            .map_or_else(
                || session.worker_id.to_string(),
                |session_id| session_id.to_string(),
            );
        match classify_tool_surface(recorded.get(&key).copied(), serving) {
            SessionToolSurface::Current => matching += 1,
            SessionToolSurface::Stale => behind += 1,
            // Never asked this build for its tools, so what it holds is not
            // knowable from here. Counted as its own thing.
            SessionToolSurface::Unknown => unconfirmed += 1,
        }
    }
    Ok(Json(serde_json::json!({
        "serving_revision": serving,
        "live_sessions": sessions.len(),
        "current": matching,
        "stale": behind,
        "unknown": unconfirmed,
    }))
    .into_response())
}

/// What one live session's cached tool list is, relative to what is served.
///
/// Three answers, and the third is the one that was missing. `None` recorded
/// means this build has never served that session its tools, which is NOT the
/// same as serving it the current ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionToolSurface {
    Current,
    Stale,
    Unknown,
}

pub(crate) const fn classify_tool_surface(
    recorded: Option<u32>,
    serving: u32,
) -> SessionToolSurface {
    match recorded {
        Some(revision) if revision == serving => SessionToolSurface::Current,
        Some(_) => SessionToolSurface::Stale,
        None => SessionToolSurface::Unknown,
    }
}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    /// A LIVE SESSION THIS BUILD HAS NEVER SERVED IS UNKNOWN, NOT CURRENT.
    ///
    /// The record is in memory, and the premise that justified that — "a restart
    /// of the API ends every session anyway" — is false: terminal sessions live
    /// in a separate host so they survive it. Measured 2026-09-02, the API
    /// restarted at 11:19 and 13 sessions from 02:18 were still live holding the
    /// previous tool list, while the stale count read zero.
    ///
    /// Counting those as current is the failure. The operator's requirement was
    /// "We need a way to notify if we don't know."
    #[test]
    fn a_session_with_no_recorded_surface_is_unknown_rather_than_current() {
        let serving = 11;
        assert_eq!(
            classify_tool_surface(Some(11), serving),
            SessionToolSurface::Current
        );
        assert_eq!(
            classify_tool_surface(Some(10), serving),
            SessionToolSurface::Stale
        );
        assert_eq!(
            classify_tool_surface(None, serving),
            SessionToolSurface::Unknown,
            "a session this build has never served is not known to be fine, and counting it as \
             current is what made 13 stale sessions read as zero"
        );
    }
}

/// Restarts EVERY live worker session, whatever provider release it is on.
///
/// The operator's own lever, and it exists because one kind of staleness is
/// invisible. A worker caches its MCP tool list when it connects, so a change
/// to the agent tool surface reaches nobody until the session reconnects — and
/// unlike a stale worker engine, nothing announces it: the engine card
/// correctly says the engine is current, because it is. On 2026-09-02 the API
/// began serving tool surface revision 11 while all 13 live sessions still held
/// revision 10, and the Hive looked entirely healthy.
///
/// `restart_superseded_workers` cannot do this: it deliberately restarts only
/// sessions whose PROVIDER release has moved on, so it does nothing when the
/// providers are current and only the tool surface has changed.
///
/// This ENDS every live session on purpose. It is not offered automatically and
/// never fires on its own — the operator has held that act repeatedly, and the
/// point of a button is that they choose the moment.
pub(super) async fn restart_all_workers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let guard = state.worker_lifecycle.lock().await;
    let store = task_store(&state)?;
    let workers = store
        .active_worker_sessions()
        .map_err(|error| task_store_error(&error))?
        .into_iter()
        .map(|session| session.worker_id)
        .collect::<Vec<_>>();
    if workers.is_empty() {
        return Ok(Json(RestartWorkersResponse {
            restarted_workers: 0,
        })
        .into_response());
    }
    // Same order as the superseded path: intents first, so a worker that was
    // loaded from a conversation comes back to it rather than starting cold.
    store
        .record_worker_revival_intents(&workers, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    for worker_id in &workers {
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
    // Released before reviving: starting a worker takes this same mutex, and it
    // is not reentrant.
    drop(guard);
    let restarted_workers = revive_loaded_workers(&state, &workers).await;
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
    /// Where the pre-migration copy went, when this reload carried one.
    ///
    /// Reported rather than merely taken: a precaution nobody can see is one
    /// nobody can check, and the caller has to be able to say WHERE the escape
    /// route is without going to look for it.
    pub(crate) backup: Option<PathBuf>,
}

/// Copies the database first when the incoming build would migrate it.
///
/// Refuses the reload on any failure. A backup that warns and proceeds is the
/// same as no backup on the only day it matters.
fn back_up_before_reload(
    state: &Arc<AppState>,
    source_revision: &str,
) -> Result<Option<PathBuf>, ApiError> {
    let (Some(directory), Some(checkout)) = (
        state.database_directory.as_ref(),
        state.development_checkout_path.as_ref(),
    ) else {
        return Ok(None);
    };
    reload_backup::back_up_before_migrating_reload(
        task_store(state)?,
        directory.as_ref(),
        checkout.as_ref(),
        source_revision,
        &backup_timestamp(),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "development_reload_backup_failed",
            error.to_string(),
        )
    })
}

/// A sortable UTC stamp, so backups from one day read in order.
///
/// Matches the shape the operator's own hand-taken backups already use —
/// `pre-557d78d-20260815T214222Z.sqlite3` — so the directory stays readable
/// rather than acquiring a second naming convention.
fn backup_timestamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
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

    // A MIGRATION IS THE ONE PART OF A RELOAD THAT CANNOT BE UNDONE, so it is
    // the one part that gets an enforced precaution rather than a remembered
    // one. Reloading a bad build costs another reload; migrating onto bad data
    // costs the data, because migrations here run forward only. The copy is
    // taken BEFORE the build is requested, so a refusal leaves the running Hive
    // exactly as it was.
    //
    // This deliberately does not ask the operator first. Requiring an ask would
    // reinstate the wait-for-a-human dependency the heartbeat work exists to
    // remove, and would bite hardest overnight, when nobody is there to grant it
    // and the schema is no more dangerous than at noon.
    let backup = back_up_before_reload(state, &source_revision)?;
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
        backup,
    })
}

/// The protocol a prepared-but-deferred migration is waiting to activate.
///
/// A protocol change cannot be applied while a worker holds a terminal, so
/// `swarm-package` prepares it and leaves a marker naming the installed
/// release. The 2-minute reconcile timer completes it once the workers go
/// quiet — this is what lets the operator say "not in two minutes, now".
///
/// Read from the release's own PROTOCOL rather than remembered, because the
/// marker outlives the process that wrote it.
pub(crate) fn pending_protocol_migration(state: &AppState) -> Option<u16> {
    let marker = state
        .release_state_root
        .as_ref()?
        .join("protocol-migration.pending");
    let release = std::fs::read_to_string(marker).ok()?;
    let release = std::path::Path::new(release.trim());
    // The shell validates this path before acting on it; here it is only read
    // to decide what to wait for, so a bad value costs a missing number and
    // not an install.
    std::fs::read_to_string(release.join("PROTOCOL"))
        .ok()?
        .trim()
        .parse()
        .ok()
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
    // A DEFERRED PROTOCOL MIGRATION IS ALSO WORK THIS CARD CAN DO.
    //
    // Without this the button was a no-op for exactly the case an operator
    // most wants it: a prepared migration leaves BOTH symlinks on the old
    // release, so the engine build ids match and the engine check reports
    // nothing to do, while the Hive sits waiting for its workers to go idle.
    let pending_protocol = pending_protocol_migration(state);
    let awaiting_protocol =
        pending_protocol.is_some_and(|wanted| wanted != previous.protocol_version);
    if !worker_engine_update_required(&previous) && !awaiting_protocol {
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
                // A protocol migration moves current and host-current TOGETHER,
                // so the engine ids already agree before it runs and the engine
                // condition alone would return the instant the drain cleared —
                // reporting success while the old protocol was still serving.
                // The host's own protocol_version is the fact that changes.
                && pending_protocol.is_none_or(|wanted| status.protocol_version == wanted)
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
pub(crate) fn loaded_workers(
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
        match worker_runtime::revive_worker_process(state, *worker_id, TerminalSize::default())
            .await
        {
            Ok(None) => {}
            Ok(Some(_)) => {
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
                // The start was attempted: preserve its error, not a promise
                // that could replay an ambiguous process creation.
                if let Ok(store) = task_store(state) {
                    let _ = store.clear_worker_revival_intent(*worker_id);
                }
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
            ephemeral: false,
            mark: None,
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
