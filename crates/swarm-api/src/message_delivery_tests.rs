use super::{AppState, HostClient, TaskStore};
use swarm_domain::{ProviderKind, WorkerSessionId};
use swarm_persistence::MessageEnd;

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
    state.deliver_task_messages(&store, &client).await;
    let first = store.task_message_attention().unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.items[0].state, "uncertain");
    let first_claim = first.items[0].claim_id.clone();
    state.deliver_task_messages(&store, &client).await;
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
