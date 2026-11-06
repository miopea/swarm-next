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
//!
//! What a refusal leaves behind is Swarm's (ADR 0161). When the engine refuses
//! a session for a reason only a new session clears, and Swarm's own reading
//! of that session's screen says it rests, Swarm restarts that worker alone,
//! once, and the engine is asked again. The engine names every session it
//! refuses, and Swarm settles them all on that same answer (ADR 0162). The
//! engine still decides the swap.
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde::Serialize;
use swarm_domain::{ControlRoomEventKind, WorkerId, WorkerSessionId};
use swarm_persistence::{WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};
use swarm_terminal::{
    HostRequest, HostResponse, HostSessionSummary, MaintenanceOutcome, MaintenanceRefusal,
    ProviderActivity, TerminalHostStatus,
};

use crate::{
    ApiError, AppState, build_version,
    coordination_delivery::{
        BusyPolicy, Cadence, CoordinationMessage, TerminalSubmission, submit_coordination_message,
    },
    maintenance::{host_status_snapshot, worker_engine_update_required},
    provider_activity::ProviderSignals,
    task_store, task_store_error,
    terminal_host::{request_host, terminal_client},
    unix_timestamp, worker_engine_build_id, worker_runtime,
};

/// At most one admission a minute. Each one drains the engine for the length
/// of one request, and a busy Hive would otherwise be asked every pass.
const ATTEMPT_INTERVAL: Duration = Duration::from_secs(60);

/// The engine's refusals that a new session clears, because each is about the
/// turns the engine counted in this one: a turn it saw start and not end (Esc
/// sends no report), a count it could not keep, a session whose hooks never
/// reported, a turn that failed, or a session cron, which Claude ends with its
/// process anyway. Anything else is work a restart would lose, or the operator.
const RESTART_CLEARS: [&str; 5] = [
    "turn_in_progress",
    "turns_unaccounted",
    "no_startup_evidence",
    "turn_failed",
    "scheduled_work",
];

/// More restarts than one waiting update could make: one per worker.
const RESTARTED_KEPT: usize = 64;

/// What this process knows about the automatic path that the database does not.
#[derive(Default)]
pub(crate) struct IdleEngineUpdate {
    last_attempt: Mutex<Option<Instant>>,
    refusal: RwLock<Option<EngineUpdateRefusal>>,
    /// Every session the engine's last answer refused, and what Swarm does
    /// about each (ADR 0162). The first is also `refusal`.
    holders: Mutex<Vec<EngineHolder>>,
    /// Who Swarm restarted while an update waited, until each is told.
    restarted: Mutex<Vec<Restarted>>,
}

/// A session the engine refused, and what Swarm does about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct EngineHolder {
    /// The worker it belongs to, by name, when Swarm knows it.
    worker: Option<String>,
    reason: &'static str,
    /// What Swarm does about a refusal only a new session clears: `restarts`
    /// it at the next admission, or leaves it because the worker is `in_use`,
    /// is `not_resting` by Swarm's own reading, or was `already_restarted` for
    /// this update. None for a refusal a restart would not clear.
    swarm: Option<&'static str>,
    #[serde(skip)]
    session: WorkerSessionId,
}

/// A worker Swarm restarted alone while an update waited.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Restarted {
    worker: WorkerId,
    name: String,
    at: i64,
    /// The engine that was waiting. A later update may restart it again.
    engine: String,
}

impl IdleEngineUpdate {
    pub(crate) fn refusal(&self) -> Option<EngineUpdateRefusal> {
        self.refusal.read().ok().and_then(|refusal| refusal.clone())
    }

    /// Keeps `since` while the same reason holds for the same worker, so the
    /// card can say how long it has been held up by one thing.
    ///
    /// The journal gets a line only when that changes, not every pass: the
    /// card forgets on a restart or once the update lands, and this is what is
    /// left to say why it waited.
    fn refuse(&self, reason: &'static str, worker: Option<String>, count: Option<usize>) {
        self.hold(reason, worker, count, None);
        self.set_holders(Vec::new());
    }

    /// The engine refused these sessions, the one it names first first.
    fn refuse_sessions(&self, holders: Vec<EngineHolder>) {
        let Some(first) = holders.first() else {
            return;
        };
        self.hold(
            first.reason,
            first.worker.clone(),
            None,
            Some(first.session),
        );
        self.set_holders(holders);
    }

    fn holders(&self) -> Vec<EngineHolder> {
        self.holders
            .lock()
            .map(|holders| holders.clone())
            .unwrap_or_default()
    }

    fn set_holders(&self, holders: Vec<EngineHolder>) {
        if let Ok(mut current) = self.holders.lock() {
            *current = holders;
        }
    }

    /// What still holds the update once Swarm settled what it could.
    fn settled(&self, left: Vec<EngineHolder>) {
        if left.is_empty() {
            self.clear();
        } else {
            self.refuse_sessions(left);
        }
    }

    fn hold(
        &self,
        reason: &'static str,
        worker: Option<String>,
        count: Option<usize>,
        session: Option<WorkerSessionId>,
    ) {
        let Ok(mut current) = self.refusal.write() else {
            return;
        };
        let held_since = current
            .as_ref()
            .filter(|held| held.reason == reason && held.worker == worker)
            .map(|held| held.since);
        let since = held_since.unwrap_or_else(unix_timestamp);
        if held_since.is_none() {
            tracing::info!(
                reason,
                worker = worker.as_deref(),
                count,
                since,
                "the automatic engine update is held up"
            );
        }
        *current = Some(EngineUpdateRefusal {
            reason,
            worker,
            count,
            since,
            session,
        });
    }

    fn restarted_before(&self, worker: WorkerId, engine: &str) -> bool {
        self.restarted.lock().is_ok_and(|restarted| {
            restarted
                .iter()
                .any(|entry| entry.worker == worker && entry.engine == engine)
        })
    }

    fn note_restarted(&self, entry: Restarted) {
        let Ok(mut restarted) = self.restarted.lock() else {
            return;
        };
        if restarted.len() == RESTARTED_KEPT {
            restarted.remove(0);
        }
        restarted.push(entry);
    }

    fn restarted(&self) -> Vec<Restarted> {
        self.restarted
            .lock()
            .map(|restarted| restarted.clone())
            .unwrap_or_default()
    }

    fn told(&self, worker: WorkerId) {
        if let Ok(mut restarted) = self.restarted.lock() {
            restarted.retain(|entry| entry.worker != worker);
        }
    }

    fn clear(&self) {
        self.set_holders(Vec::new());
        let Ok(mut current) = self.refusal.write() else {
            return;
        };
        if let Some(held) = current.take() {
            tracing::info!(
                reason = held.reason,
                worker = held.worker.as_deref(),
                since = held.since,
                "the automatic engine update is no longer held up"
            );
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
    /// The session the engine named, which the card has no use for.
    #[serde(skip)]
    pub(crate) session: Option<WorkerSessionId>,
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
    /// Every worker the engine's last answer refused, and what Swarm does
    /// about each (ADR 0162).
    holders: Vec<EngineHolder>,
    /// Workers Swarm restarted alone while this update waited.
    restarted: Vec<WorkerRestart>,
    /// Whom the latest update stopped, and whether each is back.
    last_update_workers: Vec<StoppedWorker>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct WorkerRestart {
    worker: String,
    at: i64,
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
    let refusal = state.idle_engine_update.refusal();
    let holders = state.idle_engine_update.holders();
    let waiting = setting.waiting.as_ref().map(|(engine, _)| engine.as_str());
    let restarted = state
        .idle_engine_update
        .restarted()
        .into_iter()
        .filter(|entry| Some(entry.engine.as_str()) == waiting)
        .map(|entry| WorkerRestart {
            worker: entry.name,
            at: entry.at,
        })
        .collect();
    Some(AutoUpdateView {
        enabled: setting.enabled,
        waiting_since: setting.waiting.map(|(_, since)| since),
        refusal,
        holders,
        restarted,
        last_update_workers,
    })
}

/// One supervisor pass. Returns whether the engine was replaced, so the
/// caller brings the stopped workers back after releasing the lifecycle.
///
/// `observed` is Swarm's own reading of each session this pass, which decides
/// only whether a worker the engine refused is restarted alone (ADR 0161).
pub(crate) async fn advance(
    state: &AppState,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> bool {
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
        tell_restarted(state, observed).await;
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
    if !replaced {
        settle(state, observed).await;
    }
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

/// What Swarm does about one session the engine refused (ADR 0161, 0162).
#[derive(Debug, Eq, PartialEq)]
enum Settle {
    /// A reason a restart does not clear: the engine's answer stands.
    Leave,
    /// The operator is using it, and a restart loses what they type.
    InUse,
    /// Swarm's own reading does not say it rests either.
    NotResting,
    /// Restarted once already while this update waited: the button's now.
    AlreadyRestarted,
    /// Only a new session clears it, and Swarm reads it resting: now, on
    /// this same answer, with every other one like it.
    Restart,
}

/// Every input is an observation.
fn settle_step(reason: &str, resting: bool, engaged: bool, restarted_before: bool) -> Settle {
    if !RESTART_CLEARS.contains(&reason) {
        return Settle::Leave;
    }
    if engaged {
        return Settle::InUse;
    }
    if !resting {
        return Settle::NotResting;
    }
    if restarted_before {
        return Settle::AlreadyRestarted;
    }
    Settle::Restart
}

/// Resting at its prompt, by Swarm's own reading, with nothing it can see
/// running: the test provider updates use before typing `/restart` (ADR 0122).
fn resting(signals: Option<&ProviderSignals>) -> bool {
    signals.is_some_and(|signals| {
        signals.activity == ProviderActivity::Resting && !signals.background_work.is_running()
    })
}

/// Runs after each refused admission, on that same answer. Every session it
/// named that only a new session clears, and that Swarm reads as resting, is
/// restarted now, so the next admission finds them all on new sessions
/// rather than one per minute (ADR 0162). The rest stay named, with why.
async fn settle(state: &AppState, observed: &HashMap<WorkerSessionId, ProviderSignals>) {
    let holders = state.idle_engine_update.holders();
    if holders.is_empty() {
        return;
    }
    let Ok(store) = task_store(state) else {
        return;
    };
    let Ok(profiles) = store.list_worker_profiles() else {
        return;
    };
    // Unreadable counts as engaged: a restart while the operator types loses
    // what they typed.
    let engaged = store.engaged_devices_by_worker(unix_timestamp()).ok();
    let engine = worker_engine_build_id();
    let mut left = Vec::with_capacity(holders.len());
    let mut due = Vec::new();
    for mut holder in holders {
        let Some(profile) = profiles
            .iter()
            .find(|profile| profile.active_session_id == Some(holder.session))
        else {
            left.push(holder);
            continue;
        };
        holder.swarm = match settle_step(
            holder.reason,
            resting(observed.get(&holder.session)),
            engaged
                .as_ref()
                .is_none_or(|engaged| engaged.contains_key(&profile.id)),
            state
                .idle_engine_update
                .restarted_before(profile.id, engine),
        ) {
            Settle::Leave => None,
            Settle::InUse => Some("in_use"),
            Settle::NotResting => Some("not_resting"),
            Settle::AlreadyRestarted => Some("already_restarted"),
            Settle::Restart => {
                due.push((profile.id, profile.name.clone(), holder));
                continue;
            }
        };
        left.push(holder);
    }
    if !due.is_empty() {
        // Nothing Swarm sends may land between its last look and the stops. A
        // pass delivering now is a reason to try at the next admission.
        if let Ok(_deliveries) = state.coordination_delivery.try_lock() {
            for (worker, name, mut holder) in due {
                if !restart(state, worker, name, holder.session, holder.reason).await {
                    holder.swarm = Some("restarts");
                    left.push(holder);
                }
            }
        } else {
            left.extend(due.into_iter().map(|(_, _, mut holder)| {
                holder.swarm = Some("restarts");
                holder
            }));
        }
        state.control_room_notify.notify_waiters();
    }
    state.idle_engine_update.settled(left);
}

/// Whether this worker has had its one restart for this update: done, or
/// tried and failed, which leaves a return owed that Needs you shows. False
/// when it could not be tried now, which the next admission tries again.
async fn restart(
    state: &AppState,
    worker: WorkerId,
    name: String,
    session: WorkerSessionId,
    reason: &'static str,
) -> bool {
    match worker_runtime::restart_resting_worker(state, worker, session).await {
        Ok(false) => return false,
        Ok(true) => tracing::info!(
            worker = name,
            reason,
            "Swarm restarted a resting worker alone because the engine could not vouch for it"
        ),
        Err(error) => tracing::warn!(
            worker = name,
            reason,
            message = %error.message,
            "Swarm could not restart a resting worker the engine could not vouch for"
        ),
    }
    state.idle_engine_update.note_restarted(Restarted {
        worker,
        name,
        at: unix_timestamp(),
        engine: worker_engine_build_id().to_owned(),
    });
    true
}

/// Once no update waits, each worker Swarm restarted is told, when it next
/// rests, what happened and that its scheduled jobs did not survive.
/// Claude ends a session's crons with its process, and Swarm cannot list them.
async fn tell_restarted(state: &AppState, observed: &HashMap<WorkerSessionId, ProviderSignals>) {
    let restarted = state.idle_engine_update.restarted();
    if restarted.is_empty() {
        return;
    }
    let (Ok(store), Ok(client)) = (task_store(state), terminal_client(state)) else {
        return;
    };
    for entry in restarted {
        let Ok(profile) = store.get_worker_profile(entry.worker) else {
            // No such worker any more: nobody to tell.
            state.idle_engine_update.told(entry.worker);
            continue;
        };
        let Some(session) = profile.active_session_id else {
            continue;
        };
        if !resting(observed.get(&session)) {
            continue;
        }
        let outcome = submit_coordination_message(store, client, session, restarted_notice()).await;
        // A deferral keeps it for the next pass. Anything else is final: an
        // uncertain write may have landed, and twice is worse than once.
        match outcome {
            Ok(TerminalSubmission::Deferred(_)) => continue,
            Ok(TerminalSubmission::Acknowledged) => tracing::info!(
                worker = entry.name,
                "Swarm told a worker it restarted that its scheduled jobs are gone"
            ),
            Ok(TerminalSubmission::Uncertain) => tracing::warn!(
                worker = entry.name,
                "Swarm may not have told a worker it restarted that its scheduled jobs are gone"
            ),
            Ok(TerminalSubmission::Rejected { code, .. }) => tracing::warn!(
                worker = entry.name,
                code,
                "Swarm could not tell a worker it restarted that its scheduled jobs are gone"
            ),
            Err(error) => tracing::warn!(
                worker = entry.name,
                %error,
                "Swarm could not tell a worker it restarted that its scheduled jobs are gone"
            ),
        }
        state.idle_engine_update.told(entry.worker);
    }
}

/// Says what happened and asks for nothing new, so a worker that was waiting
/// on someone keeps waiting.
fn restarted_notice() -> CoordinationMessage {
    CoordinationMessage {
        cadence: Cadence::Immediate,
        busy: BusyPolicy::Wait,
        bytes: b"[Swarm] While a worker engine update waited, Swarm restarted this session \
                 because the engine could not confirm it was resting. The update has now \
                 applied. Your conversation and your work are as you left them. Scheduled jobs \
                 (CronCreate, /loop) end with a session, so any you had set are gone: set again \
                 the ones you still need. If you were waiting on someone, keep waiting.\r"
            .to_vec(),
        marker: b"Swarm restarted this session".to_vec(),
    }
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
        MaintenanceOutcome::Refused {
            reason,
            session_id,
            others,
        } => {
            cancel_drain(state).await;
            returns.withdraw(state, |_| true);
            match session_id {
                Some(session) => {
                    let holders = std::iter::once((session, reason))
                        .chain(others.iter().map(|other| (other.session_id, other.reason)))
                        .map(|(session, reason)| EngineHolder {
                            worker: returns.name_of(session),
                            reason: refusal_reason(reason),
                            swarm: None,
                            session,
                        })
                        .collect();
                    state.idle_engine_update.refuse_sessions(holders);
                }
                None => {
                    state
                        .idle_engine_update
                        .refuse(refusal_reason(reason), None, None);
                }
            }
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
        // Two causes, told apart so the journal and the card can say which.
        MaintenanceRefusal::TurnCountUnknown => "turns_unaccounted",
        MaintenanceRefusal::ProviderEvidenceUnavailable => "no_startup_evidence",
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

    /// What a test's tracing subscriber wrote, as the journal would show it.
    #[derive(Clone, Default)]
    struct Journal(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Journal {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The refusal is held only in memory, so without a line here the first
    /// automatic run could not be reconstructed once the update landed. One
    /// line when it starts waiting on a reason, one each time the reason or
    /// the worker changes, one when it clears. A pass that finds the same
    /// thing again adds nothing, even when the busy count moves.
    #[test]
    fn the_journal_says_once_why_the_update_waits_and_once_when_it_stops() {
        let journal = Journal::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer({
                let journal = journal.clone();
                move || journal.clone()
            })
            .with_ansi(false)
            .finish();
        let update = IdleEngineUpdate::default();
        tracing::subscriber::with_default(subscriber, || {
            update.clear();
            update.refuse("workers_busy", None, Some(5));
            update.refuse("workers_busy", None, Some(5));
            update.refuse("workers_busy", None, Some(4));
            update.refuse("screen_not_resting", Some("ledger".to_owned()), None);
            update.refuse("screen_not_resting", Some("ledger".to_owned()), None);
            update.refuse("screen_not_resting", Some("pantry".to_owned()), None);
            update.clear();
            update.clear();
        });

        let logged = String::from_utf8(journal.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(lines.len(), 4, "{logged}");
        for (line, expected) in lines.iter().zip([
            [
                "the automatic engine update is held up",
                "workers_busy",
                "count=5",
            ],
            [
                "the automatic engine update is held up",
                "screen_not_resting",
                "ledger",
            ],
            [
                "the automatic engine update is held up",
                "screen_not_resting",
                "pantry",
            ],
            [
                "the automatic engine update is no longer held up",
                "screen_not_resting",
                "pantry",
            ],
        ]) {
            for part in expected {
                assert!(line.contains(part), "{part:?} missing from {line}");
            }
            assert!(line.contains("since="), "{line}");
        }
        assert_eq!(update.refusal(), None);
    }

    type Admission = Box<dyn Fn(&[WorkerSessionId]) -> MaintenanceOutcome + Send>;

    /// What the fake engine was asked, and what it did.
    #[derive(Default)]
    struct Engine {
        requests: Vec<String>,
        admitted: Vec<Vec<WorkerSessionId>>,
        stopped: Vec<WorkerSessionId>,
        released: Vec<WorkerSessionId>,
        /// Sessions stopped one at a time, outside admission.
        retained_stops: Vec<WorkerSessionId>,
        /// The conversation each start asked for, and the session it got.
        started: Vec<String>,
        restarted_as: Vec<WorkerSessionId>,
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
                // Its own sessions, and any it started since: a restarted
                // worker's new session runs like any other.
                sessions: sessions
                    .iter()
                    .chain(&engine.restarted_as)
                    .map(|&session_id| swarm_terminal::HostSessionSummary {
                        session_id,
                        running: !engine.stopped.contains(&session_id),
                        // A released session is no longer waiting to be.
                        stop_pending_release: engine.stopped.contains(&session_id)
                            && !engine.released.contains(&session_id),
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
                    "only a session admission or a retained stop stopped is released"
                );
                engine.released.push(session_id);
                HostResponse::Acknowledged
            }
            request @ (HostRequest::Ping
            | HostRequest::StopRetained { .. }
            | HostRequest::StartClaude { .. }) => restart_alone(&mut engine, protocol, request),
            other => panic!("the automatic engine update sent an unexpected request: {other:?}"),
        }
    }

    /// The one-worker restart (ADR 0161): it stops like Sleep and starts the
    /// worker again on the same conversation.
    fn restart_alone(engine: &mut Engine, protocol: u16, request: HostRequest) -> HostResponse {
        match request {
            HostRequest::Ping => HostResponse::Pong {
                protocol_version: protocol,
            },
            HostRequest::StopRetained { session_id } => {
                engine.retained_stops.push(session_id);
                engine.stopped.push(session_id);
                HostResponse::Acknowledged
            }
            HostRequest::StartClaude { conversation, .. } => {
                engine.started.push(format!("{conversation:?}"));
                let session_id = WorkerSessionId::new();
                engine.restarted_as.push(session_id);
                HostResponse::SessionStarted { session_id }
            }
            other => panic!("not part of a one-worker restart: {other:?}"),
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
        assert!(advance(&hive.state, &HashMap::new()).await);

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
                others: Vec::new(),
            }),
        );
        *pantry.lock().unwrap() = Some(hive.pantry.1);
        hive.store()
            .record_worker_revival_intents(&[hive.ledger.0], 1)
            .unwrap();

        assert!(!advance(&hive.state, &HashMap::new()).await);
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
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.asked("admit_maintenance"), 1);
        hive.rearm();
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.asked("admit_maintenance"), 2);
    }

    /// ADR 0161, from 2026-11-06: with every worker paused the update still
    /// waited, on a turn count the engine lost, a turn ended with Esc, which
    /// sends no report, and a session cron. Each is cleared by a new session;
    /// nothing else may be. ADR 0162: at once, with no minute's wait.
    #[test]
    fn only_a_resting_worker_refused_for_what_a_new_session_clears_is_restarted() {
        for reason in RESTART_CLEARS {
            assert_eq!(
                settle_step(reason, true, false, false),
                Settle::Restart,
                "{reason}: restarted on the first answer"
            );
        }
        // A restart would lose this work, or it is the operator's.
        for reason in [
            "background_work",
            "unsent_input",
            "waiting_on_operator",
            "operator_has_terminal",
            "not_claude",
            "screen_not_resting",
            "sessions_changed",
            "workers_busy",
        ] {
            assert_eq!(
                settle_step(reason, true, false, false),
                Settle::Leave,
                "{reason}"
            );
        }
        assert_eq!(
            settle_step("turn_in_progress", false, false, false),
            Settle::NotResting,
            "Swarm's own reading must say it rests"
        );
        assert_eq!(
            settle_step("turn_in_progress", true, true, false),
            Settle::InUse,
            "never while the operator is using it"
        );
        assert_eq!(
            settle_step("scheduled_work", true, false, true),
            Settle::AlreadyRestarted,
            "once per waiting update"
        );
    }

    /// ADR 0162 end to end: one answer names both workers, nothing is
    /// restarted while a delivery is in flight, and then both are restarted
    /// on the same pass, each alone with the stop Sleep uses and owed a
    /// return like any other.
    #[tokio::test]
    async fn every_resting_worker_the_engine_cannot_vouch_for_is_restarted_on_one_answer() {
        let mut hive = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(|sessions| MaintenanceOutcome::Refused {
                reason: MaintenanceRefusal::TurnCountUnknown,
                session_id: sessions.first().copied(),
                others: sessions[1..]
                    .iter()
                    .map(|&session_id| swarm_terminal::SessionRefusal {
                        session_id,
                        reason: MaintenanceRefusal::ScheduledWork,
                    })
                    .collect(),
            }),
        );
        hive.state.test_start_admission = Some(crate::runtime::CoordinatorStartAdmission::Allowed);
        let resting = reading(&hive, ProviderActivity::Resting);
        let update = &hive.state.idle_engine_update;

        let delivering = hive.state.coordination_delivery.lock().await;
        assert!(!advance(&hive.state, &resting).await);
        assert!(
            hive.engine.lock().unwrap().retained_stops.is_empty(),
            "never while something is being delivered"
        );
        let shown = serde_json::to_value(view(&hive.state, None).unwrap()).unwrap();
        let holders = shown["holders"].as_array().unwrap();
        assert_eq!(holders.len(), 2, "both named on one answer: {holders:?}");
        assert!(holders.iter().all(|holder| holder["swarm"] == "restarts"));
        drop(delivering);

        hive.rearm();
        assert!(!advance(&hive.state, &resting).await);
        assert_restarted_together(&hive);
        assert_eq!(update.refusal(), None, "nothing left holding it");

        // Once no update waits, each is told only when Swarm reads it resting.
        std::fs::write(
            hive.state
                .maintenance_request_path
                .as_ref()
                .unwrap()
                .as_ref(),
            "",
        )
        .unwrap();
        assert!(!advance(&hive.state, &resting).await);
        assert_eq!(
            update.restarted().len(),
            2,
            "not read on their new sessions yet"
        );
    }

    /// Swarm's own reading: Ledger resting, Pantry as given.
    fn reading(hive: &Hive, pantry: ProviderActivity) -> HashMap<WorkerSessionId, ProviderSignals> {
        let screen = |activity| {
            ProviderSignals::from_screen(activity, swarm_terminal::BackgroundWork::default())
        };
        HashMap::from([
            (hive.ledger.1, screen(ProviderActivity::Resting)),
            (hive.pantry.1, screen(pantry)),
        ])
    }

    /// Both were stopped one at a time, like Sleep, outside any admission, and
    /// started again on their own conversations; their returns are settled
    /// and the card says so.
    fn assert_restarted_together(hive: &Hive) {
        let update = &hive.state.idle_engine_update;
        let mut both = vec![hive.ledger.1, hive.pantry.1];
        both.sort_by_key(ToString::to_string);
        let new_sessions = {
            let engine = hive.engine.lock().unwrap();
            let mut stopped = engine.retained_stops.clone();
            stopped.sort_by_key(ToString::to_string);
            assert_eq!(stopped, both, "each alone, on the same pass");
            let mut released = engine.released.clone();
            released.sort_by_key(ToString::to_string);
            assert_eq!(released, both);
            assert_eq!(engine.admitted.len(), 2, "no admission of its own");
            assert_eq!(engine.started.len(), 2);
            assert!(
                engine
                    .started
                    .iter()
                    .all(|start| start.starts_with("Resume")),
                "their own conversations: {:?}",
                engine.started
            );
            engine.restarted_as.clone()
        };
        for worker in [hive.ledger.0, hive.pantry.0] {
            assert_eq!(
                hive.store()
                    .last_session_end_reason(worker)
                    .unwrap()
                    .as_deref(),
                Some("engine_update_restart")
            );
            let profile = hive.store().get_worker_profile(worker).unwrap();
            assert!(
                profile
                    .active_session_id
                    .is_some_and(|session| new_sessions.contains(&session)),
                "back, on a new session"
            );
            assert!(update.restarted_before(worker, worker_engine_build_id()));
        }
        assert!(hive.owed().is_empty(), "their returns are settled");
        let shown = serde_json::to_value(view(&hive.state, None).unwrap()).unwrap();
        let mut restarted = shown["restarted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["worker"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        restarted.sort();
        assert_eq!(restarted, ["Ledger", "Pantry"]);
        assert_eq!(shown["holders"], serde_json::json!([]));
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

        assert!(!advance(&hive.state, &HashMap::new()).await);
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
        assert!(!advance(&hive.state, &HashMap::new()).await);
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

        assert!(!advance(&hive.state, &HashMap::new()).await);
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
        assert!(!advance(&partly.state, &HashMap::new()).await);
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
        assert!(!advance(&partly.state, &HashMap::new()).await);
        assert_eq!(partly.asked("admit_maintenance"), 1, "never asked again");

        // Nobody stopped: nothing owed and nothing recorded.
        let nobody = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(|_| MaintenanceOutcome::Refused {
                reason: MaintenanceRefusal::TurnInProgress,
                session_id: None,
                others: Vec::new(),
            }),
        );
        nobody.lose_answers(false);
        assert!(!advance(&nobody.state, &HashMap::new()).await);
        assert!(nobody.owed().is_empty());
        assert_eq!(nobody.store().last_worker_engine_update().unwrap(), None);
        assert_eq!(nobody.asked("cancel_drain"), 1);
        assert_eq!(nobody.refusal(), Some(("answer_lost", None)));

        // Everybody stopped: the update goes on as if the answer had come.
        let everyone = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        everyone.lose_answers(false);
        assert!(advance(&everyone.state, &HashMap::new()).await);
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
        assert!(!advance(&gone.state, &HashMap::new()).await);
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
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.refusal(), Some(("switched_off", None)));

        let older = Hive::new(17, 0, stop_all());
        assert!(!advance(&older.state, &HashMap::new()).await);
        assert_eq!(older.refusal(), Some(("engine_cannot_check_workers", None)));

        let busy = Hive::new(swarm_terminal::PROTOCOL_VERSION, 1, stop_all());
        assert!(!advance(&busy.state, &HashMap::new()).await);
        assert_eq!(
            busy.state
                .idle_engine_update
                .refusal()
                .map(|refusal| (refusal.reason, refusal.count)),
            Some(("workers_busy", Some(1)))
        );

        let mut unmanaged = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        unmanaged.state.maintenance_request_path = None;
        assert!(!advance(&unmanaged.state, &HashMap::new()).await);
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
        assert!(!advance(&failed_before.state, &HashMap::new()).await);
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

    /// The package timer says a waiting update applies itself only on an
    /// engine this path can stop workers on. Its first such protocol is
    /// written in the script, so it is held here to the engine's own answer.
    #[test]
    fn the_package_timer_names_the_first_engine_this_path_can_stop() {
        let script = include_str!("../../../packaging/linux/swarm-package");
        let first: u16 = script
            .lines()
            .find_map(|line| {
                line.strip_prefix(
                    "maintenance_admission_protocol=${SWARM_MAINTENANCE_ADMISSION_PROTOCOL:-",
                )
            })
            .and_then(|rest| rest.strip_suffix('}'))
            .and_then(|number| number.parse().ok())
            .expect("swarm-package declares the first protocol that admits maintenance");

        assert!(swarm_terminal::supports_maintenance_admission(first));
        assert!(!swarm_terminal::supports_maintenance_admission(first - 1));
    }
}
