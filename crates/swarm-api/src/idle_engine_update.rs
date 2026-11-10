//! A waiting worker engine update applies by itself once every worker rests
//! (ADR 0156).
//!
//! The operator ruled "keep it automatic, but say so when it happens". The
//! card's own path does the work: record who is owed a return, stop them, ask
//! the package layer for the swap, wait for the engine, bring them back. Only
//! the stop differs. The card stops what is running; this asks the engine to
//! stop the exact running set only if every session in it is resting by the
//! engine's own evidence, and to stop none otherwise.
//!
//! The engine names every session it refuses (ADR 0162), and what Swarm does
//! about them is Swarm's (ADR 0163). When every one of them is refused only
//! for what the engine counted, and Swarm's own reading says each rests with
//! nothing typed, Swarm stops those itself and asks the engine again for the
//! rest, on the same pass. The swap restarts every worker anyway, so nobody is
//! restarted first to be checked. When anything else holds the update, Swarm
//! stops nobody and the card says who holds it and why.
//!
//! It never runs before the API listens, nor while swarm-package holds its
//! lifecycle lock, and a drain nothing owns any more is lifted (ADR 0164). An
//! install holds that lock while it waits on this API's health, and the swap
//! needs it, so an update tried during an install waits on the install while
//! the install gives up on this API.
use std::{
    collections::{HashMap, HashSet},
    fs::{File, TryLockError},
    io,
    path::Path,
    sync::{
        Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde::Serialize;
use swarm_domain::{ControlRoomEventKind, WorkerId, WorkerSessionId};
use swarm_persistence::{WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};
use swarm_terminal::{
    HostRequest, HostResponse, HostSessionSummary, MaintenanceOutcome, MaintenanceRefusal,
    ProviderActivity, SessionRefusal, TerminalHostStatus, TerminalSize,
};

use crate::{
    ApiError, AppState, build_version,
    coordination_delivery::{
        BusyPolicy, Cadence, CoordinationMessage, TerminalSubmission, submit_coordination_message,
    },
    maintenance::{host_status_snapshot, worker_engine_update_required},
    provider_activity::{self, ProviderSignals},
    task_store, task_store_error,
    terminal_host::{request_host, terminal_client},
    unix_timestamp, worker_engine_build_id, worker_runtime,
};

/// How often a waiting update asks the engine, at most. Each ask drains the
/// engine for the length of one request. Workers rest together only in the
/// short gaps between deliveries, so the gaps are looked for often.
const ATTEMPT_INTERVAL: Duration = Duration::from_secs(15);

/// The engine's refusals Swarm's own reading may overrule, because each is
/// about what the engine counted in that session rather than what is on its
/// screen: a turn it saw start and not end (Esc sends no report), a count it
/// could not keep, hooks that never reported, a turn that failed, a session
/// cron or background work the last turn report named, or a keystroke since
/// the last prompt. Swarm reads the screen for each of these. Anything else is
/// the operator's, or something Swarm's reading cannot see.
const SWARM_VOUCHES: [&str; 7] = [
    "turn_in_progress",
    "turns_unaccounted",
    "no_startup_evidence",
    "turn_failed",
    "scheduled_work",
    "background_work",
    "unsent_input",
];

/// How long Swarm waits for one session's screen before it reads it as busy.
const REREAD_TIMEOUT: Duration = Duration::from_secs(2);

/// More restarts than one waiting update could make: one per worker.
const RESTARTED_KEPT: usize = 64;

/// The file swarm-package locks, in its state root, for the whole of an
/// install, a reload, an engine swap or a protocol migration (ADR 0164).
pub const PACKAGE_LIFECYCLE_LOCK: &str = ".package-lifecycle.lock";

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
    /// Whether the API listens yet (ADR 0164). Nothing is tried before then.
    listening: AtomicBool,
}

/// A session the engine refused, and what Swarm does about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct EngineHolder {
    /// The worker it belongs to, by name, when Swarm knows it.
    worker: Option<String>,
    reason: &'static str,
    /// What Swarm reads of a refusal it may overrule (ADR 0163): it
    /// `stops_with_update` once nothing else holds the update, or it cannot
    /// because the worker is `in_use`, is `not_resting` by Swarm's own
    /// reading, has text `typed` at its prompt, or was `already_restarted`
    /// for this update. None for a refusal Swarm does not overrule.
    swarm: Option<&'static str>,
    #[serde(skip)]
    session: WorkerSessionId,
}

/// A worker Swarm stopped itself for an update: brought back by the swap, or
/// at once when the update could not apply after all.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Restarted {
    worker: WorkerId,
    name: String,
    at: i64,
    /// The engine that was waiting. A later update may restart it again.
    engine: String,
}

impl IdleEngineUpdate {
    pub(crate) fn note_listening(&self) {
        self.listening.store(true, Ordering::Release);
    }

    fn listening(&self) -> bool {
        self.listening.load(Ordering::Acquire)
    }

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
    /// Workers Swarm stopped itself for this update, though the engine could
    /// not vouch for them (ADR 0163).
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
/// only whether Swarm may overrule the engine's refusal of a worker (ADR 0163).
pub(crate) async fn advance(
    state: &AppState,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> bool {
    // The pass before the API listens is awaited before it can answer an
    // install's health check (ADR 0164).
    if !state.idle_engine_update.listening() {
        return false;
    }
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
    if status.draining && lift_stale_drain(state).await {
        publish_runtime_change(state);
        return true;
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
    let replaced = attempt(state, &status, observed).await;
    drop(lifecycle);
    if replaced {
        state.idle_engine_update.clear();
        publish_runtime_change(state);
    }
    replaced
}

fn publish_runtime_change(state: &AppState) {
    if let Ok(store) = task_store(state)
        && let Err(error) = store.record_control_room_event(ControlRoomEventKind::RuntimeChanged)
    {
        tracing::warn!(message = %error, "the automatic engine update could not publish its runtime event");
    }
    state.control_room_notify.notify_waiters();
}

/// Lifts a drain that nothing still running owns (ADR 0164).
///
/// The engine drains until someone cancels it, and an attempt cancels its own
/// drain only if it lives to finish. An install that restarts this API in the
/// middle of an attempt, and the rollback after a failed install, leave it on,
/// and the engine then refuses every start. A drain is owned while this
/// process holds the worker lifecycle (an attempt or the card), while
/// swarm-package holds its lock (an install, a reload or a swap), or while a
/// swap is asked for and not yet taken. With none of those, nobody is left to
/// cancel it, so Swarm does. Workers owed a return come back once it lifts.
async fn lift_stale_drain(state: &AppState) -> bool {
    let Ok(_lifecycle) = state.worker_lifecycle.try_lock() else {
        return false;
    };
    if !matches!(package_operation_active(state), Ok(false)) {
        return false;
    }
    if state
        .maintenance_request_path
        .as_ref()
        .is_some_and(|path| path.exists())
    {
        return false;
    }
    if !matches!(
        request_host(state, HostRequest::CancelDrain).await,
        Ok(HostResponse::HostStatus { .. })
    ) {
        return false;
    }
    tracing::warn!(
        "the worker engine was left draining by an update that is no longer running; Swarm lifted it so workers can start"
    );
    true
}

/// Whether swarm-package holds its lifecycle lock now (ADR 0164).
///
/// Asked with a shared lock taken and dropped at once, so it never holds an
/// install up. A package step that tries for the lock in that instant defers
/// and runs again on its timer. No lock file means no package step has run.
fn package_operation_active(state: &AppState) -> io::Result<bool> {
    state
        .package_lifecycle_lock_path
        .as_deref()
        .map_or(Ok(false), |path| lock_is_held(path))
}

fn lock_is_held(path: &Path) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// What Swarm reads of one session the engine refused (ADR 0163).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Settle {
    /// A reason Swarm does not overrule: the engine's answer stands.
    Leave,
    /// The operator is using it, and a stop loses what they type.
    InUse,
    /// Swarm's own reading does not say it rests either.
    NotResting,
    /// Text the operator typed sits at its prompt, which a stop would lose.
    Typed,
    /// Swarm stopped it once already for this update, and the update could
    /// not apply then: the button's now.
    AlreadyRestarted,
    /// Swarm reads it resting with nothing typed: it is stopped with the rest
    /// once nothing else holds the update.
    Vouch,
}

impl Settle {
    /// What the card is told Swarm does about it.
    const fn code(self) -> Option<&'static str> {
        match self {
            Self::Leave => None,
            Self::InUse => Some("in_use"),
            Self::NotResting => Some("not_resting"),
            Self::Typed => Some("typed"),
            Self::AlreadyRestarted => Some("already_restarted"),
            Self::Vouch => Some("stops_with_update"),
        }
    }
}

/// Every input is an observation.
fn settle_step(reason: &str, resting: bool, engaged: bool, restarted_before: bool) -> Settle {
    if !SWARM_VOUCHES.contains(&reason) {
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
    Settle::Vouch
}

/// The second look, taken just before any stop: the screen as it is now.
const fn reread_step(resting: bool, typed: bool) -> Settle {
    if !resting {
        Settle::NotResting
    } else if typed {
        Settle::Typed
    } else {
        Settle::Vouch
    }
}

/// Resting at its prompt, by Swarm's own reading, with nothing it can see
/// running: the test provider updates use before typing `/restart` (ADR 0122).
fn resting(signals: Option<&ProviderSignals>) -> bool {
    signals.is_some_and(|signals| {
        signals.activity == ProviderActivity::Resting && !signals.background_work.is_running()
    })
}

/// A worker Swarm stopped itself, outside the engine's admission.
struct SetAside {
    worker: WorkerId,
    name: String,
    session: WorkerSessionId,
}

/// What Swarm did with the sessions the engine's first answer named.
enum Settled<'a> {
    /// It stopped every one, and nothing Swarm sends lands until the guard
    /// drops.
    SetAside(Vec<SetAside>, tokio::sync::MutexGuard<'a, ()>),
    /// A stop failed. These were stopped, or may have been.
    StopFailed(Vec<SetAside>),
    /// The update waits on these, with what Swarm reads of each.
    Held(Vec<EngineHolder>),
}

/// Runs on the engine's first refusal, while the engine drains. Swarm stops
/// the sessions the engine named only when it can vouch for every one of
/// them, because the swap is then one admission away. When even one holds the
/// update for a reason Swarm does not overrule, nobody is stopped: a stop
/// would buy nothing.
async fn settle<'a>(
    state: &'a AppState,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
    holders: Vec<EngineHolder>,
) -> Settled<'a> {
    let Ok(store) = task_store(state) else {
        return Settled::Held(holders);
    };
    let Ok(profiles) = store.list_worker_profiles() else {
        return Settled::Held(holders);
    };
    // Unreadable counts as engaged: a stop while the operator types loses
    // what they typed.
    let engaged = store.engaged_devices_by_worker(unix_timestamp()).ok();
    let engine = worker_engine_build_id();
    let mut verdicts = holders
        .into_iter()
        .map(|holder| {
            let profile = profiles
                .iter()
                .find(|profile| profile.active_session_id == Some(holder.session));
            let step = profile.map_or(Settle::Leave, |profile| {
                settle_step(
                    holder.reason,
                    resting(observed.get(&holder.session)),
                    engaged
                        .as_ref()
                        .is_none_or(|engaged| engaged.contains_key(&profile.id)),
                    state
                        .idle_engine_update
                        .restarted_before(profile.id, engine),
                )
            });
            (holder, profile, step)
        })
        .collect::<Vec<_>>();
    if all_vouched(&verdicts)
        // Nothing Swarm sends may land between this look and the stops. A pass
        // delivering now is a reason to look again at the next one.
        && let Ok(deliveries) = state.coordination_delivery.try_lock()
        // A worker stopped here comes back at once if the update cannot apply
        // after all, so it is stopped only when it could be started again.
        && crate::runtime::coordinator_start_admission(state)
            .await
            .permits_start()
    {
        for (holder, profile, step) in &mut verdicts {
            let Some(profile) = profile else {
                continue;
            };
            *step = if worker_runtime::automation_admitted(state, profile.provider) {
                reread(state, holder.session, profile.provider).await
            } else {
                Settle::Leave
            };
        }
        if all_vouched(&verdicts) {
            let mut stopped = Vec::with_capacity(verdicts.len());
            for (holder, profile, _) in verdicts {
                let Some(profile) = profile else {
                    continue;
                };
                stopped.push(SetAside {
                    worker: profile.id,
                    name: profile.name.clone(),
                    session: holder.session,
                });
                if let Err(error) = set_aside(state, holder.session).await {
                    tracing::warn!(
                        worker = profile.name,
                        message = %error.message,
                        "Swarm could not stop a resting worker for the engine update"
                    );
                    return Settled::StopFailed(stopped);
                }
            }
            return Settled::SetAside(stopped, deliveries);
        }
    }
    Settled::Held(
        verdicts
            .into_iter()
            .map(|(mut holder, _, step)| {
                holder.swarm = step.code();
                holder
            })
            .collect(),
    )
}

/// Whether Swarm vouches for every session the engine named.
fn all_vouched<T>(verdicts: &[(EngineHolder, T, Settle)]) -> bool {
    verdicts.iter().all(|(_, _, step)| *step == Settle::Vouch)
}

/// The screen as it is now, read just before a stop. Unreadable is not
/// resting.
async fn reread(
    state: &AppState,
    session: WorkerSessionId,
    provider: swarm_domain::ProviderKind,
) -> Settle {
    match tokio::time::timeout(
        REREAD_TIMEOUT,
        provider_activity::observe_session_snapshot(state, session, provider),
    )
    .await
    {
        Ok(Some((signals, snapshot))) => reread_step(
            resting(Some(&signals)),
            provider_activity::has_open_provider_input(provider, &snapshot),
        ),
        _ => Settle::NotResting,
    }
}

/// Stops one session with the stop Sleep uses and releases it. Its return was
/// recorded with every other one this attempt made.
async fn set_aside(state: &AppState, session: WorkerSessionId) -> Result<(), ApiError> {
    worker_runtime::stop_worker_session_preserving_context(state, session).await?;
    let store = task_store(state)?;
    store
        .release_worker_session_because(session, Some(("engine_update_restart", "swarm")))
        .map_err(|error| task_store_error(&error))?;
    store
        .release_session_assignments(session)
        .map(|_| ())
        .map_err(|error| task_store_error(&error))
}

/// Remembered once stopped, so each is told its scheduled jobs are gone and is
/// not stopped again for this update if it has to come back.
fn note_set_aside(state: &AppState, stopped: &[SetAside]) {
    let at = unix_timestamp();
    for entry in stopped {
        tracing::info!(
            worker = entry.name,
            "Swarm stopped a resting worker the engine could not vouch for, to apply the engine update"
        );
        state.idle_engine_update.note_restarted(Restarted {
            worker: entry.worker,
            name: entry.name.clone(),
            at,
            engine: worker_engine_build_id().to_owned(),
        });
    }
}

/// The update could not apply after Swarm stopped these, so they come back
/// now rather than wait for a swap that is not coming. The drain is already
/// cancelled, and the caller holds the worker lifecycle.
async fn bring_back(state: &AppState, stopped: &[SetAside]) {
    let Ok(store) = task_store(state) else {
        return;
    };
    for entry in stopped {
        match worker_runtime::start_owed_return_unlocked(
            state,
            store,
            entry.worker,
            TerminalSize::default(),
        )
        .await
        {
            Ok(_) => tracing::info!(
                worker = entry.name,
                "Swarm brought back a worker it stopped for an engine update that could not apply"
            ),
            Err(error) => tracing::warn!(
                worker = entry.name,
                message = %error.message,
                "Swarm could not bring back a worker it stopped for an engine update"
            ),
        }
    }
}

/// Once no update waits, each worker Swarm stopped itself is told, when it
/// next rests, what happened and that its scheduled jobs did not survive.
/// The engine refuses a session with a cron, so these are the workers that
/// had one. Claude ends a session's crons with its process, and Swarm cannot
/// list them.
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
        bytes: b"[Swarm] Swarm restarted this session for a worker engine update, though the \
                 engine could not confirm it was resting. The update has now applied. Your \
                 conversation and your work are as you left them. Scheduled jobs \
                 (CronCreate, /loop) end with a session, so any you had set are gone: set again \
                 the ones you still need. If you were waiting on someone, keep waiting.\r"
            .to_vec(),
        marker: b"Swarm restarted this session".to_vec(),
    }
}

/// Whether a waiting update can ever apply by itself here, whatever the
/// workers are doing now. A member reports it, so its Keeper says "swaps by
/// itself" only where that is true (ADR 0167).
pub(crate) fn applies_itself(state: &AppState, status: &TerminalHostStatus) -> bool {
    standing_refusal(state, status).is_ok()
}

/// What refuses for as long as the setup stays as it is: the switch, the
/// installation, a prepared migration, and an engine that cannot be asked.
fn standing_refusal(
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
    Ok(())
}

/// Everything that refuses before the engine is asked anything.
fn ready(
    state: &AppState,
    status: &TerminalHostStatus,
) -> Result<(), (&'static str, Option<usize>)> {
    standing_refusal(state, status)?;
    let store = task_store(state).map_err(|_| ("store_unavailable", None))?;
    // An install, a reload or a swap holds the lock the swap needs. One that
    // cannot be read is not taken to be free.
    match package_operation_active(state) {
        Ok(false) => {}
        Ok(true) => return Err(("package_operation_active", None)),
        Err(error) => {
            tracing::warn!(message = %error, "the package lifecycle lock could not be read");
            return Err(("package_operation_active", None));
        }
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
///
/// At most two admissions. When the first refuses only sessions Swarm can
/// vouch for, Swarm stops those and asks once more for the rest (ADR 0163).
async fn attempt(
    state: &AppState,
    status: &TerminalHostStatus,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> bool {
    let Ok(client) = terminal_client(state) else {
        return false;
    };
    let Some((mut asked, returns)) = drain_and_record(state).await else {
        return false;
    };
    let mut set_aside = Vec::new();
    // Held from Swarm's last look at the sessions it stopped until the engine
    // answers for the rest, so nothing Swarm sends starts a turn in between.
    let mut deliveries = None;
    loop {
        if asked.is_empty() {
            // Every running session was one Swarm stopped itself.
            return replace_engine(state, status, &returns, &[], &set_aside).await;
        }
        // Asked through the client itself rather than `request_host`, which
        // folds a lost answer and an engine's error into one: an error comes
        // before any stop, and a lost answer may come after some.
        let answer = client
            .request(&HostRequest::AdmitMaintenance {
                sessions: asked.clone(),
            })
            .await;
        drop(deliveries.take());
        let outcome = match answer {
            Ok(HostResponse::Maintenance { outcome }) => outcome,
            // Any other answer, an engine error included, comes before any stop.
            Ok(_) => {
                give_up(state, &returns, &set_aside).await;
                state
                    .idle_engine_update
                    .refuse("engine_unreachable", None, None);
                return false;
            }
            Err(error) => {
                tracing::warn!(%error, "the engine's answer to the automatic engine update was lost");
                return answer_lost(state, status, &returns, &asked, &set_aside).await;
            }
        };
        let (reason, session_id, others) = match outcome {
            MaintenanceOutcome::Refused {
                reason,
                session_id,
                others,
            } => (reason, session_id, others),
            MaintenanceOutcome::Partial { stopped, failed } => {
                stopped_partway(state, status, &returns, &stopped, &set_aside, Some(failed)).await;
                return false;
            }
            MaintenanceOutcome::Stopped { session_ids } => {
                return replace_engine(state, status, &returns, &session_ids, &set_aside).await;
            }
        };
        let holders = named(&returns, reason, session_id, &others);
        // Only the first answer is settled. Once Swarm has stopped anyone, a
        // second refusal ends the attempt and brings them back.
        if set_aside.is_empty() && !holders.is_empty() {
            match settle(state, observed, holders).await {
                Settled::SetAside(stopped, guard) => {
                    note_set_aside(state, &stopped);
                    asked.retain(|session| !stopped.iter().any(|entry| entry.session == *session));
                    set_aside = stopped;
                    deliveries = Some(guard);
                    continue;
                }
                Settled::StopFailed(stopped) => {
                    note_set_aside(state, &stopped);
                    give_up(state, &returns, &stopped).await;
                    state.idle_engine_update.refuse("stop_failed", None, None);
                    return false;
                }
                Settled::Held(holders) => {
                    give_up(state, &returns, &[]).await;
                    state.idle_engine_update.refuse_sessions(holders);
                    return false;
                }
            }
        }
        give_up(state, &returns, &set_aside).await;
        if holders.is_empty() {
            state
                .idle_engine_update
                .refuse(refusal_reason(reason), None, None);
        } else {
            state.idle_engine_update.refuse_sessions(holders);
        }
        return false;
    }
}

/// Lists the running set, drains the engine, and records a return for every
/// worker in it. None, with the reason given, when any of that fails.
async fn drain_and_record(state: &AppState) -> Option<(Vec<WorkerSessionId>, Returns)> {
    let running = match running_sessions(state).await {
        Ok(running) => running,
        Err(error) => {
            tracing::warn!(message = %error.message, "the automatic engine update could not list sessions");
            return None;
        }
    };
    if !matches!(
        request_host(state, HostRequest::BeginDrain).await,
        Ok(HostResponse::HostStatus { .. })
    ) {
        state
            .idle_engine_update
            .refuse("engine_unreachable", None, None);
        return None;
    }
    let returns = match Returns::record(state, &running) {
        Ok(Some(returns)) => returns,
        Ok(None) => {
            cancel_drain(state).await;
            state
                .idle_engine_update
                .refuse("session_without_worker", None, None);
            return None;
        }
        Err(error) => {
            tracing::warn!(message = %error.message, "the automatic engine update could not record who it owes a return");
            cancel_drain(state).await;
            state
                .idle_engine_update
                .refuse("returns_unrecorded", None, None);
            return None;
        }
    };
    let sessions = running.iter().map(|session| session.session_id).collect();
    Some((sessions, returns))
}

/// Every session a refusal named, the first first, by worker.
fn named(
    returns: &Returns,
    reason: MaintenanceRefusal,
    session_id: Option<WorkerSessionId>,
    others: &[SessionRefusal],
) -> Vec<EngineHolder> {
    session_id
        .map(|session| (session, reason))
        .into_iter()
        .chain(others.iter().map(|other| (other.session_id, other.reason)))
        .map(|(session, reason)| EngineHolder {
            worker: returns.name_of(session),
            reason: refusal_reason(reason),
            swarm: None,
            session,
        })
        .collect()
}

/// Nothing more is stopped and the engine is not replaced: the drain ends,
/// whoever still runs owes nothing, and whoever Swarm stopped comes back now.
async fn give_up(state: &AppState, returns: &Returns, set_aside: &[SetAside]) {
    cancel_drain(state).await;
    returns.withdraw(state, |session| {
        !set_aside.iter().any(|entry| entry.session == session)
    });
    bring_back(state, set_aside).await;
    if !set_aside.is_empty() {
        state.control_room_notify.notify_waiters();
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
    set_aside: &[SetAside],
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
        give_up(state, returns, set_aside).await;
        state.idle_engine_update.refuse("answer_lost", None, None);
        false
    } else if stopped.len() == requested.len() {
        replace_engine(state, status, returns, &stopped, set_aside).await
    } else {
        stopped_partway(state, status, returns, &stopped, set_aside, None).await;
        false
    }
}

/// Every session this attempt stopped: the engine's, then Swarm's own.
fn all_stopped(stopped: &[WorkerSessionId], set_aside: &[SetAside]) -> Vec<WorkerSessionId> {
    stopped
        .iter()
        .copied()
        .chain(set_aside.iter().map(|entry| entry.session))
        .collect()
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
    set_aside: &[SetAside],
    failed: Option<WorkerSessionId>,
) {
    cancel_drain(state).await;
    let everyone = all_stopped(stopped, set_aside);
    returns.withdraw(state, |session| !everyone.contains(&session));
    release(state, stopped).await;
    let attempt = begin(state, status, returns, &everyone);
    let names = returns.names(&everyone).join(", ");
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

/// The whole set stopped: the engine's admission stopped `stopped`, and Swarm
/// stopped and released `set_aside` itself. From here this is the card's path.
async fn replace_engine(
    state: &AppState,
    status: &TerminalHostStatus,
    returns: &Returns,
    stopped: &[WorkerSessionId],
    set_aside: &[SetAside],
) -> bool {
    release(state, stopped).await;
    let everyone = all_stopped(stopped, set_aside);
    let attempt = begin(state, status, returns, &everyone);
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
        stopped = everyone.len(),
        stopped_by_swarm = set_aside.len(),
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
        // A write or a stop the engine was making at that instant: gone by the
        // next look, unlike a keystroke left since the last prompt.
        MaintenanceRefusal::InputOrStopInFlight => "input_in_flight",
        MaintenanceRefusal::UnsentInput => "unsent_input",
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
        /// Text typed at a session's prompt. Every screen rests.
        typed: HashMap<WorkerSessionId, &'static str>,
        draining: bool,
        /// Stops as admitted, then drops the connection unanswered.
        lose_answer: bool,
        /// And then stops answering anything at all.
        gone: bool,
        /// When the engine is replaced once the request file appears.
        swap: Swap,
    }

    /// When the fake engine is replaced.
    #[derive(Default)]
    enum Swap {
        /// As soon as the package layer's request file appears.
        #[default]
        OnRequest,
        /// Not yet: the request is written and the swap not taken.
        Pending,
    }

    struct Hive {
        state: AppState,
        engine: Arc<Mutex<Engine>>,
        ledger: (WorkerId, WorkerSessionId),
        pantry: (WorkerId, WorkerSessionId),
        /// swarm-package's lifecycle lock, in the same state root.
        package_lock: std::path::PathBuf,
        request: std::path::PathBuf,
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
            let package_lock = directory.path().join(PACKAGE_LIFECYCLE_LOCK);
            let state = AppState::default()
                .with_terminal_host(crate::HostClient::new(socket), "fixture-only")
                .with_task_store(store)
                .with_maintenance_request_path(request_path.clone())
                .with_package_lifecycle_lock_path(package_lock.clone())
                .with_maintenance_timeout(Duration::from_secs(3));
            state.note_api_listening();
            Self {
                state,
                engine,
                ledger,
                pantry,
                package_lock,
                request: request_path,
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
        let swapped = request_path.exists() && matches!(engine.swap, Swap::OnRequest);
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
            HostRequest::Read { session_id, .. } => second_look(&engine, session_id),
            request @ (HostRequest::Ping
            | HostRequest::StopRetained { .. }
            | HostRequest::StartClaude { .. }) => one_at_a_time(&mut engine, protocol, request),
            other => panic!("the automatic engine update sent an unexpected request: {other:?}"),
        }
    }

    /// Swarm's second look at a session it may stop (ADR 0163).
    fn second_look(engine: &Engine, session_id: WorkerSessionId) -> HostResponse {
        HostResponse::Output {
            session_id,
            resume: swarm_terminal::Resume::Snapshot {
                snapshot: claude_at_rest(engine.typed.get(&session_id).copied().unwrap_or("")),
            },
            running: !engine.stopped.contains(&session_id),
            provider_turn: None,
            last_output_at: None,
        }
    }

    /// Swarm's own stop (ADR 0163): like Sleep, and a start on the same
    /// conversation when the update cannot apply after all.
    fn one_at_a_time(engine: &mut Engine, protocol: u16, request: HostRequest) -> HostResponse {
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
            other => panic!("not part of a stop of Swarm's own: {other:?}"),
        }
    }

    /// A Claude prompt at rest, with `typed` in its composer.
    fn claude_at_rest(typed: &str) -> swarm_terminal::TerminalSnapshot {
        const RULE: &str = "────────────────────────────────────────";
        let mut screen = swarm_terminal::CanonicalTerminalState::new(
            swarm_terminal::JournalLimits::new(64, 64 * 1024),
            TerminalSize::new(24, 100),
        );
        screen.push(
            format!(
                "● Done.\r\n{RULE}\r\n❯ {typed}\r\n{RULE}\r\n  ⏵⏵ auto mode on (shift+tab to cycle)"
            )
            .into_bytes(),
        );
        screen.snapshot()
    }

    fn stop_all() -> Admission {
        Box::new(|sessions| MaintenanceOutcome::Stopped {
            session_ids: sessions.to_vec(),
        })
    }

    /// ADR 0164, from 2026-11-06: a Hive installing a release while every
    /// worker rested never came up. The pass awaited before the API listens
    /// stopped a worker and waited for a swap the install held up, while the
    /// install waited on this API's health. Nothing is tried before it listens.
    #[tokio::test]
    async fn the_engine_update_waits_until_the_api_listens() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        hive.state
            .idle_engine_update
            .listening
            .store(false, Ordering::Release);
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert!(
            hive.engine.lock().unwrap().requests.is_empty(),
            "the engine is not even asked"
        );
        assert!(hive.owed().is_empty());

        hive.state.note_api_listening();
        assert!(
            advance(&hive.state, &HashMap::new()).await,
            "and then it applies"
        );
    }

    /// ADR 0164: an install, a reload or a swap holds swarm-package's lock,
    /// and the swap the update asks for needs it. Nothing is drained or
    /// stopped while it is held, and the update applies once it is let go.
    #[tokio::test]
    async fn nothing_is_stopped_while_a_package_operation_holds_its_lock() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        let install = File::create(&hive.package_lock).unwrap();
        install.lock().unwrap();
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.refusal(), Some(("package_operation_active", None)));
        assert_eq!(hive.asked("begin_drain"), 0);
        assert!(hive.engine.lock().unwrap().stopped.is_empty());
        assert!(hive.owed().is_empty());
        assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);

        drop(install);
        hive.rearm();
        assert!(
            advance(&hive.state, &HashMap::new()).await,
            "applies once it is let go"
        );
    }

    /// ADR 0164, from 2026-11-06: the failed install rolled back and left the
    /// engine draining, and it refused every start until the machine was
    /// restarted. A drain nothing owns any more is lifted. One that an
    /// install, a swap not yet taken, or this process still owns is not.
    #[tokio::test]
    async fn a_drain_nothing_owns_any_more_is_lifted() {
        let hive = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        hive.engine.lock().unwrap().draining = true;

        let install = File::create(&hive.package_lock).unwrap();
        install.lock().unwrap();
        assert!(!advance(&hive.state, &HashMap::new()).await);
        drop(install);

        hive.engine.lock().unwrap().swap = Swap::Pending;
        std::fs::write(&hive.request, "requested_at=0\n").unwrap();
        assert!(!advance(&hive.state, &HashMap::new()).await);
        std::fs::remove_file(&hive.request).unwrap();
        hive.engine.lock().unwrap().swap = Swap::OnRequest;

        let card = hive.state.worker_lifecycle.lock().await;
        assert!(!advance(&hive.state, &HashMap::new()).await);
        drop(card);
        assert_eq!(hive.asked("cancel_drain"), 0, "owned each time");
        assert_eq!(hive.asked("begin_drain"), 0);

        assert!(
            advance(&hive.state, &HashMap::new()).await,
            "workers owed a return may come back"
        );
        assert_eq!(hive.asked("cancel_drain"), 1);
        assert!(!hive.engine.lock().unwrap().draining);
        assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);

        hive.rearm();
        assert!(
            advance(&hive.state, &HashMap::new()).await,
            "the update still waiting applies on a later pass"
        );
        let last = hive.store().last_worker_engine_update().unwrap().unwrap();
        assert_eq!(last.outcome, Some(WorkerEngineUpdateOutcome::Succeeded));
    }

    /// The API looks for the lock swarm-package takes, by the same name in
    /// the same state root as the database (ADR 0164).
    #[test]
    fn the_api_reads_the_lock_swarm_package_takes() {
        let package = include_str!("../../../packaging/linux/swarm-package");
        assert!(package.contains(&format!("exec 7>\"$state_root/{PACKAGE_LIFECYCLE_LOCK}\"")));
        let unit = include_str!("../../../packaging/systemd-user/swarm-api.service.in");
        assert!(unit.contains("SWARM_DATABASE_PATH=@STATE_ROOT@/swarm.sqlite3"));
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

        // Asked again only once the interval has passed.
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.asked("admit_maintenance"), 1);
        hive.rearm();
        assert!(!advance(&hive.state, &HashMap::new()).await);
        assert_eq!(hive.asked("admit_maintenance"), 2);
    }

    /// ADR 0163, from 2026-11-06: the update waited on a lost turn count, a
    /// session cron and stray keystrokes on workers whose screens were empty
    /// and at rest. Swarm overrules what the engine counted, never the
    /// operator, and only when its own reading says the worker rests.
    #[test]
    fn swarm_overrules_only_what_the_engine_counted_and_only_on_its_own_reading() {
        for reason in SWARM_VOUCHES {
            assert_eq!(
                settle_step(reason, true, false, false),
                Settle::Vouch,
                "{reason}"
            );
        }
        for reason in [
            "waiting_on_operator",
            "operator_has_terminal",
            "input_in_flight",
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
            settle_step("unsent_input", true, true, false),
            Settle::InUse,
            "never while the operator is using it"
        );
        assert_eq!(
            settle_step("scheduled_work", true, false, true),
            Settle::AlreadyRestarted,
            "once per waiting update"
        );
        // The look just before the stop.
        assert_eq!(reread_step(true, false), Settle::Vouch);
        assert_eq!(reread_step(true, true), Settle::Typed, "typed text stays");
        assert_eq!(reread_step(false, false), Settle::NotResting);
    }

    /// ADR 0163 end to end, the operator's case: the engine counts a keystroke
    /// on Pantry's empty prompt as unsent. Nothing is stopped while a delivery
    /// is in flight. On the next pass Swarm stops Pantry itself, the engine
    /// stops the rest, and the update applies on that same pass. Nobody is
    /// started before the swap: it brings everyone back once.
    #[tokio::test]
    async fn the_update_applies_on_one_pass_when_swarm_vouches_for_every_worker_the_engine_cannot()
    {
        let pantry = Arc::new(Mutex::new(None));
        let named = Arc::clone(&pantry);
        let mut hive = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(move |sessions| {
                let pantry = named.lock().unwrap().unwrap();
                if sessions.contains(&pantry) {
                    MaintenanceOutcome::Refused {
                        reason: MaintenanceRefusal::UnsentInput,
                        session_id: Some(pantry),
                        others: Vec::new(),
                    }
                } else {
                    MaintenanceOutcome::Stopped {
                        session_ids: sessions.to_vec(),
                    }
                }
            }),
        );
        *pantry.lock().unwrap() = Some(hive.pantry.1);
        hive.state.test_start_admission = Some(crate::runtime::CoordinatorStartAdmission::Allowed);
        let resting = reading(&[hive.ledger.1, hive.pantry.1]);

        let delivering = hive.state.coordination_delivery.lock().await;
        assert!(!advance(&hive.state, &resting).await);
        {
            let engine = hive.engine.lock().unwrap();
            assert!(
                engine.retained_stops.is_empty(),
                "never while something is being delivered"
            );
            assert_eq!(engine.admitted.len(), 1);
        }
        assert!(hive.owed().is_empty(), "nobody stopped, nothing owed");
        let shown = serde_json::to_value(view(&hive.state, None).unwrap()).unwrap();
        assert_eq!(
            shown["holders"],
            serde_json::json!([
                {"worker": "Pantry", "reason": "unsent_input", "swarm": "stops_with_update"},
            ])
        );
        drop(delivering);

        hive.rearm();
        assert!(advance(&hive.state, &resting).await, "applied on this pass");
        {
            let engine = hive.engine.lock().unwrap();
            assert_eq!(
                engine.retained_stops,
                vec![hive.pantry.1],
                "Swarm stopped Pantry"
            );
            assert_eq!(engine.admitted.len(), 3);
            assert_eq!(
                engine.admitted[2],
                vec![hive.ledger.1],
                "the engine stopped the rest"
            );
            let mut released = engine.released.clone();
            released.sort_by_key(ToString::to_string);
            let mut both = vec![hive.ledger.1, hive.pantry.1];
            both.sort_by_key(ToString::to_string);
            assert_eq!(released, both);
            assert!(
                engine.started.is_empty(),
                "nobody is restarted to be checked: {:?}",
                engine.started
            );
        }
        let mut owed_workers = vec![hive.ledger.0, hive.pantry.0];
        owed_workers.sort_by_key(ToString::to_string);
        assert_eq!(hive.owed(), owed_workers, "the swap brings both back");
        let last = hive.store().last_worker_engine_update().unwrap().unwrap();
        assert_eq!(last.outcome, Some(WorkerEngineUpdateOutcome::Succeeded));
        assert_eq!(last.stopped_sessions, Some(2));
        assert_eq!(
            hive.store()
                .last_session_end_reason(hive.pantry.0)
                .unwrap()
                .as_deref(),
            Some("engine_update_restart")
        );
        assert_eq!(
            hive.state
                .idle_engine_update
                .restarted()
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Pantry"],
            "told its scheduled jobs are gone once it rests"
        );
    }

    /// ADR 0163: a stop buys nothing while anything Swarm cannot vouch for
    /// holds the update, so nobody is stopped and the card says who and why.
    #[tokio::test]
    async fn nobody_is_stopped_while_anything_swarm_cannot_vouch_for_holds_the_update() {
        // Pantry is waiting for the operator's answer.
        let mut waiting = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            refuse_both(
                MaintenanceRefusal::TurnCountUnknown,
                MaintenanceRefusal::WaitingOnOperator,
            ),
        );
        waiting.state.test_start_admission =
            Some(crate::runtime::CoordinatorStartAdmission::Allowed);
        let resting = reading(&[waiting.ledger.1, waiting.pantry.1]);
        assert!(!advance(&waiting.state, &resting).await);
        assert_eq!(waiting.asked("read"), 0, "no second look is needed");
        assert_holders(
            &waiting,
            &serde_json::json!([
                {"worker": "Ledger", "reason": "turns_unaccounted", "swarm": "stops_with_update"},
                {"worker": "Pantry", "reason": "waiting_on_operator", "swarm": null},
            ]),
        );

        // Pantry has text typed at its prompt: the second look sees it.
        let mut typed = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            refuse_both(
                MaintenanceRefusal::TurnCountUnknown,
                MaintenanceRefusal::UnsentInput,
            ),
        );
        typed.state.test_start_admission = Some(crate::runtime::CoordinatorStartAdmission::Allowed);
        typed
            .engine
            .lock()
            .unwrap()
            .typed
            .insert(typed.pantry.1, "half a thought");
        let resting = reading(&[typed.ledger.1, typed.pantry.1]);
        assert!(!advance(&typed.state, &resting).await);
        assert_eq!(typed.asked("read"), 2);
        assert_holders(
            &typed,
            &serde_json::json!([
                {"worker": "Ledger", "reason": "turns_unaccounted", "swarm": "stops_with_update"},
                {"worker": "Pantry", "reason": "unsent_input", "swarm": "typed"},
            ]),
        );

        for hive in [&waiting, &typed] {
            assert_eq!(hive.asked("admit_maintenance"), 1);
            assert_eq!(hive.asked("cancel_drain"), 1);
            assert!(hive.engine.lock().unwrap().retained_stops.is_empty());
            assert!(hive.owed().is_empty());
            assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);
        }
    }

    /// ADR 0163: when the engine refuses the rest after Swarm stopped one, the
    /// update cannot apply, so that worker comes back at once on its own
    /// conversation rather than wait for a swap. It is not stopped again for
    /// this update: the button is the way on.
    #[tokio::test]
    async fn a_worker_swarm_stopped_comes_back_at_once_when_the_update_cannot_apply() {
        let ledger = Arc::new(Mutex::new(None));
        let named = Arc::clone(&ledger);
        let mut hive = Hive::new(
            swarm_terminal::PROTOCOL_VERSION,
            0,
            Box::new(move |sessions| {
                let ledger = named.lock().unwrap().unwrap();
                if sessions == [ledger] {
                    // Ledger started a turn in between.
                    return MaintenanceOutcome::Refused {
                        reason: MaintenanceRefusal::TurnInProgress,
                        session_id: Some(ledger),
                        others: Vec::new(),
                    };
                }
                let other = sessions.iter().copied().find(|session| *session != ledger);
                MaintenanceOutcome::Refused {
                    reason: MaintenanceRefusal::ScheduledWork,
                    session_id: other,
                    others: Vec::new(),
                }
            }),
        );
        *ledger.lock().unwrap() = Some(hive.ledger.1);
        hive.state.test_start_admission = Some(crate::runtime::CoordinatorStartAdmission::Allowed);

        assert!(!advance(&hive.state, &reading(&[hive.ledger.1, hive.pantry.1])).await);
        let back_as = {
            let engine = hive.engine.lock().unwrap();
            assert_eq!(engine.retained_stops, vec![hive.pantry.1]);
            assert_eq!(engine.admitted.len(), 2);
            assert_eq!(engine.started.len(), 1, "Pantry is back at once");
            assert!(
                engine.started[0].starts_with("Resume"),
                "{:?}",
                engine.started
            );
            engine.restarted_as[0]
        };
        let profile = hive.store().get_worker_profile(hive.pantry.0).unwrap();
        assert_eq!(profile.active_session_id, Some(back_as));
        assert!(
            hive.owed().is_empty(),
            "its return is settled, Ledger owes none"
        );
        assert_eq!(hive.asked("cancel_drain"), 1);
        assert_eq!(hive.store().last_worker_engine_update().unwrap(), None);
        assert_eq!(
            hive.refusal(),
            Some(("turn_in_progress", Some("Ledger".to_owned())))
        );

        // Refused again on its new session: Swarm leaves it this time.
        hive.rearm();
        assert!(!advance(&hive.state, &reading(&[hive.ledger.1, back_as])).await);
        assert_eq!(hive.engine.lock().unwrap().retained_stops.len(), 1);
        assert_holders(
            &hive,
            &serde_json::json!([
                {"worker": "Pantry", "reason": "scheduled_work", "swarm": "already_restarted"},
            ]),
        );
        let shown = serde_json::to_value(view(&hive.state, None).unwrap()).unwrap();
        assert_eq!(shown["restarted"][0]["worker"], "Pantry");
    }

    /// The engine refuses Ledger for `ledger` and Pantry for `pantry`.
    fn refuse_both(ledger: MaintenanceRefusal, pantry: MaintenanceRefusal) -> Admission {
        Box::new(move |sessions| MaintenanceOutcome::Refused {
            reason: ledger,
            session_id: sessions.first().copied(),
            others: vec![swarm_terminal::SessionRefusal {
                session_id: sessions[1],
                reason: pantry,
            }],
        })
    }

    /// The holders the card shows, Ledger first.
    fn assert_holders(hive: &Hive, expected: &serde_json::Value) {
        let shown = serde_json::to_value(view(&hive.state, None).unwrap()).unwrap();
        assert_eq!(&shown["holders"], expected);
    }

    /// Swarm's own reading of these sessions: each resting at its prompt.
    fn reading(sessions: &[WorkerSessionId]) -> HashMap<WorkerSessionId, ProviderSignals> {
        sessions
            .iter()
            .map(|&session| {
                (
                    session,
                    ProviderSignals::from_screen(
                        ProviderActivity::Resting,
                        swarm_terminal::BackgroundWork::default(),
                    ),
                )
            })
            .collect()
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

    /// ADR 0167: what a member tells its Keeper. A busy worker only delays an
    /// update that applies itself; an engine on protocol 17 or an install
    /// nothing manages never applies one.
    #[tokio::test]
    async fn a_member_says_its_update_applies_itself_only_where_it_can() {
        let busy = Hive::new(swarm_terminal::PROTOCOL_VERSION, 1, stop_all());
        let status = crate::maintenance::host_status_snapshot(&busy.state)
            .await
            .unwrap();
        assert!(
            applies_itself(&busy.state, &status),
            "a busy worker only delays it"
        );

        let older = Hive::new(17, 0, stop_all());
        let status = crate::maintenance::host_status_snapshot(&older.state)
            .await
            .unwrap();
        assert!(!applies_itself(&older.state, &status));

        let mut unmanaged = Hive::new(swarm_terminal::PROTOCOL_VERSION, 0, stop_all());
        unmanaged.state.maintenance_request_path = None;
        let status = crate::maintenance::host_status_snapshot(&unmanaged.state)
            .await
            .unwrap();
        assert!(!applies_itself(&unmanaged.state, &status));
    }
}
