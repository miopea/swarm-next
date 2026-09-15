use super::*;
use base64ct::{Base64UrlUnpadded, Encoding};
use swarm_domain::{ApiaryId, FederationNodeId};

#[test]
fn outage_backoff_is_durable_and_clears_after_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retry.sqlite");
    let store = TaskStore::open(&path).unwrap();
    let consent = consent(&store);
    store.save_apiary_enrollment(&consent, 10).unwrap();
    store
        .record_apiary_enrollment_attempt(
            consent.link_id,
            Some(swarm_domain::ApiaryEnrollmentProblem::KeeperUnavailable),
            None,
            11,
        )
        .unwrap();
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    let first = store.apiary_enrollments().unwrap().remove(0);
    assert_eq!(first.next_attempt_at, Some(16));
    assert_eq!(first.consecutive_failures, 1);
    store
        .record_apiary_enrollment_attempt(consent.link_id, None, None, 16)
        .unwrap();
    let recovered = store.apiary_enrollments().unwrap().remove(0);
    assert_eq!(recovered.next_attempt_at, None);
    assert_eq!(recovered.problem, None);
    assert_eq!(recovered.consecutive_failures, 0);
}

#[test]
fn permanent_refusal_stops_retry_without_reviving_cancelled_work() {
    let store = TaskStore::in_memory().unwrap();
    let consent = consent(&store);
    store.save_apiary_enrollment(&consent, 10).unwrap();
    store
        .record_apiary_enrollment_attempt(
            consent.link_id,
            Some(swarm_domain::ApiaryEnrollmentProblem::Unclassified),
            None,
            11,
        )
        .unwrap();
    let refused = store.apiary_enrollments().unwrap().remove(0);
    assert_eq!(refused.phase, ApiaryEnrollmentPhase::Attention);
    assert_eq!(refused.next_attempt_at, None);
    store
        .record_apiary_enrollment_attempt(consent.link_id, None, None, 12)
        .unwrap();
    assert_eq!(store.apiary_enrollments().unwrap(), vec![refused]);
}

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
fn removing_a_join_in_progress_is_a_conflict_not_a_missing_link() {
    let store = TaskStore::in_memory().unwrap();
    let consent = consent(&store);
    store.save_apiary_enrollment(&consent, 10).unwrap();
    store
        .advance_apiary_enrollment(
            consent.link_id,
            ApiaryEnrollmentPhase::AwaitingApproval,
            ApiaryEnrollmentPhase::Joining,
        )
        .unwrap();
    assert!(matches!(
        store.remove_local_apiary_keeper_link(consent.link_id),
        Err(TaskStoreError::ApiaryJoinNotReady)
    ));
    assert_eq!(
        store.apiary_enrollments().unwrap()[0].phase,
        ApiaryEnrollmentPhase::Joining
    );
    assert!(matches!(
        store.remove_local_apiary_keeper_link(swarm_domain::ApiaryJoinLinkId::new()),
        Err(TaskStoreError::ApiaryJoinLinkNotFound)
    ));
}

#[test]
fn cancelling_enrollment_retires_imported_invitation_without_reviving_legacy_join() {
    let (member, consent, invitation) = approved_join();
    member
        .remove_local_apiary_keeper_link(consent.link_id)
        .unwrap();
    assert!(member.apiary_enrollments().unwrap().is_empty());
    let state: String = member
        .connection()
        .unwrap()
        .query_row(
            "SELECT state FROM apiary_join_invitations WHERE id = ?1",
            [invitation.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "revoked");
    assert!(
        member
            .prepare_consented_apiary_join(consent.link_id, invitation, 15)
            .is_err()
    );
}

#[test]
fn uncertain_join_cancellation_rolls_back_invitation_changes() {
    let (member, consent, invitation) = approved_join();
    member
        .prepare_consented_apiary_join(consent.link_id, invitation, 15)
        .unwrap();
    assert!(matches!(
        member.remove_local_apiary_keeper_link(consent.link_id),
        Err(TaskStoreError::ApiaryJoinNotReady)
    ));
    let state: String = member
        .connection()
        .unwrap()
        .query_row(
            "SELECT state FROM apiary_join_invitations WHERE id = ?1",
            [invitation.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "policy_accepted");
    assert_eq!(
        member.apiary_enrollments().unwrap()[0].phase,
        ApiaryEnrollmentPhase::Joining
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

/// A member, the Keeper that invited it, and a helper to issue FURTHER
/// invitations for the same Apiary — which is the whole subject of the
/// stranded-invitation fix below.
struct JoinFixture {
    member: TaskStore,
    keeper: TaskStore,
}

impl JoinFixture {
    fn new() -> Self {
        let member = TaskStore::in_memory().unwrap();
        let keeper = TaskStore::in_memory().unwrap();
        keeper
            .create_apiary_for_local_hive("Test garden", swarm_domain::SharedWorkBackend::Jira, 9)
            .unwrap();
        Self { member, keeper }
    }

    /// Carries one invitation all the way to imported, with its enrollment
    /// saved, exactly as a real join does.
    fn invite(&self, at: i64) -> (ApiaryEnrollmentConsent, swarm_domain::ApiaryInvitationId) {
        let card = self.member.issue_hive_connection_card(at, 3600).unwrap();
        let keeper_card = self.keeper.issue_hive_connection_card(at, 3600).unwrap();
        let link = self
            .keeper
            .issue_apiary_join_link("https://keeper.example", at, 3600)
            .unwrap();
        self.member
            .save_local_apiary_keeper_link(
                link.link.id,
                &link.link.keeper_endpoint,
                &link.one_time_secret,
                at,
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
            accepted_at: at,
            expires_at: link.link.expires_at,
        };
        self.member.save_apiary_enrollment(&consent, at).unwrap();
        self.keeper
            .present_apiary_join_link_identity(link.link.id, &link.one_time_secret, &card, at + 1)
            .unwrap();
        self.keeper
            .approve_apiary_join_link(link.link.id, at + 2)
            .unwrap();
        let bundle = self
            .keeper
            .poll_apiary_join_link(link.link.id, &link.one_time_secret, at + 3)
            .unwrap()
            .invitation
            .unwrap();
        let id = bundle.invitation.payload.invitation_id;
        self.member
            .import_apiary_invitation_bundle(&bundle, at + 4)
            .unwrap();
        (consent, id)
    }

    fn invitation_state(&self, id: swarm_domain::ApiaryInvitationId) -> String {
        self.member
            .connection()
            .unwrap()
            .query_row(
                "SELECT state FROM apiary_join_invitations WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .unwrap()
    }
}

/// ⚠️ THE DEADLOCK THIS FIX EXISTS FOR, reproduced.
///
/// Observed on a member Hive 2026-09-14: one `submitted` invitation,
/// `apiary_enrollments` EMPTY, `hives.apiary_id` NULL, and fourteen consecutive
/// invitations from the Keeper that could not be imported. Only
/// `apply_remote_join_acceptance` moves a row out of `submitted`, and it is
/// reachable only through `reconcile_one` <- `pending_enrollments`; with no
/// enrollment nothing can ever advance it. `remove_local_apiary_keeper_link`
/// deliberately refuses to revoke `submitted`, and nothing sweeps it on expiry
/// because the blocking index keys on STATE. The operator is not even offered
/// Cancel, because that button is gated on having an enrollment.
///
/// The enrollment is deleted directly here because that IS the observed state,
/// and it is not reachable through the happy path -- that is what made it a
/// trap rather than a transient.
#[test]
fn a_stranded_invitation_no_longer_blocks_every_future_join() {
    let fixture = JoinFixture::new();
    let (consent, first) = fixture.invite(10);
    fixture
        .member
        .prepare_consented_apiary_join(consent.link_id, first, 20)
        .unwrap();
    fixture
        .member
        .connection()
        .unwrap()
        .execute_batch(
            "UPDATE apiary_join_invitations SET state = 'submitted', submitted_at = 21;
             DELETE FROM apiary_enrollments;",
        )
        .unwrap();
    assert_eq!(fixture.invitation_state(first), "submitted");

    // The Keeper retires the dead link before issuing another, which is what
    // the operator did fourteen times. Its own one-pending-invitation rule
    // requires it, and it is NOT what was blocking the member.
    fixture
        .keeper
        .revoke_apiary_join_link(consent.link_id, 99)
        .unwrap();

    // Before the fix this returned FederationInvitationConflict, forever.
    let (_, second) = fixture.invite(100);

    assert_eq!(fixture.invitation_state(second), "keeper_pinned");
    assert_eq!(
        fixture.invitation_state(first),
        "revoked",
        "the stranded row must be retired, or the unique index still blocks the new one",
    );
}

/// ⚠️ AND THE PROTECTION IT MUST NOT REMOVE. An invitation still backed by an
/// enrollment may be genuinely in flight: the Keeper may already have accepted
/// it and the receipt may still be arriving. Retiring that would discard a
/// membership the Keeper believes exists. The predicate is "nothing can advance
/// this", never "this is old".
#[test]
fn an_invitation_still_backed_by_an_enrollment_is_never_retired() {
    let fixture = JoinFixture::new();
    let (consent, first) = fixture.invite(10);
    fixture
        .member
        .prepare_consented_apiary_join(consent.link_id, first, 20)
        .unwrap();
    fixture
        .member
        .connection()
        .unwrap()
        .execute("UPDATE apiary_join_invitations SET state = 'submitted'", [])
        .unwrap();

    // The enrollment is left in place, so this submission may still resolve.
    fixture
        .keeper
        .revoke_apiary_join_link(consent.link_id, 99)
        .unwrap();
    let card = fixture
        .member
        .issue_hive_connection_card(100, 3600)
        .unwrap();
    let link = fixture
        .keeper
        .issue_apiary_join_link("https://keeper.example", 100, 3600)
        .unwrap();
    fixture
        .member
        .save_local_apiary_keeper_link(
            link.link.id,
            &link.link.keeper_endpoint,
            &link.one_time_secret,
            100,
        )
        .unwrap();
    fixture
        .keeper
        .present_apiary_join_link_identity(link.link.id, &link.one_time_secret, &card, 101)
        .unwrap();
    fixture
        .keeper
        .approve_apiary_join_link(link.link.id, 102)
        .unwrap();
    let bundle = fixture
        .keeper
        .poll_apiary_join_link(link.link.id, &link.one_time_secret, 103)
        .unwrap()
        .invitation
        .unwrap();

    let refused = fixture
        .member
        .import_apiary_invitation_bundle(&bundle, 104)
        .unwrap_err();

    assert!(
        matches!(refused, TaskStoreError::FederationInvitationConflict),
        "a live submission must still hold the slot, got {refused:?}",
    );
    assert_eq!(fixture.invitation_state(first), "submitted");
}

/// ⚠️ AN UNCLASSIFIED FAILURE MUST CARRY ITS CODE ALL THE WAY TO STORAGE.
///
/// The classified variants each name a cause somebody established.
/// `Unclassified` names none, and before this the transport code was discarded
/// at the match arm that produced it — so the member's screen fell back to "the
/// approved invitation no longer matches your submitted terms", a fluent
/// sentence about a cause nobody had checked. On 2026-09-14 that sent an
/// operator looking at terms that were fine while the real block was a stranded
/// invitation.
///
/// Also asserts the code is CLEARED on recovery, because a stale code beside a
/// healthy enrollment is its own small lie.
#[test]
fn an_unclassified_failure_keeps_the_code_and_a_recovery_clears_it() {
    let store = TaskStore::in_memory().unwrap();
    let stuck = consent(&store);
    store.save_apiary_enrollment(&stuck, 10).unwrap();

    store
        .record_apiary_enrollment_attempt(
            stuck.link_id,
            Some(swarm_domain::ApiaryEnrollmentProblem::Unclassified),
            Some("apiary_join_not_ready (409)".to_owned()),
            11,
        )
        .unwrap();

    let stored = store.apiary_enrollments().unwrap();
    let record = stored.first().expect("the enrollment survives the attempt");
    assert_eq!(
        record.problem,
        Some(swarm_domain::ApiaryEnrollmentProblem::Unclassified),
    );
    assert_eq!(
        record.problem_code.as_deref(),
        Some("apiary_join_not_ready (409)"),
        "the code is the only thing that can tell anyone what actually failed",
    );

    // ⚠️ AND THIS ONE CANNOT BE CLEARED, which is worth asserting rather than
    // discovering. `record_attempt` returns early unless the phase is
    // AwaitingApproval or Joining, and an Unclassified failure has already moved
    // it to Attention — so the record is frozen with its code, exactly as the
    // no-retry policy intends. Written as an assertion because the first draft
    // of this test assumed a later success would clear it, and it does not.
    store
        .record_apiary_enrollment_attempt(stuck.link_id, None, None, 12)
        .unwrap();
    assert_eq!(
        store.apiary_enrollments().unwrap().first().unwrap().problem,
        Some(swarm_domain::ApiaryEnrollmentProblem::Unclassified),
        "Attention is terminal; a later observation does not revive the record",
    );

    // Clearing is exercised where it can happen: a retryable failure, which
    // leaves the enrollment in AwaitingApproval.
    let retryable = TaskStore::in_memory().unwrap();
    let retry_consent = consent(&retryable);
    retryable
        .save_apiary_enrollment(&retry_consent, 10)
        .unwrap();
    retryable
        .record_apiary_enrollment_attempt(
            retry_consent.link_id,
            Some(swarm_domain::ApiaryEnrollmentProblem::KeeperUnavailable),
            Some("keeper_unavailable (503)".to_owned()),
            11,
        )
        .unwrap();
    retryable
        .record_apiary_enrollment_attempt(retry_consent.link_id, None, None, 12)
        .unwrap();

    let record = retryable.apiary_enrollments().unwrap();
    let record = record.first().expect("still present");
    assert_eq!(record.problem, None);
    assert_eq!(
        record.problem_code, None,
        "a cleared problem must not leave its code behind",
    );
}
