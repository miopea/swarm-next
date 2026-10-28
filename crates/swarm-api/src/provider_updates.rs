//! Moving Claude workers onto a newly installed release in place (ADR 0122).
//!
//! Claude updates itself on disk, and a running worker keeps executing the
//! release it started with. Its `/restart` command re-executes the same process
//! onto the installed release and reopens the same conversation, so a worker can
//! be moved onto an update without being stopped. This pass types it into each
//! worker that is behind, one at a time as each one comes to rest, and tells a
//! worker that had work in hand to carry on once it is back.
//!
//! Measured 2026-10-23 against Claude 2.1.309 before any of this was written:
//! the process id stayed the same, the kernel's executable moved to 2.1.310,
//! and the process came back as `--resume` of the same conversation with the
//! same `--mcp-config`, `--strict-mcp-config` and `--settings`. Claude refuses
//! the command while background work is running, while Remote Control is
//! connected, and for a session it has not saved.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_terminal::{HostRequest, HostResponse, ProviderActivity, ProviderRelease};

use crate::{
    ApiError, AppState, authorize,
    coordination_delivery::{
        BusyPolicy, Cadence, CoordinationMessage, TerminalSubmission, submit_coordination_message,
    },
    provider_activity::ProviderSignals,
    task_store, task_store_error,
    terminal_host::request_host,
    unix_timestamp,
};

/// A `/restart` Swarm has typed into one session, and what is still owed for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UpdateRequest {
    /// The release file the session was asked onto.
    target: String,
    /// The version that file names, for the message after it.
    version: Option<String>,
    /// Whether the worker held Active work when it was asked.
    carry_on: bool,
}

/// At most one request per session, kept only while that session is live.
///
/// In memory on purpose. An API restart forgets them, which costs at most one
/// more `/restart` for a worker Claude refused, or a carry-on message never
/// sent to a worker that is resting anyway. Neither is worth a table.
#[derive(Debug, Default)]
pub(super) struct ProviderUpdates {
    requests: Mutex<HashMap<WorkerSessionId, UpdateRequest>>,
    /// One pass at a time. The supervisor and the Restart button both run it,
    /// and two passes together could type `/restart` twice into one worker.
    pass: tokio::sync::Mutex<()>,
    /// The release a worker answered "Unknown command" for (ADR 0125). Claude
    /// switches the command on or off for the account, so one answer stands
    /// for every worker behind that release. A newer release is a new question.
    unknown_on: Mutex<Option<String>>,
}

impl ProviderUpdates {
    fn requests(&self) -> std::sync::MutexGuard<'_, HashMap<WorkerSessionId, UpdateRequest>> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The sessions already asked onto `release` that are not on it yet.
    pub(super) fn asked_onto(&self, release: &ProviderRelease) -> Vec<WorkerSessionId> {
        self.requests()
            .iter()
            .filter(|(_, request)| request.target == release.resolved_path)
            .map(|(session, _)| *session)
            .collect()
    }

    /// Whether workers behind `release` can be moved onto it with `/restart`
    /// (ADR 0125). Claude's cached flag decides when Swarm can read it. When it
    /// cannot, a worker's answer that Claude has no such command stands in for
    /// it. Otherwise they take the stop-and-start every other provider gets.
    pub(super) fn moves_in_place(&self, release: &ProviderRelease, restart: Option<bool>) -> bool {
        restart.unwrap_or_else(|| {
            self.unknown_on
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_deref()
                != Some(release.resolved_path.as_str())
        })
    }

    /// Whether Swarm moves this session onto `release` itself (ADR 0122, 0125):
    /// a Claude worker known to be running an older release, while Claude's
    /// `/restart` can run. The Updates view and the Restart button both ask
    /// this, so the worker the notice says is moving is the one the button
    /// leaves running.
    pub(super) fn restarts_in_place(
        &self,
        provider: ProviderKind,
        release: Option<&ProviderRelease>,
        restart: Option<bool>,
        running: Option<&str>,
    ) -> bool {
        provider == ProviderKind::ClaudeCode
            && release.is_some_and(|release| {
                self.moves_in_place(release, restart)
                    && running.and_then(|running| {
                        swarm_terminal::running_release_current(release, running)
                    }) == Some(false)
            })
    }

    /// Lets these sessions be asked again: the Restart button.
    pub(super) fn ask_again(&self, sessions: &[WorkerSessionId]) {
        let mut requests = self.requests();
        for session in sessions {
            requests.remove(session);
        }
    }
}

/// What one session needs from this pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UpdateStep {
    /// Nothing now.
    Wait,
    /// Type `/restart`.
    Restart,
    /// It is on the installed release and had work in hand: tell it to carry on.
    CarryOn,
    /// The request is finished with.
    Forget,
}

/// Decides one session's step from what is known about it now.
///
/// Every input is an observation; nothing here waits for time to pass.
#[must_use]
pub(super) fn update_step(
    release: &ProviderRelease,
    executable: Option<&str>,
    signals: Option<&ProviderSignals>,
    request: Option<&UpdateRequest>,
) -> UpdateStep {
    // No executable, or one that cannot be compared, is no evidence: such a
    // session is left to the stop-and-start path it has always had, because a
    // `/restart` that cannot be confirmed would be asked for again forever.
    let current =
        executable.and_then(|running| swarm_terminal::running_release_current(release, running));
    let at_rest = at_rest(signals);
    match (request, current) {
        (Some(request), Some(true)) if request.carry_on => UpdateStep::CarryOn,
        (Some(_), Some(true)) => UpdateStep::Forget,
        // ONE ASK PER RELEASE. Still behind after it means Claude refused, most
        // likely over Remote Control or background work. Typing it again every
        // time the worker rests would be a loop; the Restart button asks again.
        (Some(request), _) if request.target == release.resolved_path => UpdateStep::Wait,
        (_, Some(false)) if at_rest => UpdateStep::Restart,
        // Asked onto a release that has itself been replaced since.
        (Some(_), _) => UpdateStep::Forget,
        (None, _) => UpdateStep::Wait,
    }
}

/// At rest with nothing running in the background: the only time `/restart` is
/// typed.
fn at_rest(signals: Option<&ProviderSignals>) -> bool {
    signals.is_some_and(|signals| {
        signals.activity == ProviderActivity::Resting && !signals.background_work.is_running()
    })
}

/// The release `claude` resolves to on the terminal host's `PATH` now, and
/// whether its `/restart` is switched on (ADR 0125).
pub(super) async fn installed_claude(state: &AppState) -> Option<(ProviderRelease, Option<bool>)> {
    match request_host(state, HostRequest::ProviderCapabilities).await {
        Ok(HostResponse::ProviderCapabilities {
            claude_release,
            claude_restart,
            ..
        }) => claude_release.map(|release| (release, claude_restart)),
        _ => None,
    }
}

/// Whether a worker behind `release` shows Claude's answer that it has no
/// `/restart` (ADR 0125).
///
/// The backstop for the cached flag, which can be missing or renamed. It reads
/// the workers already asked, and the ones at rest that this pass would ask:
/// an API restart forgets every ask, and a worker still showing the answer to
/// the last one must not be typed into again to learn it twice.
async fn claude_answered_unknown(
    state: &AppState,
    release: &ProviderRelease,
    executables: &HashMap<WorkerSessionId, Option<String>>,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> bool {
    let asked = state.provider_updates.asked_onto(release);
    for (&session, running) in executables {
        let behind = running
            .as_deref()
            .and_then(|running| swarm_terminal::running_release_current(release, running))
            == Some(false);
        if !behind || !(asked.contains(&session) || at_rest(observed.get(&session))) {
            continue;
        }
        if let Ok(HostResponse::Output {
            session_id,
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            ..
        }) = request_host(
            state,
            HostRequest::Read {
                session_id: session,
                after_sequence: None,
            },
        )
        .await
            && session_id == session
            && swarm_terminal::claude_restart_unknown(&snapshot)
        {
            return true;
        }
    }
    false
}

/// The executable each live terminal is running, as the host reports it.
///
/// `None` when the host could not be asked. A host older than the field
/// reports every session without one, which leaves every worker on the
/// stop-and-start path.
pub(super) async fn running_executables(
    state: &AppState,
) -> Option<HashMap<WorkerSessionId, Option<String>>> {
    match request_host(state, HostRequest::ListSessions).await {
        Ok(HostResponse::Sessions { sessions }) => Some(
            sessions
                .into_iter()
                .map(|session| (session.session_id, session.provider_executable))
                .collect(),
        ),
        _ => None,
    }
}

/// Typed exactly as the operator would. Claude runs a command only when the
/// message begins with it, which is why an operator broadcast of `/restart`
/// arrived everywhere as plain text: it begins with the broadcast header.
fn restart_message() -> CoordinationMessage {
    CoordinationMessage {
        cadence: Cadence::Immediate,
        busy: BusyPolicy::Wait,
        bytes: b"/restart\r".to_vec(),
        marker: b"/restart".to_vec(),
    }
}

/// Sent once the worker is on the installed release, and only to one that had
/// Active work. It says what happened and asks for nothing new, so a worker
/// that was waiting on someone keeps waiting.
fn carry_on_message(version: Option<&str>) -> CoordinationMessage {
    let onto = version.map_or_else(
        || "the installed Claude Code".to_owned(),
        |version| format!("Claude Code {version}"),
    );
    CoordinationMessage {
        cadence: Cadence::Immediate,
        busy: BusyPolicy::Wait,
        bytes: format!(
            "[Swarm] This session was restarted onto {onto} to pick up an update. Your \
             conversation and your work are as you left them: carry on where you left off, and \
             if you were waiting on someone, keep waiting.\r"
        )
        .into_bytes(),
        marker: format!("restarted onto {onto}").into_bytes(),
    }
}

/// The installed Claude release, and whether this pass may type `/restart`
/// onto it (ADR 0125). While Claude's flag cannot be read, a worker's answer
/// that Claude has no such command is recorded for the release here, so the
/// Updates view and the Restart button read it too.
///
/// A flag that can be read is never overruled by the screen. Measured
/// 2026-10-27: a second after each API start Swarm marked 2.1.312 from an
/// answer already on a worker's screen, and the mark held after the flag
/// switched on.
async fn restart_target(
    state: &AppState,
    executables: &HashMap<WorkerSessionId, Option<String>>,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> Option<(ProviderRelease, bool)> {
    let (release, restart) = installed_claude(state).await?;
    if let Some(on) = restart {
        return Some((release, on));
    }
    if !state.provider_updates.moves_in_place(&release, None) {
        return Some((release, false));
    }
    if !claude_answered_unknown(state, &release, executables, observed).await {
        return Some((release, true));
    }
    tracing::warn!(
        version = release.version.as_deref().unwrap_or("unknown"),
        "Claude answered /restart as an unknown command; workers behind this release need stopping and starting"
    );
    *state
        .provider_updates
        .unknown_on
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(release.resolved_path.clone());
    Some((release, false))
}

/// One pass over every live Claude worker. Returns how many were restarted.
pub(super) async fn advance(
    state: &AppState,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> usize {
    let _pass = state.provider_updates.pass.lock().await;
    let (Some(client), Ok(store)) = (state.terminal_host.as_ref(), task_store(state)) else {
        return 0;
    };
    let Some(executables) = running_executables(state).await else {
        return 0;
    };
    let Some((release, in_place)) = restart_target(state, &executables, observed).await else {
        return 0;
    };
    let Ok(sessions) = store.active_worker_sessions() else {
        return 0;
    };
    let claude = sessions
        .into_iter()
        .filter(|session| session.provider == ProviderKind::ClaudeCode)
        .collect::<Vec<_>>();
    let steps = {
        let mut requests = state.provider_updates.requests();
        requests.retain(|session, _| claude.iter().any(|live| live.session_id == *session));
        let mut steps = Vec::new();
        for session in &claude {
            let request = requests.get(&session.session_id).cloned();
            match update_step(
                &release,
                executables
                    .get(&session.session_id)
                    .and_then(Option::as_deref),
                observed.get(&session.session_id),
                request.as_ref(),
            ) {
                UpdateStep::Wait => {}
                UpdateStep::Restart if !in_place => {}
                UpdateStep::Forget => {
                    requests.remove(&session.session_id);
                }
                step => steps.push((*session, step, request)),
            }
        }
        steps
    };
    let mut restarted = 0;
    for (session, step, request) in steps {
        if step == UpdateStep::Restart {
            let carry_on = store
                .worker_has_active_task(session.worker_id)
                .unwrap_or(false);
            let outcome =
                submit_coordination_message(store, client, session.session_id, restart_message())
                    .await;
            match outcome {
                // Not written. Asked again when the worker next rests.
                Ok(TerminalSubmission::Deferred(_)) => {}
                outcome => {
                    // Written, or possibly written: either way this release has
                    // had its one ask. A refusal or an unreadable outcome is
                    // reported rather than retried into a loop.
                    if matches!(outcome, Ok(TerminalSubmission::Acknowledged)) {
                        restarted += 1;
                        tracing::info!(
                            worker_id = %session.worker_id,
                            version = release.version.as_deref().unwrap_or("unknown"),
                            "asked a Claude worker to restart onto the installed release"
                        );
                    } else {
                        tracing::warn!(
                            worker_id = %session.worker_id,
                            outcome = ?outcome,
                            "a Claude worker's restart could not be confirmed as sent"
                        );
                    }
                    state.provider_updates.requests().insert(
                        session.session_id,
                        UpdateRequest {
                            target: release.resolved_path.clone(),
                            version: release.version.clone(),
                            carry_on,
                        },
                    );
                }
            }
        } else if step == UpdateStep::CarryOn {
            let version = request.and_then(|request| request.version);
            let outcome = submit_coordination_message(
                store,
                client,
                session.session_id,
                carry_on_message(version.as_deref()),
            )
            .await;
            // A deferral keeps the request, so the message goes when the worker
            // can take it. Anything else is final: an uncertain write may have
            // landed, and sending it twice is worse than once.
            if !matches!(outcome, Ok(TerminalSubmission::Deferred(_))) {
                state
                    .provider_updates
                    .requests()
                    .remove(&session.session_id);
            }
        }
    }
    if restarted > 0 {
        state.control_room_notify.notify_waiters();
    }
    restarted
}

/// Whether this Hive's Claude workers connect to Remote Control.
#[derive(Serialize)]
struct WorkerRemoteControlView {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetWorkerRemoteControlRequest {
    enabled: bool,
}

pub(super) async fn worker_remote_control(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let enabled = task_store(&state)?
        .worker_remote_control()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerRemoteControlView { enabled }),
    )
        .into_response())
}

pub(super) async fn set_worker_remote_control(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetWorkerRemoteControlRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let enabled = task_store(&state)?
        .set_worker_remote_control(request.enabled, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(WorkerRemoteControlView { enabled }),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use swarm_terminal::BackgroundWork;

    use super::*;

    fn release() -> ProviderRelease {
        ProviderRelease {
            resolved_path: "/home/op/.local/share/claude/versions/2.1.310".into(),
            version: Some("2.1.310".into()),
            installed_at: Some(1_000),
        }
    }

    const OLDER: &str = "/home/op/.local/share/claude/versions/2.1.309";
    const INSTALLED: &str = "/home/op/.local/share/claude/versions/2.1.310";

    fn signals(activity: ProviderActivity, shells: u16) -> ProviderSignals {
        ProviderSignals {
            activity,
            background_work: BackgroundWork {
                shells,
                monitors: 0,
                workflow: false,
                others: 0,
            },
            turn_failure: None,
            denial: None,
        }
    }

    fn asked(target: &str, carry_on: bool) -> UpdateRequest {
        UpdateRequest {
            target: target.into(),
            version: Some("2.1.310".into()),
            carry_on,
        }
    }

    /// The operator's rule: only a worker with an update, only when it is at
    /// rest with no shells running.
    #[test]
    fn a_worker_behind_the_installed_release_is_restarted_only_at_rest() {
        let release = release();
        let step = |executable, signals: Option<&ProviderSignals>| {
            update_step(&release, executable, signals, None)
        };

        assert_eq!(
            step(Some(OLDER), Some(&signals(ProviderActivity::Resting, 0))),
            UpdateStep::Restart
        );
        assert_eq!(
            step(Some(OLDER), Some(&signals(ProviderActivity::Active, 0))),
            UpdateStep::Wait
        );
        assert_eq!(
            step(Some(OLDER), Some(&signals(ProviderActivity::Resting, 2))),
            UpdateStep::Wait
        );
        assert_eq!(
            step(
                Some(OLDER),
                Some(&signals(ProviderActivity::AwaitingOperator, 0))
            ),
            UpdateStep::Wait
        );
        assert_eq!(step(Some(OLDER), None), UpdateStep::Wait);
        assert_eq!(
            step(
                Some(INSTALLED),
                Some(&signals(ProviderActivity::Resting, 0))
            ),
            UpdateStep::Wait
        );
    }

    /// With no comparable executable a restart could never be confirmed, so it
    /// would be asked for again on every rest.
    #[test]
    fn a_session_whose_release_cannot_be_read_is_never_restarted_in_place() {
        let release = release();
        let resting = Some(&signals(ProviderActivity::Resting, 0));

        assert_eq!(update_step(&release, None, resting, None), UpdateStep::Wait);
        assert_eq!(
            update_step(&release, Some("/usr/bin/bash"), resting, None),
            UpdateStep::Wait
        );
    }

    #[test]
    fn a_release_is_asked_for_once_and_then_left_to_the_operator() {
        let release = release();
        let request = asked(INSTALLED, true);

        assert_eq!(
            update_step(
                &release,
                Some(OLDER),
                Some(&signals(ProviderActivity::Resting, 0)),
                Some(&request)
            ),
            UpdateStep::Wait
        );
    }

    #[test]
    fn an_arrival_tells_a_worker_with_work_in_hand_to_carry_on() {
        let release = release();
        let resting = Some(&signals(ProviderActivity::Resting, 0));

        assert_eq!(
            update_step(
                &release,
                Some(INSTALLED),
                resting,
                Some(&asked(INSTALLED, true))
            ),
            UpdateStep::CarryOn
        );
        assert_eq!(
            update_step(
                &release,
                Some(INSTALLED),
                resting,
                Some(&asked(INSTALLED, false))
            ),
            UpdateStep::Forget
        );
    }

    /// A newer Claude landing while a worker still holds an ask for the last
    /// one is a new release, and gets its own ask.
    #[test]
    fn an_ask_for_a_replaced_release_gives_way_to_the_new_one() {
        let release = release();
        let stale = asked(OLDER, false);

        assert_eq!(
            update_step(
                &release,
                Some(OLDER),
                Some(&signals(ProviderActivity::Resting, 0)),
                Some(&stale)
            ),
            UpdateStep::Restart
        );
        assert_eq!(
            update_step(
                &release,
                Some(OLDER),
                Some(&signals(ProviderActivity::Active, 0)),
                Some(&stale)
            ),
            UpdateStep::Forget
        );
    }

    #[test]
    fn the_restart_is_typed_as_a_command_and_the_carry_on_names_the_release() {
        let restart = restart_message();
        assert_eq!(restart.bytes, b"/restart\r");
        assert_eq!(restart.busy, BusyPolicy::Wait);

        let carry_on = carry_on_message(Some("2.1.310"));
        let text = String::from_utf8(carry_on.bytes).unwrap();
        assert!(text.starts_with("[Swarm] This session was restarted onto Claude Code 2.1.310"));
        assert!(text.ends_with("keep waiting.\r"));
        assert!(text.contains(std::str::from_utf8(&carry_on.marker).unwrap()));
    }

    /// How the fake Claude answers `/restart`.
    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum Reply {
        /// Moves onto the installed release.
        Moves,
        /// Refuses, as it does with Remote Control connected.
        Refuses,
        /// Has no such command, as while its flag is off (ADR 0125).
        Unknown,
        /// Moves, though the screen still shows the answer Claude gave while
        /// its flag was off.
        MovesPastAnOldAnswer,
    }

    #[cfg(unix)]
    impl Reply {
        /// What the worker's screen shows before Swarm asks it anything.
        fn screen_before(self) -> &'static str {
            match self {
                Self::MovesPastAnOldAnswer => "● Unknown command: /restart\r\n● Done.\r\n",
                Self::Moves | Self::Refuses | Self::Unknown => "● Done.\r\n",
            }
        }
    }

    /// A fake terminal host holding one Claude session, which answers
    /// `/restart` with `reply`, and reports `restart` as Claude's cached flag.
    #[cfg(unix)]
    fn fake_host(
        session: WorkerSessionId,
        reply: Reply,
        restart: Option<bool>,
    ) -> (
        tempfile::TempDir,
        crate::HostClient,
        Arc<Mutex<Vec<String>>>,
    ) {
        use std::fmt::Write as _;

        use swarm_terminal::{CanonicalTerminalState, JournalLimits, TerminalSize};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let submitted = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorder = submitted.clone();
        tokio::spawn(async move {
            let mut terminal = CanonicalTerminalState::new(
                JournalLimits::new(64 * 1024, 64),
                TerminalSize::new(30, 240),
            );
            let mut transcript = String::from(reply.screen_before());
            let mut typed = String::new();
            let mut running = OLDER.to_owned();
            let redraw = |terminal: &mut CanonicalTerminalState, transcript: &str, typed: &str| {
                terminal.push(
                    format!("\u{1b}[2J\u{1b}[H{transcript}\r\n❯ {typed}\r\n  ? for shortcuts")
                        .into_bytes(),
                );
            };
            redraw(&mut terminal, &transcript, &typed);
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
                let response = match request {
                    HostRequest::ProviderCapabilities => HostResponse::ProviderCapabilities {
                        claude_code: true,
                        codex: false,
                        experimental: None,
                        claude_release: Some(release()),
                        codex_release: None,
                        claude_restart: restart,
                    },
                    HostRequest::ListSessions => HostResponse::Sessions {
                        sessions: vec![swarm_terminal::HostSessionSummary {
                            session_id: session,
                            running: true,
                            stop_pending_release: false,
                            resources: None,
                            last_output_at: None,
                            recovery_attempt: None,
                            provider_start: None,
                            provider_selection: None,
                            continuation_unavailable: false,
                            continuation_recovery: None,
                            provider_executable: Some(running.clone()),
                            provider_tools_relay: None,
                        }],
                    },
                    HostRequest::Read { .. } => HostResponse::Output {
                        session_id: session,
                        running: true,
                        resume: swarm_terminal::Resume::Snapshot {
                            snapshot: terminal.snapshot(),
                        },
                        provider_turn: None,
                        last_output_at: None,
                    },
                    HostRequest::Write { bytes, .. } => {
                        if bytes == b"\r" {
                            let _ = write!(transcript, "\r\n❯ {typed}\r\n");
                            if typed == "/restart" {
                                match reply {
                                    Reply::Moves | Reply::MovesPastAnOldAnswer => {
                                        running = INSTALLED.to_owned();
                                        transcript.push_str("  ⎿  Successfully updated from 2.1.309 to version 2.1.310\r\n");
                                    }
                                    Reply::Refuses => transcript.push_str(
                                        "  ⎿  Can't restart while Remote Control is connected.\r\n",
                                    ),
                                    Reply::Unknown => {
                                        transcript.push_str("\r\n● Unknown command: /restart\r\n");
                                    }
                                }
                            }
                            recorder.lock().unwrap().push(std::mem::take(&mut typed));
                        } else {
                            typed.push_str(&String::from_utf8(bytes).unwrap());
                        }
                        redraw(&mut terminal, &transcript, &typed);
                        HostResponse::Acknowledged
                    }
                    other => panic!("the update pass sent an unexpected request: {other:?}"),
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                let _ = reader.get_mut().write_all(&bytes).await;
            }
        });
        (directory, crate::HostClient::new(socket), submitted)
    }

    #[cfg(unix)]
    fn hive_with_working_worker(
        client: crate::HostClient,
        session: WorkerSessionId,
    ) -> (AppState, HashMap<WorkerSessionId, ProviderSignals>) {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Fix the build", "/workspace/petal")
            .unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Active)
            .unwrap();
        let state = AppState::default()
            .with_terminal_host(client, "fixture-only")
            .with_task_store(store);
        let observed = HashMap::from([(session, signals(ProviderActivity::Resting, 0))]);
        (state, observed)
    }

    /// The whole slice against a terminal: a resting worker on an older Claude
    /// is sent `/restart`, comes back on the installed release, is told once to
    /// carry on because it had Active work, and is then left alone.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_resting_worker_is_restarted_onto_the_update_and_told_to_carry_on() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) = fake_host(session, Reply::Moves, None);
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(*submitted.lock().unwrap(), vec!["/restart".to_owned()]);

        assert_eq!(advance(&state, &observed).await, 0);
        {
            let sent = submitted.lock().unwrap();
            assert_eq!(sent.len(), 2);
            assert!(
                sent[1].starts_with("[Swarm] This session was restarted onto Claude Code 2.1.310")
            );
        }

        assert_eq!(advance(&state, &observed).await, 0);
        assert_eq!(
            submitted.lock().unwrap().len(),
            2,
            "nothing more once it has arrived"
        );
    }

    /// Claude refused, so the worker is still behind. It is not asked again on
    /// every rest; the Restart button is what asks again.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_refused_restart_is_not_repeated_until_the_operator_asks() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) = fake_host(session, Reply::Refuses, None);
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(advance(&state, &observed).await, 0);
        assert_eq!(*submitted.lock().unwrap(), vec!["/restart".to_owned()]);
        assert_eq!(state.provider_updates.asked_onto(&release()), vec![session]);

        state.provider_updates.ask_again(&[session]);
        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(submitted.lock().unwrap().len(), 2);
    }

    /// Measured 2026-10-26: Claude's flag was off, and every worker Swarm asked
    /// onto 2.1.311 answered "Unknown command: /restart", twice. With the flag
    /// read first, nothing is typed at all.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_switched_off_restart_is_never_typed() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) = fake_host(session, Reply::Unknown, Some(false));
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 0);
        state.provider_updates.ask_again(&[session]);
        assert_eq!(advance(&state, &observed).await, 0);
        assert!(submitted.lock().unwrap().is_empty());
        assert!(
            !state
                .provider_updates
                .moves_in_place(&release(), Some(false))
        );
    }

    /// The backstop for a flag Swarm cannot read: Claude's own answer stops the
    /// asking for that release, and the button no longer asks again in place.
    #[cfg(unix)]
    #[tokio::test]
    async fn claudes_unknown_command_answer_ends_in_place_updates_for_the_release() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) = fake_host(session, Reply::Unknown, None);
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(advance(&state, &observed).await, 0);
        assert!(!state.provider_updates.moves_in_place(&release(), None));

        state.provider_updates.ask_again(&[session]);
        assert_eq!(advance(&state, &observed).await, 0);
        assert_eq!(*submitted.lock().unwrap(), vec!["/restart".to_owned()]);

        // A newer release is asked about afresh.
        let newer = ProviderRelease {
            resolved_path: "/home/op/.local/share/claude/versions/2.1.311".into(),
            version: Some("2.1.311".into()),
            installed_at: Some(2_000),
        };
        assert!(state.provider_updates.moves_in_place(&newer, None));

        // And the flag, once Swarm can read it again, outranks the answer.
        assert!(
            state
                .provider_updates
                .moves_in_place(&release(), Some(true))
        );
    }

    /// Measured 2026-10-27: a second after each API start Swarm marked 2.1.312
    /// from an answer already on a worker's screen, and the mark held after
    /// Claude's flag switched on. A flag Swarm can read is never overruled by
    /// what a screen still shows.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_flag_that_reads_on_outranks_an_old_answer_on_screen() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) =
            fake_host(session, Reply::MovesPastAnOldAnswer, Some(true));
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(submitted.lock().unwrap()[0], "/restart");
        assert!(
            state
                .provider_updates
                .moves_in_place(&release(), Some(true))
        );
    }

    /// An API restart forgets every ask. The worker still shows Claude's answer
    /// to the last one, so it is read before anything is typed again.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_answer_still_on_screen_is_read_before_asking_again() {
        let session = WorkerSessionId::new();
        let (directory, client, submitted) = fake_host(session, Reply::Unknown, None);
        let (state, observed) = hive_with_working_worker(client, session);
        assert_eq!(advance(&state, &observed).await, 1);

        let reloaded = hive_with_working_worker(
            crate::HostClient::new(directory.path().join("host.sock")),
            session,
        )
        .0;
        assert_eq!(advance(&reloaded, &observed).await, 0);
        assert_eq!(*submitted.lock().unwrap(), vec!["/restart".to_owned()]);
        assert!(!reloaded.provider_updates.moves_in_place(&release(), None));
    }

    /// A refusal over Remote Control is not Claude lacking the command, so it
    /// leaves the button asking again in place.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_refusal_is_not_read_as_a_missing_command() {
        let session = WorkerSessionId::new();
        let (_directory, client, _submitted) = fake_host(session, Reply::Refuses, None);
        let (state, observed) = hive_with_working_worker(client, session);

        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(advance(&state, &observed).await, 0);
        assert!(state.provider_updates.moves_in_place(&release(), None));
        assert!(
            state
                .provider_updates
                .moves_in_place(&release(), Some(true))
        );
    }

    #[test]
    fn the_button_lets_a_refused_worker_be_asked_again() {
        let updates = ProviderUpdates::default();
        let session = WorkerSessionId::new();
        updates.requests().insert(session, asked(INSTALLED, false));

        assert_eq!(updates.asked_onto(&release()), vec![session]);
        updates.ask_again(&[session]);
        assert!(updates.asked_onto(&release()).is_empty());
    }
}
