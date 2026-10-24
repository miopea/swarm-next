//! Reconnects Swarm's tools for a Claude worker that lost them (ADR 0123).
//!
//! Workers reach Swarm's tools through a stdio relay. When its connection
//! fails, Claude stops the relay and never retries it by itself, so the worker
//! keeps working with no way to record what it did. Measured on 2.1.310:
//! `/mcp reconnect all` starts a fresh relay in under a second and gives the
//! prompt back empty. Swarm types it for the worker, once per loss, when the
//! worker rests.
use std::{collections::HashMap, sync::Mutex};

use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_terminal::ProviderActivity;

use crate::{
    AppState,
    coordination_delivery::{
        BusyPolicy, Cadence, CoordinationMessage, TerminalSubmission, submit_coordination_message,
    },
    provider_activity::ProviderSignals,
    task_store,
};

/// How many times one session is asked to reconnect, ever.
///
/// A relay that fails again right after reconnecting (a refused credential,
/// an API that answers 503) would otherwise be asked forever. Past this the
/// roster's "Swarm tools disconnected" line is what speaks.
pub(super) const MAX_RECONNECTS_PER_SESSION: u8 = 3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Asked {
    /// Asks so far for this session.
    count: u8,
    /// Asked during the current loss and not seen reconnected since.
    pending: bool,
}

/// In memory, like the update requests: an API restart forgets them, which
/// costs at most one more ask for a session still without its relay.
#[derive(Debug, Default)]
pub(super) struct ToolReconnects {
    asked: Mutex<HashMap<WorkerSessionId, Asked>>,
    /// One pass at a time, so two passes never type it twice into a worker.
    pass: tokio::sync::Mutex<()>,
}

impl ToolReconnects {
    fn asked(&self) -> std::sync::MutexGuard<'_, HashMap<WorkerSessionId, Asked>> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReconnectStep {
    Wait,
    Reconnect,
    /// The relay is back after an ask: the next loss may be asked about.
    Recovered,
}

/// Decides one session's step from what is known about it now.
#[must_use]
pub(super) fn reconnect_step(
    relay: Option<bool>,
    signals: Option<&ProviderSignals>,
    asked: Asked,
) -> ReconnectStep {
    let resting = signals.is_some_and(|signals| signals.activity == ProviderActivity::Resting);
    match relay {
        Some(true) if asked.pending => ReconnectStep::Recovered,
        // A local command, not a turn: shells running beside the prompt do
        // not stop it, but a turn, a question or unread screen does.
        Some(false) if resting && !asked.pending && asked.count < MAX_RECONNECTS_PER_SESSION => {
            ReconnectStep::Reconnect
        }
        // Unknown is no evidence of a loss.
        _ => ReconnectStep::Wait,
    }
}

fn reconnect_message() -> CoordinationMessage {
    CoordinationMessage {
        cadence: Cadence::Immediate,
        busy: BusyPolicy::Wait,
        bytes: b"/mcp reconnect all\r".to_vec(),
        marker: b"/mcp reconnect all".to_vec(),
    }
}

/// One pass over every live Claude worker. Returns how many were asked.
pub(super) async fn advance(
    state: &AppState,
    observed: &HashMap<WorkerSessionId, ProviderSignals>,
) -> usize {
    let _pass = state.tool_reconnects.pass.lock().await;
    let (Some(client), Ok(store)) = (state.terminal_host.as_ref(), task_store(state)) else {
        return 0;
    };
    let Ok(sessions) = store.active_worker_sessions() else {
        return 0;
    };
    let relays = state.tools_relays.read().await.clone();
    let claude = sessions
        .into_iter()
        .filter(|session| session.provider == ProviderKind::ClaudeCode)
        .collect::<Vec<_>>();
    let due = {
        let mut asked = state.tool_reconnects.asked();
        asked.retain(|session, _| claude.iter().any(|live| live.session_id == *session));
        let mut due = Vec::new();
        for session in &claude {
            let entry = asked.entry(session.session_id).or_default();
            match reconnect_step(
                relays.get(&session.session_id).copied(),
                observed.get(&session.session_id),
                *entry,
            ) {
                ReconnectStep::Wait => {}
                ReconnectStep::Recovered => entry.pending = false,
                ReconnectStep::Reconnect => due.push(*session),
            }
        }
        due
    };
    let mut asked_now = 0;
    for session in due {
        let outcome =
            submit_coordination_message(store, client, session.session_id, reconnect_message())
                .await;
        if matches!(outcome, Ok(TerminalSubmission::Deferred(_))) {
            // Not written. Asked when the worker next rests.
            continue;
        }
        // Written, or possibly written: this loss has had its ask either way.
        let mut asked = state.tool_reconnects.asked();
        let entry = asked.entry(session.session_id).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.pending = true;
        if matches!(outcome, Ok(TerminalSubmission::Acknowledged)) {
            asked_now += 1;
            tracing::info!(worker_id = %session.worker_id, "asked a Claude worker to reconnect Swarm's tools");
        } else {
            tracing::warn!(worker_id = %session.worker_id, outcome = ?outcome, "a Claude worker's tool reconnect could not be confirmed as sent");
        }
    }
    asked_now
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(activity: ProviderActivity) -> ProviderSignals {
        ProviderSignals::from_screen(activity, swarm_terminal::BackgroundWork::default())
    }

    #[test]
    fn a_lost_relay_is_reconnected_only_at_rest_and_once_per_loss() {
        let resting = signals(ProviderActivity::Resting);
        let fresh = Asked::default();
        assert_eq!(
            reconnect_step(Some(false), Some(&resting), fresh),
            ReconnectStep::Reconnect
        );
        for busy in [
            ProviderActivity::Active,
            ProviderActivity::AwaitingOperator,
            ProviderActivity::Unknown,
        ] {
            assert_eq!(
                reconnect_step(Some(false), Some(&signals(busy)), fresh),
                ReconnectStep::Wait,
                "{busy:?}"
            );
        }
        assert_eq!(
            reconnect_step(Some(false), None, fresh),
            ReconnectStep::Wait
        );
        // Unknown and present relays are not losses.
        assert_eq!(
            reconnect_step(None, Some(&resting), fresh),
            ReconnectStep::Wait
        );
        assert_eq!(
            reconnect_step(Some(true), Some(&resting), fresh),
            ReconnectStep::Wait
        );

        let pending = Asked {
            count: 1,
            pending: true,
        };
        assert_eq!(
            reconnect_step(Some(false), Some(&resting), pending),
            ReconnectStep::Wait,
            "asked once for this loss"
        );
        assert_eq!(
            reconnect_step(Some(true), Some(&resting), pending),
            ReconnectStep::Recovered
        );
    }

    /// A relay that keeps failing is asked about a bounded number of times.
    #[test]
    fn a_session_is_asked_at_most_three_times() {
        let resting = signals(ProviderActivity::Resting);
        let spent = Asked {
            count: MAX_RECONNECTS_PER_SESSION,
            pending: false,
        };
        assert_eq!(
            reconnect_step(Some(false), Some(&resting), spent),
            ReconnectStep::Wait
        );
    }

    /// A fake terminal host holding one resting Claude session that answers
    /// `/mcp reconnect all` the way 2.1.310 does.
    #[cfg(unix)]
    fn fake_host(
        session: WorkerSessionId,
    ) -> (
        tempfile::TempDir,
        crate::HostClient,
        std::sync::Arc<Mutex<Vec<String>>>,
    ) {
        use std::fmt::Write as _;

        use swarm_terminal::{
            CanonicalTerminalState, HostRequest, HostResponse, JournalLimits, TerminalSize,
        };
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let submitted = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let recorder = submitted.clone();
        tokio::spawn(async move {
            let mut terminal = CanonicalTerminalState::new(
                JournalLimits::new(64 * 1024, 64),
                TerminalSize::new(30, 240),
            );
            let mut transcript = String::from("● Done.\r\n");
            let mut typed = String::new();
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
                            let _ = write!(
                                transcript,
                                "\r\n❯ {typed}\r\n  ⎿  Reconnected 1 of 1 MCP server\r\n"
                            );
                            recorder.lock().unwrap().push(std::mem::take(&mut typed));
                        } else {
                            typed.push_str(&String::from_utf8(bytes).unwrap());
                        }
                        redraw(&mut terminal, &transcript, &typed);
                        HostResponse::Acknowledged
                    }
                    other => panic!("the reconnect pass sent an unexpected request: {other:?}"),
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                let _ = reader.get_mut().write_all(&bytes).await;
            }
        });
        (directory, crate::HostClient::new(socket), submitted)
    }

    /// The whole slice against a terminal: a resting worker whose relay is gone
    /// is sent `/mcp reconnect all` once, is not asked again while the relay
    /// stays gone, and is asked again for a later loss, up to the bound.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_lost_its_tools_is_reconnected_once_per_loss() {
        let session = WorkerSessionId::new();
        let (_directory, client, submitted) = fake_host(session);
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
        let state = AppState::default()
            .with_terminal_host(client, "fixture-only")
            .with_task_store(store);
        let observed = HashMap::from([(session, signals(ProviderActivity::Resting))]);
        let relay = |running: bool| {
            let state = &state;
            async move {
                state.tools_relays.write().await.insert(session, running);
            }
        };

        relay(false).await;
        assert_eq!(advance(&state, &observed).await, 1);
        assert_eq!(
            *submitted.lock().unwrap(),
            vec!["/mcp reconnect all".to_owned()]
        );
        assert_eq!(advance(&state, &observed).await, 0, "once for this loss");

        relay(true).await;
        assert_eq!(advance(&state, &observed).await, 0);
        for _ in 0..5 {
            relay(false).await;
            advance(&state, &observed).await;
            relay(true).await;
            advance(&state, &observed).await;
        }
        assert_eq!(
            submitted.lock().unwrap().len(),
            usize::from(MAX_RECONNECTS_PER_SESSION),
            "bounded however often the relay is lost"
        );
    }
}
