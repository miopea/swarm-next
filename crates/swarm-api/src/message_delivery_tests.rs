use super::{AppState, HostClient, TaskStore};
use futures_util::FutureExt;
use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_persistence::MessageEnd;

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
    let attention = store.task_message_attention().unwrap();
    assert_eq!(attention.total, 1);
    assert_eq!(attention.items[0].claim_id, claim.claim_id);
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
    let first = store.task_message_attention().unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.items[0].state, "uncertain");
    let first_claim = first.items[0].claim_id.clone();
    let unchanged = state.control_room_notify.notified();
    tokio::pin!(unchanged);
    unchanged.as_mut().enable();
    state.deliver_task_messages(&store, &client).await;
    assert!(
        unchanged.now_or_never().is_none(),
        "an unchanged queue causes no feed wakeup"
    );
    assert_eq!(
        store.task_message_attention().unwrap().items[0].claim_id,
        first_claim
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
    let retry = store.task_message_attention().unwrap();
    assert_ne!(retry.items[0].claim_id, first_claim);
    assert!(
        store.task_messages(task.id).unwrap()[0]
            .delivered_at
            .is_none()
    );
}
