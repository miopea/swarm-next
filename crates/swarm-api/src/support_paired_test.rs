//! Opt-in real Admin-route acceptance. Never runs against a production origin.
use super::{SupportTransportResult, receive, transport_client};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use swarm_application::{HiveSupportService, SupportDestination};
use swarm_domain::{SupportDeliveryState, SupportSubmissionInput};
use swarm_persistence::{SupportRefusal, TaskStore};

#[tokio::test]
#[ignore = "requires an explicitly started fictional Admin loopback fixture"]
async fn admin_fixture_recovers_frozen_submission_after_hive_restart() {
    let endpoint = std::env::var("SWARM_TEST_ADMIN_FEEDBACK_ENDPOINT")
        .expect("set the fictional Admin loopback endpoint explicitly");
    let url = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(url.port().is_some());
    assert!(url.username().is_empty() && url.password().is_none());
    assert!(url.query().is_none() && url.fragment().is_none());
    assert_eq!(url.path(), "/api/feedback/swarm-support/submissions");

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        & 0xffff_ffff_ffff;
    let input: SupportSubmissionInput = serde_json::from_value(serde_json::json!({
        "submission_key": format!("00000000-0000-4000-8000-{nonce:012x}"),
        "kind": "bug_report", "email": "paired-client@example.invalid",
        "name": "Fictional Hive Operator", "subject": "Fictional paired replay check",
        "body": "Fictional acceptance only. No customer data or task execution."
    }))
    .unwrap();
    let key = input.submission_key;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fictional-hive.db");
    let destination = SupportDestination::parse("https://admin.example.invalid").unwrap();
    let client = transport_client(Duration::from_secs(5)).unwrap();
    let (frozen, original) = {
        let hive = HiveSupportService::new(TaskStore::open(&path).unwrap(), destination.clone());
        hive.submit_reviewed(input, 1).unwrap();
        let claim = hive.claim(key, 2).unwrap();
        // The same production HTTP receive path, redirected only inside this test
        // to the explicit loopback fixture. Production destination validation stays HTTPS.
        let Some(SupportTransportResult::Receipt(receipt)) =
            receive(&client, &endpoint, &claim.frozen_submission).await
        else {
            panic!("fictional Admin did not accept the initial submission");
        };
        assert!(!receipt.deduplicated);
        assert_eq!(receipt.submission_key, key.to_string());
        // Simulate process loss after remote commit but before durable settlement.
        // Drop every Hive handle; do not mark the claim successful or generate a new key.
        (claim.frozen_submission, receipt)
    };

    let hive = HiveSupportService::new(TaskStore::open(&path).unwrap(), destination);
    assert_eq!(hive.recover_interrupted(3).unwrap(), 1);
    let claim = hive.claim(key, 4).unwrap();
    assert_eq!(claim.frozen_submission, frozen);
    let Some(SupportTransportResult::Receipt(replayed)) =
        receive(&client, &endpoint, &claim.frozen_submission).await
    else {
        panic!("fictional Admin did not recover the original receipt");
    };
    assert!(replayed.deduplicated);
    assert_eq!(replayed.conversation_id, original.conversation_id);
    assert_eq!(replayed.message_id, original.message_id);
    assert_eq!(replayed.created_at, original.created_at);
    hive.settle(key, claim.delivery.attempt_id.unwrap(), Some(replayed), 5)
        .unwrap();
    let status = hive.statuses().unwrap().remove(0);
    assert_eq!(status.delivery.state, SupportDeliveryState::Confirmed);
    assert_eq!(status.delivery.attempts, 2);
    assert!(hive.retryable_keys().unwrap().is_empty());

    let mut changed: serde_json::Value = serde_json::from_str(&frozen).unwrap();
    changed["body"] = "Changed fictional content under the same key".into();
    assert!(matches!(
        receive(&client, &endpoint, &changed.to_string()).await,
        Some(SupportTransportResult::Refused {
            reason: SupportRefusal::Conflict,
            ..
        })
    ));
    let Some(SupportTransportResult::Receipt(unchanged)) =
        receive(&client, &endpoint, &frozen).await
    else {
        panic!("conflict must not destroy the original receipt");
    };
    assert_eq!(unchanged.conversation_id, original.conversation_id);
    assert_eq!(unchanged.message_id, original.message_id);
    eprintln!(
        "Admin paired acceptance: frozen replay, Hive reopen, receipt identity and conflict preservation passed"
    );
}
