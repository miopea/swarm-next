use super::*;
use base64ct::{Base64UrlUnpadded, Encoding};
use swarm_domain::{ApiaryId, FederationNodeId};

#[test]
fn keeper_link_disclosure_is_signed_before_submission_and_rejects_tampering() {
    let keeper = TaskStore::in_memory().unwrap();
    keeper
        .create_apiary_for_local_hive("Test garden", swarm_domain::SharedWorkBackend::Jira, 9)
        .unwrap();
    let link = keeper
        .issue_apiary_join_link("https://keeper.example", 10, 3600)
        .unwrap();
    let offer = link.enrollment_offer.unwrap();
    crate::verify_apiary_enrollment_offer(&offer, 11).unwrap();
    assert_eq!(offer.payload.link_id, link.link.id);
    assert_eq!(offer.payload.apiary_id, link.link.apiary_id);
    assert_eq!(offer.payload.policy_revision, 1);
    assert!(crate::verify_apiary_enrollment_offer(&offer, offer.payload.expires_at).is_err());
    let changes: [fn(&mut swarm_domain::ApiaryEnrollmentOfferPayload); 5] = [
        |p| p.policy_revision += 1,
        |p| p.link_id = ApiaryJoinLinkId::new(),
        |p| p.keeper_endpoint = "https://different.example".into(),
        |p| p.apiary_name = "Different garden".into(),
        |p| p.management_terms_version = 2,
    ];
    for change in changes {
        let mut altered = offer.clone();
        change(&mut altered.payload);
        assert!(crate::verify_apiary_enrollment_offer(&altered, 11).is_err());
    }
}

fn approved_join() -> (
    TaskStore,
    ApiaryEnrollmentConsent,
    swarm_domain::ApiaryInvitationId,
) {
    let member = TaskStore::in_memory().unwrap();
    let card = member.issue_hive_connection_card(10, 3600).unwrap();
    let keeper = TaskStore::in_memory().unwrap();
    keeper
        .create_apiary_for_local_hive("Test garden", swarm_domain::SharedWorkBackend::Jira, 9)
        .unwrap();
    let keeper_card = keeper.issue_hive_connection_card(10, 3600).unwrap();
    let link = keeper
        .issue_apiary_join_link("https://keeper.example", 10, 3600)
        .unwrap();
    member
        .save_local_apiary_keeper_link(
            link.link.id,
            &link.link.keeper_endpoint,
            &link.one_time_secret,
            10,
        )
        .unwrap();
    let consent = ApiaryEnrollmentConsent {
        link_id: link.link.id,
        apiary_id: link.link.apiary_id,
        keeper_node_id: keeper_card.payload.node_id,
        member_node_id: card.payload.node_id,
        member_hive_id: card.payload.hive_id,
        member_operator_id: card.payload.operator_id,
        policy_revision: 1,
        accepted_at: 10,
        expires_at: link.link.expires_at,
    };
    member.save_apiary_enrollment(&consent, 10).unwrap();
    keeper
        .present_apiary_join_link_identity(link.link.id, &link.one_time_secret, &card, 11)
        .unwrap();
    keeper.approve_apiary_join_link(link.link.id, 12).unwrap();
    let approved = keeper
        .poll_apiary_join_link(link.link.id, &link.one_time_secret, 13)
        .unwrap();
    let invitation = approved.invitation.unwrap();
    let id = invitation.invitation.payload.invitation_id;
    member
        .import_apiary_invitation_bundle(&invitation, 14)
        .unwrap();
    (member, consent, id)
}

#[test]
fn keeper_approval_reuses_prior_consent_atomically_and_retries_without_second_acceptance() {
    let (member, consent, invitation) = approved_join();
    let prepared = member
        .prepare_consented_apiary_join(consent.link_id, invitation, 15)
        .unwrap();
    assert_eq!(prepared.phase, ApiaryEnrollmentPhase::Joining);
    assert_eq!(
        member
            .prepare_consented_apiary_join(consent.link_id, invitation, 16)
            .unwrap(),
        prepared
    );
    let connection = member.connection().unwrap();
    let state: (String, i64) = connection
        .query_row(
            "SELECT state, policy_accepted_at FROM apiary_join_invitations WHERE id = ?1",
            [invitation.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, ("policy_accepted".into(), consent.accepted_at));
}

#[test]
fn cancelled_or_unrelated_link_cannot_accept_an_approved_invitation() {
    let (member, original, invitation) = approved_join();
    let other = consent(&member);
    member.save_apiary_enrollment(&other, 14).unwrap();
    assert!(
        member
            .prepare_consented_apiary_join(other.link_id, invitation, 15)
            .is_err()
    );
    member
        .advance_apiary_enrollment(
            original.link_id,
            ApiaryEnrollmentPhase::AwaitingApproval,
            ApiaryEnrollmentPhase::Cancelled,
        )
        .unwrap();
    assert!(
        member
            .prepare_consented_apiary_join(original.link_id, invitation, 15)
            .is_err()
    );
    let state: String = member
        .connection()
        .unwrap()
        .query_row(
            "SELECT state FROM apiary_join_invitations WHERE id = ?1",
            [invitation.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "keeper_pinned");
}

#[test]
fn journal_failure_rolls_back_policy_acceptance_and_retry_recovers() {
    let (member, consent, invitation) = approved_join();
    member
        .connection()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_enrollment_update BEFORE UPDATE ON apiary_enrollments
         BEGIN SELECT RAISE(ABORT, 'injected journal failure'); END;",
        )
        .unwrap();
    assert!(
        member
            .prepare_consented_apiary_join(consent.link_id, invitation, 15)
            .is_err()
    );
    let state: String = member
        .connection()
        .unwrap()
        .query_row(
            "SELECT state FROM apiary_join_invitations WHERE id = ?1",
            [invitation.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "keeper_pinned");
    assert_eq!(
        member.apiary_enrollments().unwrap()[0].phase,
        ApiaryEnrollmentPhase::AwaitingApproval
    );
    member
        .connection()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_enrollment_update;")
        .unwrap();
    assert!(
        member
            .prepare_consented_apiary_join(consent.link_id, invitation, 16)
            .is_ok()
    );
}

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
