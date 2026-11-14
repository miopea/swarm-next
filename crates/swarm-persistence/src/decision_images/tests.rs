use std::collections::BTreeMap;

use super::*;
use crate::{NewDecisionRequest, OPERATOR_ANSWER_HEADER};
use swarm_domain::{
    DecisionRequestKind, DecisionRequestState, DecisionUrgency, ProviderKind, WorkerSessionId,
};

fn setup() -> (TaskStore, DecisionRequestId) {
    let store = TaskStore::in_memory().unwrap();
    store.ensure_queen("/fictional/queen").unwrap();
    let worker = store
        .create_worker(
            "Requester",
            ProviderKind::ClaudeCode,
            "/fictional/requester",
            false,
            1,
        )
        .unwrap();
    store
        .bind_worker_session(worker.id, WorkerSessionId::new())
        .unwrap();
    let decision = request(&store, worker.id);
    (store, decision)
}

fn request(store: &TaskStore, worker: swarm_domain::WorkerId) -> DecisionRequestId {
    store
        .create_decision_request(&NewDecisionRequest {
            requesting_worker_id: worker,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: "Which fictional layout?",
            summary: "Choose the layout.",
            reason: "A choice is needed.",
            risk: "Fictional risk.",
            evidence: "Fixture only.",
            suggested_action: "Keep it",
            allowed_actions: &["Keep it".into(), "Change it".into()],
            operator_actions: &[],
            questions: &[],
            deadline: None,
            requested_command: None,
            requested_script: None,
        })
        .unwrap()
        .id
}

fn image(byte_len: u64) -> NewDecisionImage {
    NewDecisionImage {
        id: uuid::Uuid::now_v7(),
        media_type: "image/png".into(),
        byte_len,
    }
}

fn own_words() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([(
        OPERATOR_ANSWER_HEADER.to_owned(),
        vec!["Move the button, as in the screenshot".to_owned()],
    )])
}

fn rows(store: &TaskStore) -> usize {
    store
        .connection()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM decision_images", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn images_sent_with_a_ruling_are_recorded_with_it_and_counted_for_delivery() {
    let (store, decision) = setup();
    let sent = [image(10), image(20)];

    let resolved = store
        .resolve_decision_request_with_images(decision, "Change it", "See both", "inbox", &sent)
        .unwrap();

    assert_eq!(resolved.state, DecisionRequestState::Resolved);
    let listed = store.decision_images(&[decision]).unwrap();
    assert_eq!(
        listed.iter().map(|image| image.id).collect::<Vec<_>>(),
        sent.iter().map(|image| image.id).collect::<Vec<_>>(),
        "kept in the order they were sent"
    );
    assert!(listed.iter().all(|image| image.clarification_id.is_none()));
    let dispatch = store.claim_decision_deliveries(1).unwrap();
    assert_eq!(dispatch.len(), 1);
    assert_eq!(dispatch[0].image_count, 2);
    assert_eq!(
        store
            .decision_image(decision, sent[1].id)
            .unwrap()
            .unwrap()
            .byte_len,
        20
    );
}

#[test]
fn images_sent_with_an_answer_in_their_own_words_are_recorded_with_it() {
    let (store, decision) = setup();

    store
        .answer_decision_request_with_images(
            decision,
            &own_words(),
            "",
            "inbox",
            None,
            &[image(10)],
        )
        .unwrap();

    assert_eq!(store.decision_images(&[decision]).unwrap().len(), 1);
    assert_eq!(
        store.claim_decision_deliveries(1).unwrap()[0].image_count,
        1
    );
}

#[test]
fn a_refused_answer_records_none_of_its_images() {
    let (store, decision) = setup();

    let refused = store.resolve_decision_request_with_images(
        decision,
        "Not offered",
        "",
        "inbox",
        &[image(10)],
    );
    assert!(matches!(
        refused,
        Err(TaskStoreError::InvalidDecisionResolution)
    ));
    assert_eq!(rows(&store), 0);

    store
        .resolve_decision_request(decision, "Keep it", "", "inbox")
        .unwrap();
    let late =
        store.resolve_decision_request_with_images(decision, "Keep it", "", "inbox", &[image(10)]);
    assert!(matches!(late, Err(TaskStoreError::DecisionAlreadyResolved)));
    assert_eq!(rows(&store), 0);
}

#[test]
fn refused_images_leave_the_request_unanswered() {
    let (store, decision) = setup();
    let five: Vec<_> = (0..5).map(|_| image(10)).collect();
    let mut wrong_type = image(10);
    wrong_type.media_type = "application/pdf".into();

    for (images, expected) in [
        (five, "too many"),
        (vec![wrong_type], "invalid"),
        (vec![image(0)], "invalid"),
        (vec![image(MAX_DECISION_IMAGE_BYTES + 1)], "invalid"),
    ] {
        let refused =
            store.resolve_decision_request_with_images(decision, "Keep it", "", "inbox", &images);
        match (expected, refused) {
            ("too many", Err(TaskStoreError::TooManyDecisionImages { limit: 4 }))
            | ("invalid", Err(TaskStoreError::InvalidDecisionImage)) => {}
            (_, other) => panic!("{expected}: {other:?}"),
        }
        assert_eq!(
            store.get_decision_request(decision).unwrap().state,
            DecisionRequestState::Pending
        );
    }
    assert_eq!(rows(&store), 0);
}

#[test]
fn a_question_carries_its_images_and_a_repeat_adds_none() {
    let (store, decision) = setup();
    let id = DecisionClarificationId::new();

    let asked = store
        .ask_decision_clarification_with_images(id, decision, "Which one?", &[image(10)], 100)
        .unwrap();
    assert_eq!(asked.image_count, 1);
    let replay = store
        .ask_decision_clarification_with_images(id, decision, "Which one?", &[image(10)], 101)
        .unwrap();
    assert_eq!(replay.image_count, 1);
    assert_eq!(rows(&store), 1);
    let listed = store.decision_images(&[decision]).unwrap();
    assert_eq!(listed[0].clarification_id, Some(id));
    let claimed = store.claim_clarification_deliveries(102).unwrap();
    assert_eq!(claimed[0].clarification.image_count, 1);

    // The answer that follows counts only its own.
    store
        .resolve_decision_request(decision, "Keep it", "", "inbox")
        .unwrap();
    assert_eq!(
        store.claim_decision_deliveries(103).unwrap()[0].image_count,
        0
    );
}

#[test]
fn one_request_holds_at_most_sixteen_images() {
    let (store, decision) = setup();
    for round in 0..4 {
        store
            .ask_decision_clarification_with_images(
                DecisionClarificationId::new(),
                decision,
                "Which one?",
                &[image(10), image(10), image(10), image(10)],
                100 + round,
            )
            .unwrap();
        // One open question at a time: a reply settles each round.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE decision_clarifications SET delivery_state='cancelled' WHERE reply IS NULL",
                [],
            )
            .unwrap();
    }

    let refused =
        store.resolve_decision_request_with_images(decision, "Keep it", "", "inbox", &[image(10)]);
    assert!(matches!(
        refused,
        Err(TaskStoreError::TooManyDecisionImages { limit: 16 })
    ));
    assert_eq!(rows(&store), 16);
}

#[test]
fn a_full_store_refuses_and_makes_room_only_from_closed_history() {
    let (store, held) = setup();
    let worker = store
        .get_decision_request(held)
        .unwrap()
        .requesting_worker_id;
    // Fill the Hive's byte budget with one pending request's worth and the
    // rest on requests answered long ago.
    let per_image = MAX_DECISION_IMAGE_BYTES;
    let images_to_fill = usize::try_from(MAX_DECISION_IMAGE_TOTAL_BYTES / per_image).unwrap();
    store
        .ask_decision_clarification_with_images(
            DecisionClarificationId::new(),
            held,
            "Which one?",
            &[image(per_image)],
            100,
        )
        .unwrap();
    let mut closed = Vec::new();
    let mut left = images_to_fill - 1;
    while left > 0 {
        let batch: Vec<_> = (0..left.min(MAX_IMAGES_PER_SUBMISSION))
            .map(|_| image(per_image))
            .collect();
        left -= batch.len();
        let decision = request(&store, worker);
        store
            .resolve_decision_request_with_images(decision, "Keep it", "", "inbox", &batch)
            .unwrap();
        closed.push(decision);
    }
    let now = super::now();
    let latest = request(&store, worker);

    let full = store.resolve_decision_request_with_images(
        latest,
        "Keep it",
        "",
        "inbox",
        &[image(per_image)],
    );
    assert!(matches!(
        full,
        Err(TaskStoreError::DecisionImageStorageFull)
    ));
    assert_eq!(
        store.get_decision_request(latest).unwrap().state,
        DecisionRequestState::Pending
    );

    // Past retention, closed requests' images give way; the pending one's do not.
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE decision_requests SET resolved_at = ?1 WHERE state = 'resolved'",
            [now - RETAINED_AFTER_CLOSE_SECONDS - 1],
        )
        .unwrap();
    store
        .resolve_decision_request_with_images(latest, "Keep it", "", "inbox", &[image(per_image)])
        .unwrap();
    assert!(store.decision_images(&closed).unwrap().is_empty());
    assert_eq!(store.decision_images(&[held]).unwrap().len(), 1);
    assert_eq!(store.decision_images(&[latest]).unwrap().len(), 1);
}

#[test]
fn kept_ids_are_every_recorded_image() {
    let (store, decision) = setup();
    let sent = [image(10), image(10)];
    store
        .resolve_decision_request_with_images(decision, "Keep it", "", "inbox", &sent)
        .unwrap();

    let kept = store.kept_decision_image_ids().unwrap();

    assert_eq!(
        kept,
        sent.iter()
            .map(|image| image.id)
            .collect::<std::collections::HashSet<_>>()
    );
    assert!(
        store
            .decision_image(DecisionRequestId::new(), sent[0].id)
            .unwrap()
            .is_none()
    );
}
