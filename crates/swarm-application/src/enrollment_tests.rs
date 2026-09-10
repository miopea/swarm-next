use super::*;

#[test]
fn submitted_offer_is_durable_retry_stable_and_approval_needs_no_second_consent() {
    let keeper = ApiaryService::new(TaskStore::in_memory().unwrap());
    keeper
        .store
        .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9)
        .unwrap();
    let link = keeper
        .create_join_link("https://keeper.example", 10)
        .unwrap();
    let offer = link.enrollment_offer.unwrap();
    let member = ApiaryService::new(TaskStore::in_memory().unwrap());
    let submitted = member
        .begin_consented_enrollment(&offer, &link.one_time_secret, 11)
        .unwrap();
    assert_eq!(
        member
            .begin_consented_enrollment(&offer, &link.one_time_secret, 12)
            .unwrap(),
        submitted
    );
    assert!(
        member
            .begin_consented_enrollment(&offer, "different-secret", 12)
            .is_err()
    );
    let card = member.connection_card(12).unwrap();
    keeper
        .present_join_link_identity(link.link.id, &link.one_time_secret, &card, 12)
        .unwrap();
    keeper.approve_join_link(link.link.id, 13).unwrap();
    let poll = keeper
        .poll_join_link(link.link.id, &link.one_time_secret, 14)
        .unwrap();
    let bundle = poll.invitation.unwrap();
    member.import_invitation(&bundle, 15).unwrap();
    let prepared = member
        .prepare_consented_join(link.link.id, bundle.invitation.payload.invitation_id, 15)
        .unwrap();
    assert_eq!(prepared.phase, swarm_domain::ApiaryEnrollmentPhase::Joining);
    assert_eq!(prepared.consent.accepted_at, 11);
    // This test intentionally stops before transport: automatic runtime
    // reconciliation is a separate acceptance gate, not proved by these calls.
}

#[test]
fn altered_disclosure_leaves_no_saved_capability_or_consent() {
    let keeper = ApiaryService::new(TaskStore::in_memory().unwrap());
    keeper
        .store
        .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, 9)
        .unwrap();
    let link = keeper
        .create_join_link("https://keeper.example", 10)
        .unwrap();
    let mut offer = link.enrollment_offer.unwrap();
    offer.payload.policy_revision += 1;
    let member = ApiaryService::new(TaskStore::in_memory().unwrap());
    assert!(
        member
            .begin_consented_enrollment(&offer, &link.one_time_secret, 11)
            .is_err()
    );
    assert!(member.keeper_links().unwrap().is_empty());
    assert!(member.store.apiary_enrollments().unwrap().is_empty());
}
