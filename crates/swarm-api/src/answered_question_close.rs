//! Closing a question on a worker's screen that the operator already answered
//! in Needs you (ADR 0134).
//!
//! An answer to a decision is held while the worker's terminal shows a
//! question, because text written into an open question would be read as its
//! answer. When the question on screen IS the decision the operator just
//! answered, holding protects nothing: the operator has answered once and is
//! waiting for the answer to arrive. Measured over 14 days, 180 of 577 inbox
//! answers waited more than a minute, and one waited four hours behind the very
//! question it answered.
//!
//! So Swarm presses Esc on that question, once, and then delivers the answer.
//! It does this only when the live menu asks one of the decision's questions in
//! the same words; any other question is left alone.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use swarm_domain::{DecisionRequestId, ProviderKind, WorkerSessionId};
use swarm_persistence::{DecisionDispatch, TaskStore};
use swarm_terminal::{HostRequest, HostResponse, ProviderActivity, TerminalSnapshot};

use crate::coordination_delivery::coordination_request;
use crate::{HostClient, TerminalWriteProvenance, provider_activity, unix_timestamp};

/// How long Swarm watches for the closed question to leave the screen before
/// leaving delivery to the next pass.
const MENU_CLOSE_DEADLINE: Duration = Duration::from_secs(5);
const MENU_CLOSE_POLL: Duration = Duration::from_millis(250);

/// The decisions whose question Swarm has already pressed Esc on.
///
/// ONCE PER DECISION, because Esc is not idempotent on Claude's screen: on an
/// empty prompt a second press opens the rewind menu. If the question is still
/// there after one press, something other than this answer is holding it, and
/// pressing again on every pass would be a repeated timer standing in for
/// correctness. The answer then waits, as it did before.
///
/// Owned by `AppState` and bounded: the oldest entry goes first. A decision is
/// answered and delivered within minutes, so an entry that old is finished.
/// Not persisted: after a restart a question that is STILL open and still
/// matches may be pressed once more, which is the same decision made again on
/// fresh evidence.
#[derive(Debug, Default)]
pub(super) struct AnsweredQuestions {
    pressed: Mutex<VecDeque<DecisionRequestId>>,
}

impl AnsweredQuestions {
    const CAPACITY: usize = 256;

    /// A record that cannot be read counts as pressed: a doubt about whether
    /// Esc was already sent must never send it again.
    fn already_pressed(&self, decision: DecisionRequestId) -> bool {
        self.pressed
            .lock()
            .map_or(true, |pressed| pressed.contains(&decision))
    }

    fn record(&self, decision: DecisionRequestId) {
        if let Ok(mut pressed) = self.pressed.lock() {
            if pressed.len() == Self::CAPACITY {
                pressed.pop_front();
            }
            pressed.push_back(decision);
        }
    }
}

/// What one look at a held answer's terminal did.
#[derive(Debug, Eq, PartialEq)]
enum Closing {
    /// The screen is not showing this decision's question, or Esc was already
    /// pressed for it. Nothing was written.
    LeftAlone,
    /// Esc was pressed and the question has left the screen.
    Closed,
    /// Esc was pressed, or may have been, and the question was still on screen
    /// at the deadline. The next pass delivers if it has gone by then.
    StillOpen,
}

/// Closes the decision's own question if it is open on the worker's screen.
///
/// Returns true when the question has left the screen, so the caller can
/// deliver the answer in this pass rather than the next.
pub(super) async fn close_answered_question(
    store: &TaskStore,
    client: &HostClient,
    answered: &AnsweredQuestions,
    delivery: &DecisionDispatch,
) -> bool {
    let Ok(decision) = store.get_decision_request(delivery.decision_id) else {
        return false;
    };
    let Ok(provider) = store.provider_for_active_session(delivery.session_id) else {
        return false;
    };
    let questions = decision
        .questions
        .iter()
        .map(|question| question.question.as_str())
        .collect::<Vec<_>>();
    let closing = close_matching_menu(
        client,
        delivery.session_id,
        provider,
        &questions,
        delivery.decision_id,
        answered,
    )
    .await;
    match closing {
        Closing::LeftAlone => false,
        Closing::Closed => {
            tracing::info!(decision_id = %delivery.decision_id, worker_id = %delivery.worker_id, "the operator answered this question in Needs you; closed it on the worker's screen to deliver the answer");
            true
        }
        Closing::StillOpen => {
            tracing::warn!(decision_id = %delivery.decision_id, worker_id = %delivery.worker_id, "pressed Esc on a question the operator answered in Needs you, and it is still on screen; the answer waits");
            false
        }
    }
}

async fn close_matching_menu(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
    questions: &[&str],
    decision: DecisionRequestId,
    answered: &AnsweredQuestions,
) -> Closing {
    // Claude's menu is the only one this build can read a question from.
    if provider != ProviderKind::ClaudeCode || questions.is_empty() {
        return Closing::LeftAlone;
    }
    let Some(snapshot) = awaiting_snapshot(client, session_id, provider).await else {
        return Closing::LeftAlone;
    };
    if !asks_any(&snapshot, questions) || answered.already_pressed(decision) {
        return Closing::LeftAlone;
    }
    let pressed = coordination_request(
        client,
        &HostRequest::Write {
            session_id,
            bytes: vec![0x1b],
            provenance: TerminalWriteProvenance::coordination(),
        },
    )
    .await;
    match pressed {
        Ok(Some(HostResponse::Acknowledged)) => answered.record(decision),
        // A timed-out write may have landed, so it counts as the one press.
        Ok(None) => {
            answered.record(decision);
            return Closing::StillOpen;
        }
        // Refused, including while someone holds the terminal: nothing was
        // pressed, so a later pass may still try once.
        Ok(Some(_)) | Err(_) => return Closing::LeftAlone,
    }
    let deadline = Instant::now() + MENU_CLOSE_DEADLINE;
    while Instant::now() < deadline {
        tokio::time::sleep(MENU_CLOSE_POLL).await;
        if let Some(snapshot) = current_snapshot(client, session_id).await
            && !asks_any(&snapshot, questions)
        {
            return Closing::Closed;
        }
    }
    Closing::StillOpen
}

fn asks_any(snapshot: &TerminalSnapshot, questions: &[&str]) -> bool {
    questions
        .iter()
        .any(|question| swarm_terminal::claude_menu_asks(snapshot, question))
}

/// The screen, only while this running session is waiting on the operator.
async fn awaiting_snapshot(
    client: &HostClient,
    session_id: WorkerSessionId,
    provider: ProviderKind,
) -> Option<TerminalSnapshot> {
    let response = read(client, session_id).await?;
    let HostResponse::Output {
        resume: swarm_terminal::Resume::Snapshot { snapshot },
        provider_turn,
        last_output_at,
        ..
    } = response
    else {
        return None;
    };
    let activity = provider_activity::observed_signals(
        provider,
        &snapshot,
        provider_turn.as_ref(),
        last_output_at,
        unix_timestamp(),
    )
    .activity;
    (activity == ProviderActivity::AwaitingOperator).then_some(snapshot)
}

async fn current_snapshot(
    client: &HostClient,
    session_id: WorkerSessionId,
) -> Option<TerminalSnapshot> {
    match read(client, session_id).await? {
        HostResponse::Output {
            resume: swarm_terminal::Resume::Snapshot { snapshot },
            ..
        } => Some(snapshot),
        _ => None,
    }
}

/// A full snapshot of this running session, or nothing.
async fn read(client: &HostClient, session_id: WorkerSessionId) -> Option<HostResponse> {
    let response = coordination_request(
        client,
        &HostRequest::Read {
            session_id,
            after_sequence: None,
        },
    )
    .await
    .ok()??;
    match &response {
        HostResponse::Output {
            session_id: observed,
            running: true,
            ..
        } if *observed == session_id => Some(response),
        _ => None,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use swarm_terminal::{CanonicalTerminalState, JournalLimits, TerminalSize};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const JAR: &str = "Which fictional jar should hold the spare buttons?";

    fn screen(session_id: WorkerSessionId, text: &str) -> HostResponse {
        let mut state = CanonicalTerminalState::new(
            JournalLimits::new(64 * 1024, 64),
            TerminalSize::new(24, 60),
        );
        state.push(text.as_bytes().to_vec());
        HostResponse::Output {
            session_id,
            running: true,
            resume: swarm_terminal::Resume::Snapshot {
                snapshot: state.snapshot(),
            },
            provider_turn: None,
            last_output_at: None,
        }
    }

    /// A live menu asking `question`, drawn as Claude draws it: a dim border,
    /// and the gaps between words as cursor moves.
    fn menu(question: &str) -> String {
        let mut row = String::from("\x1b[2m│\x1b[3G\x1b[22m");
        let mut column = 3;
        for (index, word) in question.split_whitespace().enumerate() {
            if index > 0 {
                let _ = write!(row, "\x1b[{column}G");
            }
            row.push_str(word);
            column += word.len() + 1;
        }
        format!(
            "● Fictional work.\r\n\r\n──────────\r\n ☐ Jar \r\n{row}\r\n\
             ❯ 1. Amber\r\n  2. Teal\r\n  3. Type something.\r\n──────────\r\n\
             4. Chat about this\r\n\r\nEnter to select · ↑/↓ to navigate · Esc to cancel"
        )
    }

    const CLOSED: &str = "● Fictional work.\r\n  ⎿  Interrupted\r\n\r\n──────────\r\n❯ \r\n──────────\r\n  ? for shortcuts";

    /// Serves one canned response per connection and records every request.
    fn host(
        responses: Vec<HostResponse>,
    ) -> (
        tempfile::TempDir,
        HostClient,
        tokio::task::JoinHandle<Vec<HostRequest>>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for response in responses {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                seen.push(serde_json::from_str::<HostRequest>(&line).unwrap());
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
            }
            seen
        });
        (directory, HostClient::new(socket), server)
    }

    fn escapes(requests: &[HostRequest]) -> usize {
        requests
            .iter()
            .filter(
                |request| matches!(request, HostRequest::Write { bytes, .. } if bytes == &[0x1b]),
            )
            .count()
    }

    /// The operator, 2026-10-31: "I answer in the worker and the needs you
    /// page still shows and vice versa." This is the second half: the card was
    /// answered in Needs you while the worker's screen showed the same
    /// question, and the answer waited behind it.
    #[tokio::test]
    async fn the_same_question_on_screen_is_closed_once_and_then_left_alone() {
        let session = WorkerSessionId::new();
        let decision = DecisionRequestId::new();
        let answered = AnsweredQuestions::default();
        let (_directory, client, server) = host(vec![
            screen(session, &menu(JAR)),
            HostResponse::Acknowledged,
            screen(session, CLOSED),
        ]);
        let closing = close_matching_menu(
            &client,
            session,
            ProviderKind::ClaudeCode,
            &[JAR],
            decision,
            &answered,
        )
        .await;
        let requests = server.await.unwrap();
        assert_eq!(closing, Closing::Closed);
        assert_eq!(escapes(&requests), 1);

        // Still on screen on a later pass: Esc is not pressed a second time.
        let (_directory, client, server) = host(vec![screen(session, &menu(JAR))]);
        let again = close_matching_menu(
            &client,
            session,
            ProviderKind::ClaudeCode,
            &[JAR],
            decision,
            &answered,
        )
        .await;
        assert_eq!(again, Closing::LeftAlone);
        assert_eq!(escapes(&server.await.unwrap()), 0);
    }

    /// The operator's ruling: "Only when the question on screen matches the
    /// card's wording; any other question is left alone."
    #[tokio::test]
    async fn a_different_question_or_provider_is_never_pressed() {
        let session = WorkerSessionId::new();
        let answered = AnsweredQuestions::default();
        let (_directory, client, server) =
            host(vec![screen(session, &menu("Which fictional lid fits?"))]);
        let closing = close_matching_menu(
            &client,
            session,
            ProviderKind::ClaudeCode,
            &[JAR],
            DecisionRequestId::new(),
            &answered,
        )
        .await;
        assert_eq!(closing, Closing::LeftAlone);
        assert_eq!(escapes(&server.await.unwrap()), 0);

        // Another session's screen is not this worker's, whatever it shows.
        let (_directory, client, server) = host(vec![screen(WorkerSessionId::new(), &menu(JAR))]);
        let closing = close_matching_menu(
            &client,
            session,
            ProviderKind::ClaudeCode,
            &[JAR],
            DecisionRequestId::new(),
            &answered,
        )
        .await;
        assert_eq!(closing, Closing::LeftAlone);
        assert_eq!(escapes(&server.await.unwrap()), 0);

        // A provider whose menu this build cannot read is not even looked at.
        let (_directory, client, server) = host(Vec::new());
        let closing = close_matching_menu(
            &client,
            session,
            ProviderKind::Codex,
            &[JAR],
            DecisionRequestId::new(),
            &answered,
        )
        .await;
        assert_eq!(closing, Closing::LeftAlone);
        assert!(server.await.unwrap().is_empty());
    }

    /// Refused while someone holds the terminal: nothing was pressed, so the
    /// one press is still available to a later pass.
    #[tokio::test]
    async fn a_refused_press_does_not_spend_the_only_one() {
        let session = WorkerSessionId::new();
        let decision = DecisionRequestId::new();
        let answered = AnsweredQuestions::default();
        let (_directory, client, server) = host(vec![
            screen(session, &menu(JAR)),
            HostResponse::Error {
                code: "coordination_control_held".into(),
                message: "held".into(),
            },
        ]);
        let closing = close_matching_menu(
            &client,
            session,
            ProviderKind::ClaudeCode,
            &[JAR],
            decision,
            &answered,
        )
        .await;
        server.await.unwrap();
        assert_eq!(closing, Closing::LeftAlone);
        assert!(!answered.already_pressed(decision));
    }

    #[test]
    fn the_record_of_presses_is_bounded() {
        let answered = AnsweredQuestions::default();
        let first = DecisionRequestId::new();
        answered.record(first);
        for _ in 0..AnsweredQuestions::CAPACITY {
            answered.record(DecisionRequestId::new());
        }
        assert!(!answered.already_pressed(first));
        assert_eq!(
            answered.pressed.lock().unwrap().len(),
            AnsweredQuestions::CAPACITY
        );
    }

    /// A Claude session that shows `question` until Esc, then rests, takes
    /// typed text and submits it on Enter. Records every write it is sent.
    fn claude_host(
        session: WorkerSessionId,
        question: &'static str,
    ) -> (
        tempfile::TempDir,
        HostClient,
        std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("host.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let writes = std::sync::Arc::new(Mutex::new(Vec::new()));
        let recorder = writes.clone();
        tokio::spawn(async move {
            let mut menu_open = true;
            let mut transcript = String::from("● Fictional work.");
            let mut typed = String::new();
            // ONE terminal, redrawn on every change, so its output sequence
            // advances the way a real one does: delivery waits for that.
            let mut terminal = CanonicalTerminalState::new(
                JournalLimits::new(64 * 1024, 64),
                TerminalSize::new(30, 240),
            );
            let draw = |menu_open: bool, transcript: &str, typed: &str| {
                if menu_open {
                    format!("\x1b[2J\x1b[H{}", menu(question))
                } else {
                    format!("\x1b[2J\x1b[H{transcript}\r\n\r\n❯ {typed}\r\n  ? for shortcuts")
                }
                .into_bytes()
            };
            terminal.push(draw(menu_open, &transcript, &typed));
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    continue;
                }
                let response = match serde_json::from_str::<HostRequest>(&line).unwrap() {
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
                        recorder.lock().unwrap().push(bytes.clone());
                        if bytes == [0x1b] {
                            if menu_open {
                                menu_open = false;
                                transcript.push_str("\r\n  ⎿  Interrupted");
                            }
                        } else if bytes == b"\r" {
                            let _ = write!(transcript, "\r\n❯ {}", std::mem::take(&mut typed));
                        } else {
                            typed.push_str(&String::from_utf8(bytes).unwrap());
                        }
                        terminal.push(draw(menu_open, &transcript, &typed));
                        HostResponse::Acknowledged
                    }
                    other => panic!("delivery sent an unexpected request: {other:?}"),
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                let _ = reader.get_mut().write_all(&bytes).await;
            }
        });
        (directory, HostClient::new(socket), writes)
    }

    /// The whole slice, through the real delivery pass: the operator answers
    /// the card in Needs you while the worker shows the same question. Swarm
    /// presses Esc once, then types and submits the answer, in one pass.
    #[tokio::test]
    async fn an_answer_given_in_needs_you_closes_the_same_question_and_arrives() {
        use std::collections::BTreeMap;

        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let decision = store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: worker.id,
                task_id: None,
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Fictional jar",
                summary: "Choose",
                reason: "Test",
                risk: "",
                evidence: "",
                suggested_action: "Choose",
                allowed_actions: &[],
                operator_actions: &[],
                questions: &[swarm_domain::DecisionQuestion {
                    header: "Jar".into(),
                    question: JAR.into(),
                    options: vec!["Amber".into(), "Teal".into()],
                    option_descriptions: BTreeMap::new(),
                    multi_select: false,
                }],
                deadline: None,
                requested_command: None,
                requested_script: None,
            })
            .unwrap();
        store
            .answer_decision_request(
                decision.id,
                &BTreeMap::from([("Jar".to_owned(), vec!["Amber".to_owned()])]),
                "",
                "inbox",
            )
            .unwrap();
        let (_directory, client, writes) = claude_host(session, JAR);
        let state = crate::AppState::default().with_task_store(store.clone());

        state.deliver_decision_outcomes(&store, &client).await;

        let writes = writes.lock().unwrap().clone();
        assert_eq!(writes.first().map(Vec::as_slice), Some(&[0x1b][..]));
        assert_eq!(writes.iter().filter(|bytes| bytes == &&[0x1b]).count(), 1);
        let typed = String::from_utf8(writes[1].clone()).unwrap();
        assert!(typed.contains(&decision.id.to_string()) && typed.contains("Jar: Amber"));
        assert_eq!(writes.last().map(Vec::as_slice), Some(&b"\r"[..]));
        assert!(
            store
                .claim_decision_deliveries(i64::MAX)
                .unwrap()
                .is_empty(),
            "the answer was delivered, not held for another pass"
        );
    }
}
