use super::*;
use base64ct::{Base64UrlUnpadded, Encoding};
use swarm_domain::{ApiaryId, FederationNodeId};

fn consent(store: &TaskStore) -> ApiaryEnrollmentConsent {
    let card = store.issue_hive_connection_card(10, 3600).unwrap();
    let consent = ApiaryEnrollmentConsent {
        link_id: ApiaryJoinLinkId::new(),
        apiary_id: ApiaryId::new(),
        keeper_node_id: FederationNodeId::new(),
        member_node_id: card.payload.node_id,
        member_hive_id: card.payload.hive_id,
        member_operator_id: card.payload.operator_id,
        policy_revision: 1,
        accepted_at: 10,
        expires_at: 100,
    };
    store
        .save_local_apiary_keeper_link(
            consent.link_id,
            "https://keeper.example",
            &Base64UrlUnpadded::encode_string(&[7; 32]),
            10,
        )
        .unwrap();
    consent
}

#[test]
fn saved_consent_and_phase_survive_reopen_without_browser() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hive.sqlite");
    let store = TaskStore::open(&path).unwrap();
    let consent = consent(&store);
    let saved = store.save_apiary_enrollment(&consent, 10).unwrap();
    assert_eq!(store.save_apiary_enrollment(&consent, 11).unwrap(), saved);
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    assert_eq!(store.apiary_enrollments().unwrap(), vec![saved]);
    let joining = store
        .advance_apiary_enrollment(
            consent.link_id,
            ApiaryEnrollmentPhase::AwaitingApproval,
            ApiaryEnrollmentPhase::Joining,
        )
        .unwrap();
    assert_eq!(store.save_apiary_enrollment(&consent, 12).unwrap(), joining);
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    assert_eq!(store.apiary_enrollments().unwrap(), vec![joining.clone()]);
    assert_eq!(
        store
            .advance_apiary_enrollment(
                consent.link_id,
                ApiaryEnrollmentPhase::AwaitingApproval,
                ApiaryEnrollmentPhase::Joining
            )
            .unwrap(),
        joining
    );
}

#[test]
fn consent_cannot_be_replaced_expired_or_bound_to_another_local_node() {
    let store = TaskStore::in_memory().unwrap();
    let original = consent(&store);
    store.save_apiary_enrollment(&original, 10).unwrap();
    let mut changed = original.clone();
    changed.policy_revision = 2;
    assert!(store.save_apiary_enrollment(&changed, 11).is_err());
    changed = original.clone();
    changed.member_node_id = FederationNodeId::new();
    assert!(store.save_apiary_enrollment(&changed, 11).is_err());
    assert!(store.save_apiary_enrollment(&original, 100).is_err());
    assert_eq!(store.apiary_enrollments().unwrap()[0].consent, original);
}

#[test]
fn cancellation_blocks_stale_advancement_and_link_removal_cleans_journal() {
    let store = TaskStore::in_memory().unwrap();
    let consent = consent(&store);
    store.save_apiary_enrollment(&consent, 10).unwrap();
    store
        .advance_apiary_enrollment(
            consent.link_id,
            ApiaryEnrollmentPhase::AwaitingApproval,
            ApiaryEnrollmentPhase::Cancelled,
        )
        .unwrap();
    assert!(
        store
            .advance_apiary_enrollment(
                consent.link_id,
                ApiaryEnrollmentPhase::AwaitingApproval,
                ApiaryEnrollmentPhase::Joining
            )
            .is_err()
    );
    assert!(
        store
            .advance_apiary_enrollment(
                consent.link_id,
                ApiaryEnrollmentPhase::Cancelled,
                ApiaryEnrollmentPhase::Joining
            )
            .is_err()
    );
    store
        .remove_local_apiary_keeper_link(consent.link_id)
        .unwrap();
    assert!(store.apiary_enrollments().unwrap().is_empty());
}

#[test]
fn journal_is_bounded_and_existing_retries_work_at_capacity() {
    let store = TaskStore::in_memory().unwrap();
    let first = consent(&store);
    store.save_apiary_enrollment(&first, 10).unwrap();
    for _ in 1..MAX_ENROLLMENTS {
        store.save_apiary_enrollment(&consent(&store), 10).unwrap();
    }
    assert!(store.save_apiary_enrollment(&consent(&store), 10).is_err());
    assert!(store.save_apiary_enrollment(&first, 11).is_ok());
    assert_eq!(
        store.apiary_enrollments().unwrap().len(),
        usize::try_from(MAX_ENROLLMENTS).unwrap()
    );
}

#[test]
fn migration_preserves_existing_links_without_inventing_consent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("upgrade.sqlite");
    let store = TaskStore::open(&path).unwrap();
    let consent = consent(&store);
    {
        let connection = store.connection().unwrap();
        connection
            .execute_batch("DROP TABLE apiary_enrollments; PRAGMA user_version = 164;")
            .unwrap();
    }
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    assert!(store.apiary_enrollments().unwrap().is_empty());
    assert_eq!(
        store.local_apiary_keeper_links().unwrap()[0].link_id,
        consent.link_id
    );
    assert!(store.save_apiary_enrollment(&consent, 11).is_ok());
}
