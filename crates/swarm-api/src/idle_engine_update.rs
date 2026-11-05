//! A waiting worker engine update applies by itself once every worker rests
//! (ADR 0156).
//!
//! The operator ruled "keep it automatic, but say so when it happens". The
//! card's own path does the work: record who is owed a return, stop them, ask
//! the package layer for the swap, wait for the engine, bring them back. Only
//! the stop differs. The card stops what is running; this asks the engine to
//! stop the exact running set only if every session in it is resting by the
//! engine's own evidence, and to stop none otherwise. Nothing here judges
//! whether a worker rests.
use std::{
    collections::HashSet,
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde::Serialize;
use swarm_domain::{ControlRoomEventKind, WorkerId, WorkerSessionId};
use swarm_persistence::{WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};
use swarm_terminal::{
    HostRequest, HostResponse, HostSessionSummary, MaintenanceOutcome, MaintenanceRefusal,
    TerminalHostStatus,
};

use crate::{
    ApiError, AppState, build_version,
    maintenance::{host_status_snapshot, worker_engine_update_required},
    task_store, task_store_error,
    terminal_host::{request_host, terminal_client},
    unix_timestamp, worker_engine_build_id, worker_runtime,
};

/// At most one admission a minute. Each one drains the engine for the length
/// of one request, and a busy Hive would otherwise be asked every pass.
const ATTEMPT_INTERVAL: Duration = Duration::from_secs(60);

/// What this process knows about the automatic path that the database does not.
#[derive(Default)]
pub(crate) struct IdleEngineUpdate {
    last_attempt: Mutex<Option<Instant>>,
    refusal: RwLock<Option<EngineUpdateRefusal>>,
}

impl IdleEngineUpdate {
    pub(crate) fn refusal(&self) -> Option<EngineUpdateRefusal> {
        self.refusal.read().ok().and_then(|refusal| refusal.clone())
    }

    /// Keeps `since` while the same reason holds for the same worker, so the
    /// card can say how long it has been held up by one thing.
    fn refuse(&self, reason: &'static str, worker: Option<String>, count: Option<usize>) {
        if let Ok(mut current) = self.refusal.write() {
            let since = current
                .as_ref()
                .filter(|held| held.reason == reason && held.worker == worker)
                .map_or_else(unix_timestamp, |held| held.since);
            *current = Some(EngineUpdateRefusal {
                reason,
                worker,
                count,
                since,
            });
        }
    }

    fn clear(&self) {
        if let Ok(mut current) = self.refusal.write() {
            *current = None;
        }
    }

    fn due(&self) -> bool {
        let Ok(mut last) = self.last_attempt.lock() else {
            return false;
        };
        if last.is_some_and(|at| at.elapsed() < ATTEMPT_INTERVAL) {
            return false;
        }
        *last = Some(Instant::now());
        true
    }
}

/// Why the waiting update has not applied itself, as the card shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct EngineUpdateRefusal {
    pub(crate) reason: &'static str,
    /// The worker whose session refused, by name, when one did.
    pub(crate) worker: Option<String>,
    /// How many sessions were busy, when the engine's own count refused.
    pub(crate) count: Option<usize>,
    pub(crate) since: i64,
}

/// What the card shows of the automatic path.
#[derive(Debug, Serialize)]
pub(crate) struct AutoUpdateView {
    /// Whether it is on. Turned off in Settings, under Maintenance.
    enabled: bool,
    /// When Swarm first saw the current engine update waiting.
    waiting_since: Option<i64>,
    /// Why the waiting update has not applied itself yet.
    refusal: Option<EngineUpdateRefusal>,
    /// Whom the latest update stopped, and whether each is back.
    last_update_workers: Vec<StoppedWorker>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct StoppedWorker {
    name: String,
    /// `back`, `returning`, or `needs_you` when its return failed or was held
    /// for the operator.
    status: &'static str,
}

pub(crate) fn view(
    state: &AppState,
    last: Option<&swarm_persistence::WorkerEngineUpdateAttempt>,
) -> Option<AutoUpdateView> {
    let store = task_store(state).ok()?;
    let setting = store.worker_engine_auto_update().ok()?;
    let held = store.worker_return_attention().ok()?;
    let last_update_workers = match last {
        Some(last) => store
            .worker_engine_update_workers(&last.id)
            .ok()?
            .into_iter()
            .map(|(worker, name)| {
                let status = if held.contains_key(&worker) {
                    "needs_you"
                } else if store.worker_revival_pending(worker).unwrap_or(true) {
                    "returning"
                } else {
                    "back"
                };
                StoppedWorker { name, status }
            })
            .collect(),
        None => Vec::new(),
    };
    Some(AutoUpdateView {
        enabled: setting.enabled,
        waiting_since: setting.waiting.map(|(_, since)| since),
        refusal: state.idle_engine_update.refusal(),
        last_update_workers,
    })
}

/// One supervisor pass. Returns whether the engine was replaced, so the
/// caller brings the stopped workers back after releasing the lifecycle.
pub(crate) async fn advance(state: &AppState) -> bool {
    let Ok(status) = host_status_snapshot(state).await else {
        return false;
    };
    let Ok(store) = task_store(state) else {
        return false;
    };
    let behind = worker_engine_update_required(&status);
    if let Err(error) =
        store.note_worker_engine_waiting(behind.then(worker_engine_build_id), unix_timestamp())
    {
        tracing::warn!(message = %error, "how long the engine update has waited could not be recorded");
    }
    if !behind {
        state.idle_engine_update.clear();
        return false;
    }
    if let Err((reason, count)) = ready(state, &status) {
        state.idle_engine_update.refuse(reason, None, count);
        return false;
    }
    if !state.idle_engine_update.due() {
        return false;
    }
    // Never waits: the operator's card, a worker starting, or another pass
    // holding the lifecycle is a reason to try again later, not to queue.
    let Ok(lifecycle) = state.worker_lifecycle.try_lock() else {
        return false;
    };
    let replaced = attempt(state, &status).await;
    drop(lifecycle);
    if replaced {
        state.idle_engine_update.clear();
        if let Ok(store) = task_store(state)
            && let Err(error) =
                store.record_control_room_event(ControlRoomEventKind::RuntimeChanged)
        {
            tracing::warn!(message = %error, "the automatic engine update could not publish its runtime event");
        }
        state.control_room_notify.notify_waiters();
    }
    replaced
}

/// Everything that refuses before the engine is asked anything.
fn ready(
    state: &AppState,
    status: &TerminalHostStatus,
) -> Result<(), (&'static str, Option<usize>)> {
    let store = task_store(state).map_err(|_| ("store_unavailable", None))?;
    if !store
        .worker_engine_auto_update()
        .map_err(|_| ("store_unavailable", None))?
        .enabled
    {
        return Err(("switched_off", None));
    }
    // Nothing would replace the engine, so the workers it stopped would wait
    // for a swap that never comes. The card refuses the same installation.
    if state.maintenance_request_path.is_none() {
        return Err(("engine_not_managed", None));
    }
    // A protocol migration replaces this API too. The operator applies it.
    if !matches!(
        crate::maintenance::prepared_protocol_target(state),
        Ok(None)
    ) {
        return Err(("protocol_migration_waits", None));
    }
    if !swarm_terminal::supports_maintenance_admission(status.protocol_version) {
        return Err(("engine_cannot_check_workers", None));
    }
    // An attempt at this same update that stopped workers and did not finish,
    // whether the operator's or this path's, is not repeated by itself:
    // whatever stopped it may stop the next one too, and the workers it stops
    // stay down until a swap works. A swap noticed after the fact counts no
    // stopped sessions and does not hold this.
    if store
        .last_worker_engine_update()
        .map_err(|_| ("store_unavailable", None))?
        .is_some_and(|last| {
            last.stopped_sessions.is_some()
                && last.to_version == build_version()
                && last.outcome != Some(WorkerEngineUpdateOutcome::Succeeded)
        })
    {
        return Err(("earlier_attempt_failed", None));
    }
    if status.draining {
        return Err(("engine_draining", None));
    }
    // The engine's own count of sessions that are not at rest on screen. Asking
    // for admission while one is busy would only be refused, after a drain.
    match (status.busy_sessions, status.unreadable_sessions) {
        (Some(0), Some(0)) => Ok(()),
        (Some(busy), Some(unreadable)) => Err(("workers_busy", Some(busy + unreadable))),
        _ => Err(("engine_cannot_check_workers", None)),
    }
}

/// Drains, records the returns, and asks the engine to stop the running set.
/// Holds the worker lifecycle for its whole length, like the card.
async fn attempt(state: &AppState, status: &TerminalHostStatus) -> bool {
    let Ok(client) = terminal_client(state) else {
        return false;
    };
    let running = match running_sessions(state).await {
        Ok(running) => running,
        Err(error) => {
            tracing::warn!(message = %error.message, "the automatic engine update could not list sessions");
            return false;
        }
    };
    if !matches!(
        request_host(state, HostRequest::BeginDrain).await,
        Ok(HostResponse::HostStatus { .. })
    ) {
        state
            .idle_engine_update
            .refuse("engine_unreachable", None, None);
        return false;
    }
    let returns = match Returns::record(state, &running) {
        Ok(Some(returns)) => returns,
        Ok(None) => {
            cancel_drain(state).await;
            state
                .idle_engine_update
                .refuse("session_without_worker", None, None);
            return false;
        }
        Err(error) => {
            tracing::warn!(message = %error.message, "the automatic engine update could not record who it owes a return");
            cancel_drain(state).await;
            state
                .idle_engine_update
                .refuse("returns_unrecorded", None, None);
            return false;
        }
    };
    let sessions = running
        .iter()
        .map(|session| session.session_id)
        .collect::<Vec<_>>();
    // Asked through the client itself rather than `request_host`, which folds
    // a lost answer and an engine's error into one: an error comes before any
    // stop, and a lost answer may come after some.
    let answer = client
        .request(&HostRequest::AdmitMaintenance {
            sessions: sessions.clone(),
        })
        .await;
    let outcome = match answer {
        Ok(HostResponse::Maintenance { outcome }) => outcome,
        // Any other answer, an engine error included, comes before any stop.
        Ok(_) => {
            cancel_drain(state).await;
            returns.withdraw(state, |_| true);
            state
                .idle_engine_update
                .refuse("engine_unreachable", None, None);
            return false;
        }
        Err(error) => {
            tracing::warn!(%error, "the engine's answer to the automatic engine update was lost");
            return answer_lost(state, status, &returns, &sessions).await;
        }
    };
    match outcome {
        MaintenanceOutcome::Refused { reason, session_id } => {
            cancel_drain(state).await;
            returns.withdraw(state, |_| true);
            let worker = session_id.and_then(|session| returns.name_of(session));
            state
                .idle_engine_update
                .refuse(refusal_reason(reason), worker, None);
            false
        }
        MaintenanceOutcome::Partial { stopped, failed } => {
            stopped_partway(state, status, &returns, &stopped, Some(failed)).await;
            false
        }
        MaintenanceOutcome::Stopped { session_ids } => {
            replace_engine(state, status, &returns, &session_ids).await
        }
    }
}

/// The engine took the admission and its answer never arrived: the connection
/// broke, or the reply could not be read. The engine holds its session
/// registry for the whole admission and lists sessions under that same lock,
/// so a list taken now shows what the admission left running. Whoever it
/// stopped keeps a return; whoever it did not stop owes nothing.
async fn answer_lost(
    state: &AppState,
    status: &TerminalHostStatus,
    returns: &Returns,
    requested: &[WorkerSessionId],
) -> bool {
    let Ok(now) = running_sessions(state).await else {
        // Nothing can say who was stopped, so every return this attempt made
        // stays owed. A stopped worker without one would be lost. One still
        // running holds a return it does not need, which is harmless: returns
        // are made only once the engine is replaced, and replacing it stops
        // every running worker first.
        cancel_drain(state).await;
        state.idle_engine_update.refuse("answer_lost", None, None);
        return false;
    };
    let still_running = now
        .iter()
        .map(|session| session.session_id)
        .collect::<HashSet<_>>();
    let stopped = requested
        .iter()
        .copied()
        .filter(|session| !still_running.contains(session))
        .collect::<Vec<_>>();
    if stopped.is_empty() {
        cancel_drain(state).await;
        returns.withdraw(state, |_| true);
        state.idle_engine_update.refuse("answer_lost", None, None);
        false
    } else if stopped.len() == requested.len() {
        replace_engine(state, status, returns, &stopped).await
    } else {
        stopped_partway(state, status, returns, &stopped, None).await;
        false
    }
}

/// The engine stopped some of the set and then could not stop one, or its
/// answer was lost partway. Nothing is stopped again: the stopped workers keep
/// their exact returns, which are held like every return while the engine is
/// behind and come back once it is replaced; Needs you names them. The rest
/// keep running, and this update is not tried again by itself.
async fn stopped_partway(
    state: &AppState,
    status: &TerminalHostStatus,
    returns: &Returns,
    stopped: &[WorkerSessionId],
    failed: Option<WorkerSessionId>,
) {
    cancel_drain(state).await;
    let stopped_set = stopped.iter().copied().collect::<HashSet<_>>();
    returns.withdraw(state, |session| !stopped_set.contains(&session));
    release(state, stopped).await;
    let attempt = begin(state, status, returns, stopped);
    let names = returns.names(stopped).join(", ");
    let failed_name = failed.and_then(|session| returns.name_of(session));
    let (detail, reason) = match failed {
        Some(_) => (
            format!(
                "Stopped {names} while resting, then could not stop {}. The engine was not \
                 replaced, so the stopped workers come back once it is.",
                failed_name.as_deref().unwrap_or("another session")
            ),
            "stopped_partway",
        ),
        None => (
            format!(
                "Stopped {names} while resting, then the engine's answer was lost. The engine \
                 was not replaced, so the stopped workers come back once it is."
            ),
            "answer_lost",
        ),
    };
    finish(
        state,
        attempt.as_deref(),
        WorkerEngineUpdateOutcome::Failed,
        &detail,
    );
    state.idle_engine_update.refuse(reason, failed_name, None);
    state.control_room_notify.notify_waiters();
}

/// The whole set stopped. From here this is the card's path.
async fn replace_engine(
    state: &AppState,
    status: &TerminalHostStatus,
    returns: &Returns,
    stopped: &[WorkerSessionId],
) -> bool {
    release(state, stopped).await;
    let attempt = begin(state, status, returns, stopped);
    let Some(request_path) = state.maintenance_request_path.as_ref() else {
        abandon(
            state,
            attempt.as_deref(),
            "this installation does not expose managed worker-engine maintenance",
        )
        .await;
        return false;
    };
    if let Err(error) = std::fs::write(
        request_path.as_ref(),
        format!(
            "requested_at={}\ntarget_version={}\n",
            unix_timestamp(),
            build_version()
        ),
    ) {
        abandon(
            state,
            attempt.as_deref(),
            &format!("the maintenance request could not be recorded: {error}"),
        )
        .await;
        return false;
    }
    state.control_room_notify.notify_waiters();
    let updated = crate::maintenance::await_expected_engine(state, None).await;
    let _ = std::fs::remove_file(request_path.as_ref());
    let Ok(current) = updated else {
        // As on the card: the returns stay owed, and the supervisor brings
        // the workers back once the engine reports in.
        finish(
            state,
            attempt.as_deref(),
            WorkerEngineUpdateOutcome::TimedOut,
            "the engine never reported the expected release inside the budget",
        );
        return false;
    };
    finish(
        state,
        attempt.as_deref(),
        WorkerEngineUpdateOutcome::Succeeded,
        "",
    );
    // Asked for, so the observer does not file it as a swap nobody asked for.
    state.accept_worker_engine(&current);
    tracing::info!(
        from = %status.host_version,
        to = %current.host_version,
        stopped = stopped.len(),
        "the worker engine updated by itself while every worker was resting"
    );
    true
}

/// The set is stopped and the swap cannot be asked for. Like a partial stop:
/// nothing more is stopped, and the stopped workers keep their returns.
async fn abandon(state: &AppState, attempt: Option<&str>, detail: &str) {
    cancel_drain(state).await;
    finish(state, attempt, WorkerEngineUpdateOutcome::Failed, detail);
    state
        .idle_engine_update
        .refuse("swap_not_requested", None, None);
    state.control_room_notify.notify_waiters();
}

/// Saves each stopped session's context and lets go of what it owned. A
/// failure is logged and the rest continue: the session is already stopped.
async fn release(state: &AppState, stopped: &[WorkerSessionId]) {
    for &session_id in stopped {
        let released = async {
            worker_runtime::release_stopped_session(state, session_id, true).await?;
            let store = task_store(state)?;
            store
                .release_worker_session(session_id)
                .map_err(|error| task_store_error(&error))?;
            store
                .release_session_assignments(session_id)
                .map(|_| ())
                .map_err(|error| task_store_error(&error))
        }
        .await;
        if let Err(error) = released {
            tracing::warn!(%session_id, message = %error.message, "a session the engine update stopped could not be released");
        }
    }
}

/// The history row, written once something has been stopped. A refusal stops
/// nothing and leaves no row: it is shown on the card instead.
fn begin(
    state: &AppState,
    status: &TerminalHostStatus,
    returns: &Returns,
    stopped: &[WorkerSessionId],
) -> Option<String> {
    let store = task_store(state).ok()?;
    let attempt = store
        .begin_worker_engine_update(
            &status.host_version,
            build_version(),
            None,
            Some(stopped.len()),
            WorkerEngineUpdateInitiator::Automatic,
            unix_timestamp(),
        )
        .inspect_err(|error| {
            tracing::warn!(message = %error, "this engine update will leave no history entry");
        })
        .ok()?;
    if let Err(error) =
        store.record_worker_engine_update_workers(&attempt, &returns.workers(stopped))
    {
        tracing::warn!(message = %error, "the workers this engine update stopped could not be recorded");
    }
    Some(attempt)
}

fn finish(
    state: &AppState,
    attempt: Option<&str>,
    outcome: WorkerEngineUpdateOutcome,
    detail: &str,
) {
    let Some(attempt) = attempt else {
        return;
    };
    if let Err(error) = task_store(state).and_then(|store| {
        store
            .finish_worker_engine_update(attempt, outcome, detail, unix_timestamp())
            .map_err(|error| task_store_error(&error))
    }) {
        tracing::warn!(message = %error.message, "an engine update outcome could not be recorded");
    }
}

async fn cancel_drain(state: &AppState) {
    if !matches!(
        request_host(state, HostRequest::CancelDrain).await,
        Ok(HostResponse::HostStatus { .. })
    ) {
        tracing::warn!("the automatic engine update could not cancel its drain");
    }
}

async fn running_sessions(state: &AppState) -> Result<Vec<HostSessionSummary>, ApiError> {
    let HostResponse::Sessions { sessions } =
        request_host(state, HostRequest::ListSessions).await?
    else {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "unexpected_host_response",
            "terminal host returned an unexpected session response",
        ));
    };
    Ok(sessions
        .into_iter()
        .filter(|session| session.running)
        .collect())
}

/// The returns recorded for one attempt, and which of them this attempt made.
struct Returns {
    /// Each running session's worker, by session.
    workers: Vec<(WorkerSessionId, WorkerId, String)>,
    /// Workers that were already owed a return before this attempt. Their
    /// promise is older than this attempt and is never withdrawn by it.
    already_owed: HashSet<WorkerId>,
}

impl Returns {
    /// Records a return for the worker of every running session, or `None`,
    /// recording nothing, when a running session has no worker: nothing could
    /// bring it back, so it is not stopped.
    fn record(state: &AppState, running: &[HostSessionSummary]) -> Result<Option<Self>, ApiError> {
        let store = task_store(state)?;
        let running_ids = running
            .iter()
            .map(|session| session.session_id)
            .collect::<HashSet<_>>();
        let workers = store
            .list_worker_profiles()
            .map_err(|error| task_store_error(&error))?
            .into_iter()
            .filter_map(|profile| {
                profile
                    .active_session_id
                    .filter(|session| running_ids.contains(session))
                    .map(|session| (session, profile.id, profile.name))
            })
            .collect::<Vec<_>>();
        if workers.len() != running_ids.len() {
            return Ok(None);
        }
        let mut already_owed = HashSet::new();
        for (_, worker, _) in &workers {
            if store
                .worker_revival_pending(*worker)
                .map_err(|error| task_store_error(&error))?
            {
                already_owed.insert(*worker);
            }
        }
        store
            .record_worker_revival_intents(
                &workers
                    .iter()
                    .map(|(_, worker, _)| *worker)
                    .collect::<Vec<_>>(),
                unix_timestamp(),
            )
            .map_err(|error| task_store_error(&error))?;
        Ok(Some(Self {
            workers,
            already_owed,
        }))
    }

    /// Withdraws the returns this attempt recorded for sessions it did not
    /// stop. A refusal stops nothing, so it owes nothing.
    fn withdraw(&self, state: &AppState, not_stopped: impl Fn(WorkerSessionId) -> bool) {
        let Ok(store) = task_store(state) else {
            return;
        };
        for (session, worker, _) in &self.workers {
            if not_stopped(*session)
                && !self.already_owed.contains(worker)
                && let Err(error) = store.clear_worker_revival_intent(*worker)
            {
                tracing::warn!(%worker, message = %error, "a return the engine update did not need could not be withdrawn");
            }
        }
    }

    fn name_of(&self, session: WorkerSessionId) -> Option<String> {
        self.workers
            .iter()
            .find(|(candidate, _, _)| *candidate == session)
            .map(|(_, _, name)| name.clone())
    }

    fn workers(&self, sessions: &[WorkerSessionId]) -> Vec<(WorkerId, String)> {
        self.workers
            .iter()
            .filter(|(session, _, _)| sessions.contains(session))
            .map(|(_, worker, name)| (*worker, name.clone()))
            .collect()
    }

    fn names(&self, sessions: &[WorkerSessionId]) -> Vec<String> {
        let mut names = self
            .workers(sessions)
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

/// The engine's refusal, as the card's reason code.
const fn refusal_reason(reason: MaintenanceRefusal) -> &'static str {
    match reason {
        MaintenanceRefusal::NotDraining
        | MaintenanceRefusal::LockPoisoned
        | MaintenanceRefusal::SessionChanged
        | MaintenanceRefusal::ReturnSetMismatch => "sessions_changed",
        MaintenanceRefusal::InputOrStopInFlight | MaintenanceRefusal::UnsentInput => "unsent_input",
        MaintenanceRefusal::InteractiveOwner => "operator_has_terminal",
        MaintenanceRefusal::ProviderEvidenceUnavailable | MaintenanceRefusal::TurnCountUnknown => {
            "turns_unaccounted"
        }
        MaintenanceRefusal::NotClaude => "not_claude",
        MaintenanceRefusal::TurnInProgress => "turn_in_progress",
        MaintenanceRefusal::TurnFailed => "turn_failed",
        MaintenanceRefusal::WaitingOnOperator => "waiting_on_operator",
        MaintenanceRefusal::BackgroundWork => "background_work",
        MaintenanceRefusal::ScheduledWork => "scheduled_work",
        MaintenanceRefusal::ScreenNotResting => "screen_not_resting",
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use super::*;
    use swarm_domain::ProviderKind;
    use swarm_persistence::TaskStore;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const OLD_ENGINE: &str = "74b1dfe239bb11a7201cbe85587a6cff1fed5a13e6d94d91952adf53940a30d9";

    type Admission = Box<dyn Fn(&[WorkerSessionId]) -> MaintenanceOutcome + Send>;

    /// What the fake engine was asked, and what it did.
    #[derive(Default)]
    struct Engine {
        requests: Vec<String>,
        admitted: Vec<Vec<WorkerSessionId>>,
        stopped: Vec<WorkerSessionId>,
        released: Vec<WorkerSessionId>,
        draining: bool,
        /// Stops as admitted, then drops the connection unanswered.
        lose_answer: bool,
        /// And then stops answering anything at all.
        gone: bool,
    }

    struct Hive {
        state: AppState,
        engine: Arc<Mutex<Engine>>,
        ledger: (WorkerId, WorkerSessionId),
        pantry: (WorkerId, WorkerSessionId),
        _directory: tempfile::TempDir,
    }

    impl Hive {
        /// Two resting Claude workers on an engine one build behind, which
        /// speaks `protocol`, counts `busy` sessions as busy, answers
        /// admission with `admission`, and reports the new build once the
        /// package layer's request file appears.
        fn new(protocol: u16, busy: usize, admission: Admission) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("host.sock");
            let request_path = directory.path().join("maintenance.request");
            let store = TaskStore::in_memory().unwrap();
            let bind = |name: &str| {
                let worker = store
                    .create_worker(
                        name,
                        ProviderKind::ClaudeCode,
                        "/workspace/fictional",
                        false,
                        1,
                    )
                    .unwrap();
                let session = WorkerSessionId::new();
                store.bind_worker_session(worker.id, session).unwrap();
                (worker.id, session)
            };
            let ledger = bind("Ledger");
            let pantry = bind("Pantry");
            let sessions = [ledger.1, pantry.1];
            let engine = Arc::new(Mutex::new(Engine::default()));
            let recorder = Arc::clone(&engine);
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            let swapped = request_path.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        continue;
                    }
                    let request: HostRequest = serde_json::from_str(&line).unwrap();
                    let admitting = matches!(request, HostRequest::AdmitMaintenance { .. });
                    let response = answer(
                        &recorder, &admission, &sessions, protocol, busy, &swapped, request,
                    );
                    let (lose, gone) = {
                        let engine = recorder.lock().unwrap();
                        (engine.lose_answer, engine.gone)
                    };
                    if admitting && lose {
                        if gone {
                            return;
                        }
                        continue;
                    }
                    let mut bytes = serde_json::to_vec(&response).unwrap();
                    bytes.push(b'\n');
                    let _ = reader.get_mut().write_all(&bytes).await;
                }
            });
            let state = AppState::default()
                .with_terminal_host(crate::HostClient::new(socket), "fixture-only")
                .with_task_store(store)
                .with_maintenance_request_path(request_path)
                .with_maintenance_timeout(Duration::from_secs(3));
            Self {
                state,
                engine,
                ledger,
                pantry,
                _directory: directory,
            }
        }

        fn store(&self) -> &TaskStore {
            task_store(&self.state).unwrap()
        }

        fn asked(&self, request: &str) -> usize {
            let engine = self.engine.lock().unwrap();
            engine
                .requests
                .iter()
                .filter(|name| *name == request)
                .count()
        }

        fn owed(&self) -> Vec<WorkerId> {
            let mut owed = self.store().worker_revival_intents().unwrap();
            owed.sort_by_key(ToString::to_string);
            owed
        }

        fn refusal(&self) -> Option<(&'static str, Option<String>)> {
            self.state
                .idle_engine_update
                .refusal()
                .map(|refusal| (refusal.reason, refusal.worker))
        }

        fn rearm(&self) {
            *self.state.idle_engine_update.last_attempt.lock().unwrap() = None;
        }

        fn lose_answers(&self, then_gone: bool) {
            let mut engine = self.engine.lock().unwrap();
            engine.lose_answer = true;
            engine.gone = then_gone;
        }
    }

    fn answer(
        engine: &Mutex<Engine>,
        admission: &Admission,
        sessions: &[WorkerSessionId],
        protocol: u16,
        busy: usize,
        request_path: &Path,
        request: HostRequest,
    ) -> HostResponse {
        let mut engine = engine.lock().unwrap();
        engine.requests.push(
            serde_json::to_value(&request).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        let swapped = request_path.exists();
        let status = |engine: &Engine| TerminalHostStatus {
            protocol_version: protocol,
            host_version: if swapped {
                build_version()
            } else {
                "1.0.0-old"
            }
            .to_owned(),
            host_build_id: Some(
                if swapped {
                    worker_engine_build_id()
                } else {
                    OLD_ENGINE
                }
                .to_owned(),
            ),
            // A swapped engine is a new process, which is not draining.
            draining: engine.draining && !swapped,
            running_sessions: sessions.len() - engine.stopped.len(),
            retained_sessions: sessions.len(),
            busy_sessions: Some(busy),
            unreadable_sessions: Some(0),
            resources: None,
            takeover_relay: true,
        };
        match request {
            HostRequest::HostStatus => HostResponse::HostStatus {
                status: status(&engine),
            },
            HostRequest::BeginDrain => {
                engine.draining = true;
                HostResponse::HostStatus {
                    status: status(&engine),
                }
            }
            HostRequest::CancelDrain => {
                engine.draining = false;
                HostResponse::HostStatus {
                    status: status(&engine),
                }
            }
            HostRequest::ListSessions => HostResponse::Sessions {
                sessions: sessions
                    .iter()
                    .map(|&session_id| swarm_terminal::HostSessionSummary {
                        session_id,
                        running: !engine.stopped.contains(&session_id),
                        stop_pending_release: engine.stopped.contains(&session_id),
                        resources: None,
                        last_output_at: None,
                        recovery_attempt: None,
                        provider_start: None,
                        provider_selection: None,
                        continuation_unavailable: false,
                        continuation_recovery: None,
                        provider_executable: None,
                        provider_tools_relay: None,
                    })
                    .collect(),
            },
            HostRequest::AdmitMaintenance { sessions } => {
                engine.admitted.push(sessions.clone());
                let outcome = admission(&sessions);
                match &outcome {
                    MaintenanceOutcome::Stopped { session_ids } => {
                        engine.stopped.extend(session_ids.iter().copied());
                    }
                    MaintenanceOutcome::Partial { stopped, .. } => {
                        engine.stopped.extend(stopped.iter().copied());
                    }
                    MaintenanceOutcome::Refused { .. } => {}
                }
                HostResponse::Maintenance { outcome }
            }
            HostRequest::Stop { session_id } => {
                assert!(
                    engine.stopped.contains(&session_id),
                    "only a session admission stopped is released"
                );
                engine.released.push(session_id);
                HostResponse::Acknowledged
            }
            other => panic!("the automatic engine update sent an unexpected request: {other:?}"),
        }
    }

    fn stop_all() -> Admission {
        Box::new(|sessions| MaintenanceOutcome::Stopped {
            session_ids: sessions.to_vec(),
        })
    }

    /// The operator's ruling: it applies by itself, and says so, naming the
    /// workers it stopped and the engine.
    #[tokio::test]
    async fn a_resting_hive_updates_its_engine_by_itself_and_names_who_it_stopped() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        assert!(advance(&hive.state).await);

        let engine = hive.engine.lock().unwrap();
        let mut expected = vec![hive.ledger.1, hive.pantry.1];
        expected.sort_by_key(ToString::to_string);
        let mut admitted = engine.admitted[0].clone();
        admitted.sort_by_key(ToString::to_string);
        assert_eq!(admitted, expected, "the exact running set, once");
        assert_eq!(engine.admitted.len(), 1);
        assert_eq!(engine.released.len(), 2, "each stopped session is released");
        assert!(
            engine.requests.iter().position(|r| r == "begin_drain")
                < engine
                    .requests
                    .iter()
                    .position(|r| r == "admit_maintenance")
        );
        drop(engine);

        let mut owed_workers = vec![hive.ledger.0, hive.pantry.0];
        owed_workers.sort_by_key(ToString::to_string);
        assert_eq!(hive.owed(), owed_workers, "both are owed a return");
        let last = hive.store().last_worker_engine_update().unwrap().unwrap();
        assert_eq!(last.initiated, WorkerEngineUpdateInitiator::Automatic);
        assert_eq!(last.outcome, Some(WorkerEngineUpdateOutcome::Succeeded));
        assert_eq!(last.stopped_sessions, Some(2));
        assert_eq!(last.to_version, build_version());
        let shown = serde_json::to_value(view(&hive.state, Some(&last)).unwrap()).unwrap();
        assert_eq!(shown["enabled"], true);
        assert_eq!(shown["refusal"], serde_json::Value::Null);
        assert_eq!(
            shown["last_update_workers"],
            serde_json::json!([
                {"name": "Ledger", "status": "returning"},
                {"name": "Pantry", "status": "returning"},
            ])
        );
        assert_eq!(
            hive.store()
                .worker_engine_auto_update()
                .unwrap()
                .waiting
                .map(|(engine, _)| engine),
            Some(worker_engine_build_id().to_owned()),
            "recorded on the pass that found it waiting"
        );
    }

    /// A refusal stops nothing, owes nothing, records no attempt, and names
    /// the worker that held it up. A return owed from before is kept.
    #[tokio::test]
    async fn a_refusal_stops_nothing_and_owes_nothing() {
        let pantry = Arc::new(Mutex::new(None));
        let named = Arc::clone(&pantry);
        let hive = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(move |_| MaintenanceOutcome::Refused {
                reason: MaintenanceRefusal::TurnInProgress,
                session_id: *named.lock().unwrap(),
            }),
        );
        *pantry.lock().unwrap() = Some(hive.pantry.1);
        hive.store()
            .record_worker_revival_intents(&[hive.ledger.0], 1)
            .unwrap();

        assert!(!advance(&hive.state).await);
        assert_eq!(hive.asked("cancel_drain"), 1);
        assert!(hive.engine.lock().unwrap().released.is_empty());
        assert_eq!(
            hive.owed(),
            vec![hive.ledger.0],
            "only the older promise is left"
        );
        assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);
        assert_eq!(
            hive.refusal(),
            Some(("turn_in_progress", Some("Pantry".to_owned())))
        );

        // Asked again only after a minute.
        assert!(!advance(&hive.state).await);
        assert_eq!(hive.asked("admit_maintenance"), 1);
        hive.rearm();
        assert!(!advance(&hive.state).await);
        assert_eq!(hive.asked("admit_maintenance"), 2);
    }

    /// Queen's change (4): a partial stop keeps the exact returns, names the
    /// workers not brought back in Needs you, and is never repeated.
    #[tokio::test]
    async fn a_partial_stop_keeps_its_returns_and_is_never_repeated() {
        let sessions = Arc::new(Mutex::new(None));
        let script = Arc::clone(&sessions);
        let hive = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(move |_| {
                let (stopped, failed) = script.lock().unwrap().unwrap();
                MaintenanceOutcome::Partial {
                    stopped: vec![stopped],
                    failed,
                }
            }),
        );
        *sessions.lock().unwrap() = Some((hive.ledger.1, hive.pantry.1));

        assert!(!advance(&hive.state).await);
        assert_eq!(hive.asked("cancel_drain"), 1);
        assert_eq!(hive.engine.lock().unwrap().released, vec![hive.ledger.1]);
        assert!(
            hive.store().worker_revival_pending(hive.ledger.0).unwrap(),
            "Ledger keeps its return, which waits for the engine"
        );
        assert!(hive.store().worker_return_attention().unwrap().is_empty());
        assert!(
            !hive.store().worker_revival_pending(hive.pantry.0).unwrap(),
            "Pantry was never stopped, so nothing is owed it"
        );
        let last = hive.store().last_worker_engine_update().unwrap().unwrap();
        assert_eq!(last.outcome, Some(WorkerEngineUpdateOutcome::Failed));
        assert_eq!(last.stopped_sessions, Some(1));
        assert!(last.detail.contains("Ledger") && last.detail.contains("Pantry"));
        let shown = serde_json::to_value(view(&hive.state, Some(&last)).unwrap()).unwrap();
        assert_eq!(
            shown["last_update_workers"],
            serde_json::json!([{"name": "Ledger", "status": "returning"}])
        );

        hive.rearm();
        assert!(!advance(&hive.state).await);
        assert_eq!(hive.asked("admit_maintenance"), 1, "never asked again");
        assert_eq!(hive.asked("begin_drain"), 1);
        assert_eq!(hive.refusal(), Some(("earlier_attempt_failed", None)));
    }

    /// ADR 0085: a running session no worker owns could not be brought back,
    /// so it refuses before anyone is owed anything or asked to stop.
    #[tokio::test]
    async fn a_session_no_worker_owns_is_never_stopped() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        hive.store().release_worker_session(hive.pantry.1).unwrap();

        assert!(!advance(&hive.state).await);
        assert_eq!(hive.refusal(), Some(("session_without_worker", None)));
        assert_eq!(hive.asked("admit_maintenance"), 0);
        assert_eq!(hive.asked("cancel_drain"), 1);
        assert!(hive.owed().is_empty());
        assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);
    }

    /// ADR 0085: when the answer is lost, whoever the engine stopped keeps a
    /// return, whoever it did not owes nothing, and nobody is stopped twice.
    #[tokio::test]
    async fn a_lost_answer_is_read_back_from_what_the_engine_left_running() {
        // Ledger stopped and Pantry not: a partial stop, never repeated.
        let sessions = Arc::new(Mutex::new(None));
        let script = Arc::clone(&sessions);
        let partly = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(move |_| {
                let (stopped, failed) = script.lock().unwrap().unwrap();
                MaintenanceOutcome::Partial {
                    stopped: vec![stopped],
                    failed,
                }
            }),
        );
        *sessions.lock().unwrap() = Some((partly.ledger.1, partly.pantry.1));
        partly.lose_answers(false);
        assert!(!advance(&partly.state).await);
        assert_eq!(
            partly.owed(),
            vec![partly.ledger.0],
            "Ledger keeps its return"
        );
        assert_eq!(
            partly.engine.lock().unwrap().released,
            vec![partly.ledger.1]
        );
        let last = partly.store().last_worker_engine_update().unwrap().unwrap();
        assert_eq!(last.outcome, Some(WorkerEngineUpdateOutcome::Failed));
        assert_eq!(last.stopped_sessions, Some(1));
        assert!(last.detail.contains("Ledger") && last.detail.contains("answer was lost"));
        assert_eq!(partly.refusal(), Some(("answer_lost", None)));
        partly.rearm();
        assert!(!advance(&partly.state).await);
        assert_eq!(partly.asked("admit_maintenance"), 1, "never asked again");

        // Nobody stopped: nothing owed and nothing recorded.
        let nobody = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(|_| MaintenanceOutcome::Refused {
                reason: MaintenanceRefusal::TurnInProgress,
                session_id: None,
            }),
        );
        nobody.lose_answers(false);
        assert!(!advance(&nobody.state).await);
        assert!(nobody.owed().is_empty());
        assert_eq!(nobody.store().last_worker_engine_update().unwrap(), None);
        assert_eq!(nobody.asked("cancel_drain"), 1);
        assert_eq!(nobody.refusal(), Some(("answer_lost", None)));

        // Everybody stopped: the update goes on as if the answer had come.
        let everyone = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        everyone.lose_answers(false);
        assert!(advance(&everyone.state).await);
        assert_eq!(
            everyone
                .store()
                .last_worker_engine_update()
                .unwrap()
                .unwrap()
                .outcome,
            Some(WorkerEngineUpdateOutcome::Succeeded)
        );

        // The engine gone with its answer: nothing says who stopped, so every
        // return stays owed.
        let gone = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        gone.lose_answers(true);
        assert!(!advance(&gone.state).await);
        let mut both = vec![gone.ledger.0, gone.pantry.0];
        both.sort_by_key(ToString::to_string);
        assert_eq!(gone.owed(), both);
        assert_eq!(gone.store().last_worker_engine_update().unwrap(), None);
        assert_eq!(gone.refusal(), Some(("answer_lost", None)));
    }

    /// Switched off, an installation nothing would swap, an engine that cannot
    /// check its workers, a busy count, and an earlier attempt at this update
    /// that failed for the operator never drain the engine or record a return.
    #[tokio::test]
    async fn what_refuses_before_asking_the_engine_never_drains_it() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        hive.store()
            .set_worker_engine_auto_update(false, 1)
            .unwrap();
        assert!(!advance(&hive.state).await);
        assert_eq!(hive.refusal(), Some(("switched_off", None)));

        let older = Hive::new(17, 0, stop_all());
        assert!(!advance(&older.state).await);
        assert_eq!(older.refusal(), Some(("engine_cannot_check_workers", None)));

        let busy = Hive::new(swarm_terminal::PROTOCOL_VERSION, 1, stop_all());
        assert!(!advance(&busy.state).await);
        assert_eq!(
            busy.state
                .idle_engine_update
                .refusal()
                .map(|refusal| (refusal.reason, refusal.count)),
            Some(("workers_busy", Some(1)))
        );

        let mut unmanaged = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        unmanaged.state.maintenance_request_path = None;
        assert!(!advance(&unmanaged.state).await);
        assert_eq!(unmanaged.refusal(), Some(("engine_not_managed", None)));

        let failed_before = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        let attempt = failed_before
            .store()
            .begin_worker_engine_update(
                "1.0.0-old",
                build_version(),
                None,
                Some(2),
                WorkerEngineUpdateInitiator::Operator,
                1,
            )
            .unwrap();
        failed_before
            .store()
            .finish_worker_engine_update(&attempt, WorkerEngineUpdateOutcome::TimedOut, "", 2)
            .unwrap();
        assert!(!advance(&failed_before.state).await);
        assert_eq!(
            failed_before.refusal(),
            Some(("earlier_attempt_failed", None))
        );

        for hive in [&hive, &older, &busy, &unmanaged, &failed_before] {
            assert_eq!(hive.asked("begin_drain"), 0);
            assert_eq!(hive.asked("admit_maintenance"), 0);
            assert!(hive.owed().is_empty());
        }
        for hive in [&hive, &older, &busy, &unmanaged] {
            assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);
        }
    }
}
