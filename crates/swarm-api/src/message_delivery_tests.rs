use super::{AppState, HostClient, TaskStore};
use futures_util::FutureExt;
use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_persistence::MessageEnd;
#[cfg(unix)]
use swarm_terminal::{
    CanonicalTerminalState, HostRequest, HostResponse, JournalLimits, TerminalSize,
};

#[tokio::test]
async fn next_exclusive_delivery_pass_recovers_an_abandoned_claim_without_replaying() {
    let runtime = tempfile::TempDir::new().unwrap();
    let store = TaskStore::in_memory().unwrap();
    let worker = store
        .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
        .unwrap();
    store
        .bind_worker_session(worker.id, WorkerSessionId::new())
        .unwrap();
    let task = store
        .create_task("Interrupted delivery", "/workspace")
        .unwrap();
    store
        .send_task_message(
            task.id,
            MessageEnd::queen(),
            MessageEnd::worker(worker.id),
            "Which SHA?",
            10,
        )
        .unwrap();
    let claim = store.claim_task_messages(11).unwrap().remove(0);
    let state = AppState::default()
        .with_task_store(store.clone())
        .with_terminal_host(
            HostClient::new(runtime.path().join("absent.sock")),
            "test-only",
        );
    let owner = state.coordination_delivery.lock().await;
    let next_pass = state.deliver_coordination();
    tokio::pin!(next_pass);
    assert!(next_pass.as_mut().now_or_never().is_none());
    assert_eq!(
        store.task_messages(task.id).unwrap()[0].delivery_state,
        "dispatching",
        "a live owner cannot have its claim recovered by another pass"
    );
    drop(owner);
    next_pass.await;
    let attention = store.task_message_attention(i64::MAX, i64::MAX).unwrap();
    assert_eq!(attention.total, 1);
    assert_eq!(
        attention.items[0].claim_id.as_deref(),
        Some(claim.claim_id.as_str())
    );
    assert_eq!(attention.items[0].state, "uncertain");
    assert!(
        store.task_messages(task.id).unwrap()[0]
            .delivered_at
            .is_none()
    );
}

#[tokio::test]
async fn failed_transport_is_visible_and_only_explicit_reconciliation_retries() {
    let runtime = tempfile::TempDir::new().unwrap();
    let client = HostClient::new(runtime.path().join("absent.sock"));
    let store = TaskStore::in_memory().unwrap();
    let worker = store
        .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
        .unwrap();
    store
        .bind_worker_session(worker.id, WorkerSessionId::new())
        .unwrap();
    let task = store.create_task("Delivery check", "/workspace").unwrap();
    let message = store
        .send_task_message(
            task.id,
            MessageEnd::queen(),
            MessageEnd::worker(worker.id),
            "Which SHA?",
            10,
        )
        .unwrap();
    let state = AppState::default();
    let changed = state.control_room_notify.notified();
    tokio::pin!(changed);
    changed.as_mut().enable();
    state.deliver_task_messages(&store, &client).await;
    assert!(
        changed.now_or_never().is_some(),
        "persisted uncertainty wakes existing feed waiters"
    );
    let first = store.task_message_attention(i64::MAX, i64::MAX).unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.items[0].state, "uncertain");
    let first_claim = first.items[0]
        .claim_id
        .clone()
        .expect("an uncertain row was claimed before it became uncertain");
    let unchanged = state.control_room_notify.notified();
    tokio::pin!(unchanged);
    unchanged.as_mut().enable();
    state.deliver_task_messages(&store, &client).await;
    assert!(
        unchanged.now_or_never().is_none(),
        "an unchanged queue causes no feed wakeup"
    );
    assert_eq!(
        store
            .task_message_attention(i64::MAX, i64::MAX)
            .unwrap()
            .items[0]
            .claim_id
            .as_deref(),
        Some(first_claim.as_str())
    );
    assert!(
        store
            .reconcile_task_message(
                &message.id,
                &first_claim,
                true,
                "Explicit safe retry check",
                20
            )
            .unwrap()
    );
    state.deliver_task_messages(&store, &client).await;
    let retry = store.task_message_attention(i64::MAX, i64::MAX).unwrap();
    assert_ne!(
        retry.items[0].claim_id.as_deref(),
        Some(first_claim.as_str())
    );
    assert!(
        store.task_messages(task.id).unwrap()[0]
            .delivered_at
            .is_none()
    );
}

/// Worker→Queen messages whose write delivery gave up on, with Queen's live
/// terminal. Returns the store, Queen's session, the task and message ids.
#[cfg(unix)]
fn unconfirmed_messages_to_queen(
    count: usize,
) -> (
    TaskStore,
    WorkerSessionId,
    swarm_domain::TaskId,
    Vec<String>,
) {
    let store = TaskStore::in_memory().unwrap();
    let queen = store.ensure_queen("/workspace/queen").unwrap();
    let queen_session = WorkerSessionId::new();
    store.bind_worker_session(queen.id, queen_session).unwrap();
    let worker = store
        .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
        .unwrap();
    store
        .bind_worker_session(worker.id, WorkerSessionId::new())
        .unwrap();
    let task = store.create_task("Stalled report", "/workspace").unwrap();
    let ids = (0..count)
        .map(|_| {
            store
                .send_task_message(
                    task.id,
                    MessageEnd::worker(worker.id),
                    MessageEnd::queen(),
                    "Ready for review.",
                    10,
                )
                .unwrap()
                .id
        })
        .collect();
    give_up_on_every_claim(&store);
    (store, queen_session, task.id, ids)
}

#[cfg(unix)]
fn give_up_on_every_claim(store: &TaskStore) {
    for claim in store.claim_task_messages(20).unwrap() {
        assert!(
            store
                .finish_task_message(&claim, swarm_persistence::TaskMessageResult::Uncertain, 21)
                .unwrap()
        );
    }
}

#[cfg(unix)]
fn screen(session_id: WorkerSessionId, text: &str, last_output_at: Option<i64>) -> HostResponse {
    let mut state = CanonicalTerminalState::new(
        JournalLimits::new(64 * 1024, 64),
        TerminalSize::new(24, 100),
    );
    state.push(text.as_bytes().to_vec());
    HostResponse::Output {
        session_id,
        running: true,
        resume: swarm_terminal::Resume::Snapshot {
            snapshot: state.snapshot(),
        },
        provider_turn: None,
        last_output_at,
    }
}

/// Every write a fake terminal host received, in order.
#[cfg(unix)]
type RecordedWrites = std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>;

/// A terminal host that shows `before` until something is written, then
/// `after`, acknowledging and recording every write.
#[cfg(unix)]
fn serve_terminal(
    listener: tokio::net::UnixListener,
    before: HostResponse,
    after: HostResponse,
) -> (tokio::task::JoinHandle<()>, RecordedWrites) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let writes = RecordedWrites::default();
    let recorded = writes.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let response = match serde_json::from_str::<HostRequest>(&line).unwrap() {
                HostRequest::Read { .. } if recorded.lock().unwrap().is_empty() => before.clone(),
                HostRequest::Read { .. } => after.clone(),
                HostRequest::Write { bytes, .. } => {
                    recorded.lock().unwrap().push(bytes);
                    HostResponse::Acknowledged
                }
                other => panic!("unexpected host request {other:?}"),
            };
            let mut bytes = serde_json::to_vec(&response).unwrap();
            bytes.push(b'\n');
            reader.get_mut().write_all(&bytes).await.unwrap();
        }
    });
    (server, writes)
}

/// ⚠️ THE STALL. A framed paste reached Queen's prompt after delivery had
/// stopped watching, sat there unsent, and held every delivery behind it for
/// hours. Swarm now finds its own marker in the prompt and presses Enter, once,
/// and the messages it carried are delivered (ADR 0173).
#[cfg(unix)]
#[tokio::test]
async fn a_write_left_unsent_in_the_prompt_is_sent_and_delivered() {
    let (store, session, task, ids) = unconfirmed_messages_to_queen(2);
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("host.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let (server, writes) = serve_terminal(
        listener,
        screen(
            session,
            &format!(
                "● Done.\r\n\r\n❯ [Swarm delivery {}] [Pasted text #1 +40 lines]\r\n  auto mode on",
                ids[0]
            ),
            None,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );
    let client = HostClient::new(socket);

    assert!(crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await);
    assert_eq!(
        *writes.lock().unwrap(),
        [vec![b'\r']],
        "one Enter, nothing else"
    );
    let messages = store.task_messages(task).unwrap();
    assert_eq!(messages.len(), ids.len());
    for message in messages {
        assert_eq!(message.delivery_state, "delivered");
        assert_eq!(message.delivered_session_id, Some(session));
    }
    assert!(
        store
            .next_coordination_cooldown_end(crate::unix_timestamp())
            .unwrap()
            .is_some(),
        "a submitted write paces Queen's terminal like any other"
    );
    assert!(
        store
            .unconfirmed_task_message_writes(crate::unix_timestamp())
            .unwrap()
            .is_empty()
    );
    assert!(
        !crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await
    );
    assert_eq!(writes.lock().unwrap().len(), 1);
    server.abort();
}

/// A write that is not in an empty prompt the terminal has drawn since is
/// queued once more, marked as a possible repeat. Its second loss, and any
/// prompt holding somebody else's text, write nothing and wait for Queen.
#[cfg(unix)]
#[tokio::test]
async fn a_lost_write_is_queued_again_once_and_a_draft_is_left_alone() {
    let (store, session, _, ids) = unconfirmed_messages_to_queen(1);
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("host.sock");
    let drawn_since = Some(crate::unix_timestamp());
    let (server, writes) = serve_terminal(
        tokio::net::UnixListener::bind(&socket).unwrap(),
        screen(
            session,
            "● Done.\r\n\r\n❯ \r\n  ? for shortcuts",
            drawn_since,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );
    let client = HostClient::new(socket.clone());

    assert!(crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await);
    let again = store.claim_task_messages(30).unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].message.message_id, ids[0]);
    assert!(again[0].message.repeats_write.is_some());
    store
        .finish_task_message(
            &again[0],
            swarm_persistence::TaskMessageResult::Uncertain,
            31,
        )
        .unwrap();
    assert!(
        !crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await,
        "a second unconfirmed attempt is Queen's to reconcile"
    );
    assert_eq!(
        store
            .task_message_attention(i64::MAX, i64::MAX)
            .unwrap()
            .total,
        1
    );
    server.abort();

    let (store, session, _, _) = unconfirmed_messages_to_queen(1);
    std::fs::remove_file(&socket).unwrap();
    let (server, draft_writes) = serve_terminal(
        tokio::net::UnixListener::bind(&socket).unwrap(),
        screen(
            session,
            "● Done.\r\n\r\n❯ operator draft\r\n  auto mode on",
            drawn_since,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );
    assert!(
        !crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await
    );
    assert_eq!(
        store
            .task_message_attention(i64::MAX, i64::MAX)
            .unwrap()
            .items[0]
            .state,
        "uncertain"
    );
    assert!(writes.lock().unwrap().is_empty());
    assert!(draft_writes.lock().unwrap().is_empty());
    server.abort();
}

/// ⚠️ A HELD MESSAGE WAS LISTED NOWHERE. A message to a prompt holding
/// somebody's draft waited in silence while outcomes held the same way were
/// listed. It now records a hold for that terminal, writes nothing into the
/// draft, and stays queued for the next pass (ADR 0173).
#[cfg(unix)]
#[tokio::test]
async fn a_message_held_at_a_draft_is_listed_and_not_written() {
    let store = TaskStore::in_memory().unwrap();
    let queen = store.ensure_queen("/workspace/queen").unwrap();
    let session = WorkerSessionId::new();
    store.bind_worker_session(queen.id, session).unwrap();
    let worker = store
        .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
        .unwrap();
    store
        .bind_worker_session(worker.id, WorkerSessionId::new())
        .unwrap();
    let task = store.create_task("Held report", "/workspace").unwrap();
    let id = store
        .send_task_message(
            task.id,
            MessageEnd::worker(worker.id),
            MessageEnd::queen(),
            "Ready for review.",
            10,
        )
        .unwrap()
        .id;
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("host.sock");
    let (server, writes) = serve_terminal(
        tokio::net::UnixListener::bind(&socket).unwrap(),
        screen(
            session,
            "● Done.\r\n\r\n❯ operator draft\r\n  auto mode on",
            None,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );
    let client = HostClient::new(socket);
    let state = AppState::default()
        .with_task_store(store.clone())
        .with_terminal_host(client.clone(), "test-only");

    state.deliver_task_messages(&store, &client).await;

    assert!(
        writes.lock().unwrap().is_empty(),
        "nothing typed into a draft"
    );
    let message = store
        .task_messages(task.id)
        .unwrap()
        .into_iter()
        .find(|message| message.id == id)
        .unwrap();
    assert_eq!(message.delivery_state, "queued");
    let holds = store
        .standing_coordinator_refusals(crate::unix_timestamp(), 0)
        .unwrap();
    let subject = format!("message-delivery:{session}");
    let hold = holds
        .iter()
        .find(|refusal| refusal.subject == subject)
        .unwrap_or_else(|| panic!("no hold for {subject}: {holds:?}"));
    assert_eq!(
        hold.kind,
        swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT
    );
    assert_eq!(hold.worker_id, Some(queen.id));
    assert!(!hold.reason.contains("clear the line"), "{}", hold.reason);
    server.abort();
}

/// The Enter that sends a stranded write is a delivery to that terminal, so
/// it waits out the cooldown like any other. The write stays in the prompt
/// meanwhile, and a pass after the cooldown sends it.
#[cfg(unix)]
#[tokio::test]
async fn a_stranded_write_waits_out_the_cooldown() {
    let (store, session, task, ids) = unconfirmed_messages_to_queen(1);
    store
        .record_coordination_delivery(session, crate::unix_timestamp())
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("host.sock");
    let (server, writes) = serve_terminal(
        tokio::net::UnixListener::bind(&socket).unwrap(),
        screen(
            session,
            &format!(
                "● Done.\r\n\r\n❯ [Swarm delivery {}] [Pasted text #1 +40 lines]\r\n  auto mode on",
                ids[0]
            ),
            None,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );
    let client = HostClient::new(socket);

    assert!(
        !crate::coordination_delivery::recover_unconfirmed_task_messages(&store, &client).await
    );
    assert!(writes.lock().unwrap().is_empty());
    assert_eq!(
        store.task_messages(task).unwrap()[0].delivery_state,
        "uncertain"
    );
    assert_eq!(
        store
            .unconfirmed_task_message_writes(crate::unix_timestamp())
            .unwrap()
            .len(),
        1
    );
    server.abort();
}

/// The hold a stranded write raised describes text the Enter has just sent,
/// so sending it releases the hold, even while newer messages still wait
/// out the cooldown it starts.
#[cfg(unix)]
#[tokio::test]
async fn sending_a_stranded_write_releases_its_hold() {
    use std::str::FromStr;
    let (store, session, task, ids) = unconfirmed_messages_to_queen(1);
    let sender = store.task_messages(task).unwrap()[0]
        .sender_worker_id
        .clone()
        .unwrap();
    store
        .send_task_message(
            task,
            MessageEnd::worker(swarm_domain::WorkerId::from_str(&sender).unwrap()),
            MessageEnd::queen(),
            "And the tag?",
            30,
        )
        .unwrap();
    // A pass found it held at the prompt and raised the hold.
    for claim in store.claim_task_messages(31).unwrap() {
        assert!(
            store
                .finish_task_message(&claim, swarm_persistence::TaskMessageResult::Deferred, 32)
                .unwrap()
        );
    }
    let subject = crate::message_delivery_hold_subject(session);
    store
        .record_coordinator_refusal(
            swarm_persistence::REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
            &subject,
            None,
            Some(session),
            "held",
            crate::unix_timestamp(),
        )
        .unwrap();
    let holds = |store: &TaskStore| {
        store
            .standing_coordinator_refusals(crate::unix_timestamp(), 0)
            .unwrap()
            .into_iter()
            .filter(|refusal| refusal.subject == subject)
            .count()
    };
    assert_eq!(holds(&store), 1);
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("host.sock");
    let (server, writes) = serve_terminal(
        tokio::net::UnixListener::bind(&socket).unwrap(),
        screen(
            session,
            &format!(
                "● Done.\r\n\r\n❯ [Swarm delivery {}] [Pasted text #1 +40 lines]\r\n  auto mode on",
                ids[0]
            ),
            None,
        ),
        screen(session, "✻ Thinking… (esc to interrupt)\r\n❯ ", None),
    );

    assert!(
        crate::coordination_delivery::recover_unconfirmed_task_messages(
            &store,
            &HostClient::new(socket)
        )
        .await
    );
    assert_eq!(writes.lock().unwrap().len(), 1);
    assert_eq!(holds(&store), 0, "the prompt no longer holds that text");
    server.abort();
}
