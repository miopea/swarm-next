//! Opt-in real Admin-route acceptance. Never runs against a production origin.
use super::{SupportTransportResult, receive, receive_attachments, transport_client};
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

fn fictional_file(bytes: &[u8]) -> swarm_domain::SupportAttachment {
    use sha2::{Digest, Sha256};
    use swarm_domain::{SupportAttachment, SupportAttachmentMetadata};
    SupportAttachment::validate(
        SupportAttachmentMetadata {
            id: "00000000-0000-4000-8000-000000000001".parse().unwrap(),
            file_name: "fictional-diagnostic.txt".into(),
            media_type: "text/plain".into(),
            size_bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        },
        bytes.to_vec(),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "requires an explicitly started fictional Admin loopback fixture"]
async fn admin_fixture_recovers_native_files_after_hive_restart() {
    let endpoint = std::env::var("SWARM_TEST_ADMIN_ATTACHMENTS_ENDPOINT").unwrap();
    let url = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(url.port().is_some());
    assert!(url.username().is_empty() && url.password().is_none());
    assert!(url.query().is_none() && url.fragment().is_none());
    assert_eq!(
        url.path(),
        "/api/feedback/swarm-support/submissions-with-attachments"
    );
    let files = vec![fictional_file(
        b"Fictional diagnostic only. No customer information.\n",
    )];
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        & 0xffff_ffff_ffff;
    let input: SupportSubmissionInput = serde_json::from_value(serde_json::json!({
        "submission_key": format!("00000000-0000-4000-8000-{nonce:012x}"),
        "kind": "bug_report", "email": "paired-files@example.invalid",
        "name": "Fictional File Operator", "subject": "Fictional native file replay",
        "body": "Isolated acceptance only. No task execution or customer sends."
    }))
    .unwrap();
    let key = input.submission_key;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fictional-files.db");
    let destination = SupportDestination::parse("https://admin.example.invalid").unwrap();
    let client = transport_client(Duration::from_secs(10)).unwrap();
    let (frozen, original) = {
        let hive = HiveSupportService::new(TaskStore::open(&path).unwrap(), destination.clone());
        hive.submit_reviewed_with_attachments(input, &files, 1)
            .unwrap();
        let claim = hive.claim(key, 2).unwrap();
        assert!(claim.attachments == files);
        let Some(SupportTransportResult::Receipt(receipt)) = receive_attachments(
            &client,
            &endpoint,
            &claim.frozen_submission,
            &claim.attachments,
        )
        .await
        else {
            panic!("fictional Admin did not accept native files");
        };
        assert!(!receipt.deduplicated);
        // Lose the response before local settlement; reopen the durable outbox.
        (claim.frozen_submission, receipt)
    };
    let hive = HiveSupportService::new(TaskStore::open(&path).unwrap(), destination);
    assert_eq!(hive.recover_interrupted(3).unwrap(), 1);
    let claim = hive.claim(key, 4).unwrap();
    assert_eq!(claim.frozen_submission, frozen);
    assert!(claim.attachments == files);
    let Some(SupportTransportResult::Receipt(replayed)) = receive_attachments(
        &client,
        &endpoint,
        &claim.frozen_submission,
        &claim.attachments,
    )
    .await
    else {
        panic!("fictional Admin did not replay native files");
    };
    assert!(replayed.deduplicated);
    assert_eq!(replayed.conversation_id, original.conversation_id);
    assert_eq!(replayed.message_id, original.message_id);
    assert_eq!(replayed.created_at, original.created_at);
    hive.settle(key, claim.delivery.attempt_id.unwrap(), Some(replayed), 5)
        .unwrap();
    assert_eq!(
        hive.statuses().unwrap()[0].delivery.state,
        SupportDeliveryState::Confirmed
    );
    assert!(hive.retryable_keys().unwrap().is_empty());
    assert!(matches!(
        receive_attachments(
            &client,
            &endpoint,
            &frozen,
            &[fictional_file(b"Changed fictional bytes")]
        )
        .await,
        Some(SupportTransportResult::Refused {
            reason: SupportRefusal::Conflict,
            ..
        })
    ));
    let Some(SupportTransportResult::Receipt(unchanged)) =
        receive_attachments(&client, &endpoint, &frozen, &files).await
    else {
        panic!("changed-file conflict damaged the original receipt");
    };
    assert_eq!(unchanged.message_id, original.message_id);
    eprintln!(
        "Native Admin pairing passed: durable files, lost receipt, restart, exact replay, changed-file conflict"
    );
}

/// The authorised endpoint, or a refusal. Separated so the pinning is one thing
/// a reader can check without reading the probe around it.
fn pinned_production_endpoint() -> String {
    let endpoint = std::env::var("SWARM_TEST_ADMIN_PRODUCTION_ATTACHMENTS_ENDPOINT")
        .expect("set the authorised production endpoint explicitly");
    let url = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(url.scheme(), "https", "production probe must be HTTPS");
    assert_eq!(
        url.host_str(),
        Some("admin.bfgsolutions.net"),
        "only the host named in the operator decision may be probed"
    );
    assert!(url.username().is_empty() && url.password().is_none());
    assert!(url.query().is_none() && url.fragment().is_none());
    assert_eq!(
        url.path(),
        "/api/feedback/swarm-support/submissions-with-attachments"
    );
    endpoint
}

/// A submission whose every field says what it is and asks for its own deletion.
fn labelled_fictional_submission() -> SupportSubmissionInput {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        & 0xffff_ffff_ffff;
    serde_json::from_value(serde_json::json!({
        "submission_key": format!("00000000-0000-4000-8000-{nonce:012x}"),
        "kind": "bug_report", "email": "fictional-replay@example.invalid",
        "name": "FICTIONAL replay probe - delete after",
        "subject": "FICTIONAL: authorised replay contract check, please delete",
        "body": "Fictional acceptance evidence only. Authorised by operator decision 01a0bc97-8b75-7971-b9e3-524a29ad572f; BFG Admin notified on 01a0bca2-016b. No customer data, no reply expected, please delete."
    }))
    .unwrap()
}

/// The one authorised production replay, pinned so it cannot reach anywhere else.
///
/// ⚠️ THIS WRITES TO A THIRD PARTY'S LIVE SYSTEM and is inert without an
/// explicit environment variable. Operator decision
/// `01a0bc97-8b75-7971-b9e3-524a29ad572f`, answered `chose_an_offered_action`
/// with no condition: "Authorise a bounded fictional replay for line 2; scope
/// line 3 out". BFG Admin were told before the fact on task
/// `01a0bca2-016b-79b1-98d0-2c92081d9e73`, and the record is to be deleted after.
///
/// The host is pinned as tightly as the loopback tests pin 127.0.0.1, for the
/// same reason: a probe that can be re-aimed by changing one variable is a
/// probe that will eventually be aimed somewhere nobody authorised.
#[tokio::test]
#[ignore = "writes to BFG Admin production; requires an explicit authorised endpoint"]
async fn admin_production_honours_the_replay_contract_once() {
    let endpoint = pinned_production_endpoint();
    let files = vec![fictional_file(
        b"FICTIONAL acceptance evidence, authorised by operator decision 01a0bc97-8b75. Delete after. No customer information.\n",
    )];
    let input = labelled_fictional_submission();
    let key = input.submission_key;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("authorised-replay.db");
    let destination = SupportDestination::parse("https://admin.bfgsolutions.net").unwrap();
    let client = transport_client(Duration::from_secs(20)).unwrap();

    let hive = HiveSupportService::new(TaskStore::open(&path).unwrap(), destination);
    hive.submit_reviewed_with_attachments(input, &files, 1)
        .unwrap();
    let claim = hive.claim(key, 2).unwrap();

    // 1. First send.
    let Some(SupportTransportResult::Receipt(original)) = receive_attachments(
        &client,
        &endpoint,
        &claim.frozen_submission,
        &claim.attachments,
    )
    .await
    else {
        panic!("production route did not accept the fictional submission");
    };
    assert!(!original.deduplicated, "a first send is not a duplicate");
    eprintln!(
        "1 FIRST SEND      submission_key={key} conversation={} message={} deduplicated={}",
        original.conversation_id, original.message_id, original.deduplicated
    );

    // 2. Byte-identical replay of the same key.
    let Some(SupportTransportResult::Receipt(replayed)) = receive_attachments(
        &client,
        &endpoint,
        &claim.frozen_submission,
        &claim.attachments,
    )
    .await
    else {
        panic!("production route did not replay the same key");
    };
    eprintln!(
        "2 IDENTICAL REPLAY conversation={} message={} deduplicated={}",
        replayed.conversation_id, replayed.message_id, replayed.deduplicated
    );
    assert!(
        replayed.deduplicated,
        "an identical replay must deduplicate"
    );
    assert_eq!(replayed.message_id, original.message_id);
    assert_eq!(replayed.conversation_id, original.conversation_id);

    // 3. Same key, changed content.
    let conflict = receive_attachments(
        &client,
        &endpoint,
        &claim.frozen_submission,
        &[fictional_file(
            b"CHANGED fictional bytes - conflict probe, delete after.\n",
        )],
    )
    .await;
    // Described rather than Debug-printed: SupportTransportResult is a
    // production type and deriving Debug on it to satisfy one probe would be
    // widening a public surface for a test's convenience.
    eprintln!(
        "3 CHANGED CONTENT  {}",
        match &conflict {
            Some(SupportTransportResult::Refused { reason, .. }) => format!("Refused({reason:?})"),
            Some(SupportTransportResult::Receipt(_)) => "Receipt - NOT refused".to_owned(),
            Some(SupportTransportResult::Uncertain) => "Uncertain".to_owned(),
            None => "no result".to_owned(),
        }
    );
    assert!(
        matches!(
            conflict,
            Some(SupportTransportResult::Refused {
                reason: SupportRefusal::Conflict,
                ..
            })
        ),
        "changed content under a used key must be refused as a conflict"
    );

    // The original must survive the conflict untouched.
    let Some(SupportTransportResult::Receipt(unchanged)) = receive_attachments(
        &client,
        &endpoint,
        &claim.frozen_submission,
        &claim.attachments,
    )
    .await
    else {
        panic!("the conflict probe damaged the original receipt");
    };
    assert_eq!(unchanged.message_id, original.message_id);
    eprintln!("4 ORIGINAL INTACT  message={}", unchanged.message_id);
    eprintln!("DELETE THIS RECORD: submission_key={key}");
}
