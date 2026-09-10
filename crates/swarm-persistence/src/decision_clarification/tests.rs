use super::*;
use crate::NewDecisionRequest;
use swarm_domain::{ClarificationDeliveryOutcome as Outcome, PresenceDeviceId};
use swarm_domain::{DecisionRequestKind, DecisionUrgency, ProviderKind};

fn notification_receipts(store: &TaskStore) -> i64 {
    store
        .connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM decision_clarification_notification_receipts",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn clarification_notifications_quiet_while_waiting_and_return_once_per_reply() {
    use swarm_domain::{NotificationPolicy, PresenceDeviceClass, PresenceMode};
    let (store, decision, worker, session) = setup();
    let now = store.get_decision_request(decision).unwrap().created_at + 20;
    let device = PresenceDeviceId::new();
    store
        .save_notification_subscription(
            &crate::notifications::PushSubscriptionInput {
                device_id: device,
                device_class: PresenceDeviceClass::Mobile,
                endpoint: "https://fcm.googleapis.com/fcm/send/fictional-clarification".into(),
                p256dh: vec![7; 65],
                auth: vec![9; 16],
            },
            now,
        )
        .unwrap();
    store
        .set_notification_policy(NotificationPolicy::AllDecisions, now)
        .unwrap();
    store
        .set_manual_presence(Some(PresenceMode::Reachable), now)
        .unwrap();
    store.sweep_attention_notifications(now).unwrap();
    let initial = store.claim_notification_deliveries(now).unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].decision_id, Some(decision));
    store
        .complete_notification_delivery(initial[0].delivery_id, device)
        .unwrap();

    // Browser-supplied UUIDs need not sort in admission order.
    let first: DecisionClarificationId = "ffffffff-ffff-4fff-bfff-ffffffffffff".parse().unwrap();
    store
        .ask_decision_clarification(first, decision, "Please explain.", now + 1)
        .unwrap();
    assert_eq!(store.sweep_attention_notifications(now + 2).unwrap(), 0);
    assert!(
        store
            .claim_notification_deliveries(now + 2)
            .unwrap()
            .is_empty()
    );
    store
        .reply_decision_clarification(first, worker, session, "Explanation one.", now + 3)
        .unwrap();
    assert_eq!(store.sweep_attention_notifications(now + 4).unwrap(), 1);
    let returned = store.claim_notification_deliveries(now + 4).unwrap();
    assert_eq!(returned.len(), 1);
    assert_eq!(
        returned[0].decision_id,
        Some(decision),
        "reply cycle preserves the original decision link"
    );
    store
        .complete_notification_delivery(returned[0].delivery_id, device)
        .unwrap();
    assert_eq!(store.sweep_attention_notifications(now + 5).unwrap(), 0);

    // No sweep between question and reply: the old delivered subject must not
    // suppress a distinct reply cycle. Even equal reply timestamps are safe.
    let second: DecisionClarificationId = "00000000-0000-4000-8000-000000000001".parse().unwrap();
    store
        .ask_decision_clarification(second, decision, "And the alternative?", now + 3)
        .unwrap();
    store
        .reply_decision_clarification(second, worker, session, "Explanation two.", now + 3)
        .unwrap();
    assert_eq!(
        store
            .decision_clarifications(decision)
            .unwrap()
            .iter()
            .map(|round| round.id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(store.sweep_attention_notifications(now + 6).unwrap(), 1);
    let returned = store.claim_notification_deliveries(now + 6).unwrap();
    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].decision_id, Some(decision));
    store
        .complete_notification_delivery(returned[0].delivery_id, device)
        .unwrap();
    store
        .reply_decision_clarification(second, worker, session, "Explanation two.", now + 7)
        .unwrap();
    assert_eq!(store.sweep_attention_notifications(now + 7).unwrap(), 0);
    store
        .resolve_decision_request(decision, "Wait", "Final choice", "test")
        .unwrap();
    assert_eq!(store.sweep_attention_notifications(now + 8).unwrap(), 0);
    assert!(
        store
            .claim_notification_deliveries(now + 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(notification_receipts(&store), 2);
    store.remove_notification_subscription(device).unwrap();
    assert_eq!(
        notification_receipts(&store),
        0,
        "removing a subscription prunes its receipts"
    );
}

#[test]
fn clarification_summary_tracks_next_mover_without_resolving_permission() {
    use swarm_domain::ClarificationNextMove;
    let (store, decision, worker, session) = setup();
    assert!(store.decision_clarification_summaries().unwrap().is_empty());
    let first = DecisionClarificationId::new();
    store
        .ask_decision_clarification(first, decision, "Why?", 100)
        .unwrap();
    let read = || {
        store
            .decision_clarification_summaries()
            .unwrap()
            .remove(&decision)
            .unwrap()
    };
    let waiting = read();
    assert_eq!(waiting.round_count, 1);
    assert_eq!(waiting.waiting_clarification_id, Some(first));
    assert_eq!(
        waiting.delivery_state,
        Some(ClarificationDeliveryState::Queued)
    );
    assert_eq!(waiting.next_move, ClarificationNextMove::Requester);
    assert_eq!(waiting.latest_reply_at, None);
    let inbox = store.decision_inbox().unwrap();
    let entry = inbox
        .iter()
        .find(|entry| entry.decision.id == decision)
        .unwrap();
    assert_eq!(entry.clarification.as_ref(), Some(&waiting));
    assert_eq!(entry.decision.state, DecisionRequestState::Pending);
    let json = serde_json::to_value(entry).unwrap();
    assert_eq!(json["id"], decision.to_string());
    assert_eq!(json["clarification"]["next_move"], "requester");
    assert!(json["clarification"].get("question").is_none());
    store
        .reply_decision_clarification(first, worker, session, "Because of the dependency.", 101)
        .unwrap();
    let answered = read();
    assert_eq!(answered.next_move, ClarificationNextMove::Operator);
    assert_eq!(answered.waiting_clarification_id, None);
    assert_eq!(answered.delivery_state, None);
    assert_eq!(answered.latest_reply_at, Some(101));
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        DecisionRequestState::Pending
    );
    let second = DecisionClarificationId::new();
    store
        .ask_decision_clarification(second, decision, "Which dependency?", 102)
        .unwrap();
    let claim = store.claim_clarification_deliveries(103).unwrap().remove(0);
    store
        .finish_clarification_delivery(&claim, Outcome::Uncertain)
        .unwrap();
    let uncertain = read();
    assert_eq!(uncertain.round_count, 2);
    assert_eq!(uncertain.waiting_clarification_id, Some(second));
    assert_eq!(uncertain.next_move, ClarificationNextMove::Requester);
    assert_eq!(
        uncertain.delivery_state,
        Some(ClarificationDeliveryState::Uncertain)
    );
    assert_eq!(uncertain.latest_reply_at, Some(101));
    store
        .resolve_decision_request(decision, "Wait", "Final choice", "test")
        .unwrap();
    assert_eq!(read().next_move, ClarificationNextMove::None);
    store
        .reply_decision_clarification(second, worker, session, "Historical reply.", 104)
        .unwrap();
    let historical = read();
    assert_eq!(historical.next_move, ClarificationNextMove::None);
    assert_eq!(historical.latest_reply_at, Some(104));
    assert_eq!(historical.waiting_clarification_id, None);
}

#[test]
fn clarification_claims_are_fenced_and_interrupted_writes_never_auto_retry() {
    let (store, decision, _, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    store
        .renew_worker_engagement(session, Some(PresenceDeviceId::new()), 100, 300)
        .unwrap();
    assert!(
        store
            .claim_clarification_deliveries(101)
            .unwrap()
            .is_empty()
    );
    let claim = store.claim_clarification_deliveries(401).unwrap().remove(0);
    assert!(
        store
            .clarification_dispatch_is_current(&claim, 401)
            .unwrap()
    );
    assert!(
        store
            .claim_clarification_deliveries(402)
            .unwrap()
            .is_empty()
    );
    let mut stale = claim.clone();
    stale.claim_id = uuid::Uuid::now_v7();
    assert!(
        !store
            .clarification_dispatch_is_current(&stale, 402)
            .unwrap()
    );
    assert!(
        !store
            .finish_clarification_delivery(&stale, Outcome::Delivered)
            .unwrap()
    );
    assert!(
        store
            .finish_clarification_delivery(&claim, Outcome::DeferredBeforeWrite)
            .unwrap()
    );
    let next = store.claim_clarification_deliveries(403).unwrap().remove(0);
    assert_ne!(claim.claim_id, next.claim_id);
    assert!(
        !store
            .finish_clarification_delivery(&claim, Outcome::Delivered)
            .unwrap()
    );
    assert_eq!(
        store.recover_inflight_clarification_deliveries().unwrap(),
        1
    );
    assert_eq!(
        store.recover_inflight_clarification_deliveries().unwrap(),
        0
    );
    assert!(
        !store
            .finish_clarification_delivery(&next, Outcome::Delivered)
            .unwrap()
    );
    assert!(
        store
            .claim_clarification_deliveries(404)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.get_decision_clarification(id).unwrap().delivery_state,
        ClarificationDeliveryState::Uncertain
    );
}

#[test]
fn clarification_final_answers_cancel_only_unsent_questions() {
    for claimed in [false, true] {
        for withdrawal in [false, true] {
            let (store, decision, worker, _) = setup();
            let id = DecisionClarificationId::new();
            store
                .ask_decision_clarification(id, decision, "Explain?", 100)
                .unwrap();
            let claim =
                claimed.then(|| store.claim_clarification_deliveries(101).unwrap().remove(0));
            if withdrawal {
                store
                    .withdraw_decision_request(decision, worker, "No longer needed")
                    .unwrap();
            } else {
                store
                    .resolve_decision_request(decision, "Wait", "Final choice", "test")
                    .unwrap();
            }
            assert_eq!(
                store.get_decision_clarification(id).unwrap().delivery_state,
                if claimed {
                    ClarificationDeliveryState::Dispatching
                } else {
                    ClarificationDeliveryState::Cancelled
                }
            );
            if let Some(claim) = claim {
                assert!(
                    !store
                        .clarification_dispatch_is_current(&claim, 102)
                        .unwrap()
                );
                assert!(
                    store
                        .finish_clarification_delivery(&claim, Outcome::DeferredBeforeWrite)
                        .unwrap()
                );
                assert_eq!(
                    store.get_decision_clarification(id).unwrap().delivery_state,
                    ClarificationDeliveryState::Cancelled
                );
            }
            assert!(
                store
                    .claim_clarification_deliveries(103)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[test]
fn clarification_reply_during_dispatch_preserves_transport_evidence() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    let claim = store.claim_clarification_deliveries(101).unwrap().remove(0);
    store
        .reply_decision_clarification(id, worker, session, "Because", 102)
        .unwrap();
    assert!(
        !store
            .clarification_dispatch_is_current(&claim, 103)
            .unwrap()
    );
    assert!(
        store
            .finish_clarification_delivery(&claim, Outcome::Delivered)
            .unwrap()
    );
    let saved = store.get_decision_clarification(id).unwrap();
    assert_eq!(saved.delivery_state, ClarificationDeliveryState::Delivered);
    assert_eq!(saved.reply.as_deref(), Some("Because"));
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        DecisionRequestState::Pending
    );
}

#[test]
fn clarification_reply_retry_after_restart_preserves_original_source_and_new_round() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    store
        .reply_decision_clarification(id, worker, session, "Because", 101)
        .unwrap();
    let followup = DecisionClarificationId::new();
    store
        .ask_decision_clarification(followup, decision, "What about this?", 102)
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE worker_sessions SET ended_at=103 WHERE session_id=?1",
            [session.to_string()],
        )
        .unwrap();
    let replacement = WorkerSessionId::new();
    store.bind_worker_session(worker, replacement).unwrap();
    assert!(matches!(
        store.reply_decision_clarification(id, worker, session, "Because", 104),
        Err(TaskStoreError::DecisionClarification(Refusal::Unauthorized))
    ));
    let saved = store
        .reply_decision_clarification(id, worker, replacement, "Because", 104)
        .unwrap();
    assert_eq!(saved.replying_session_id, Some(session));
    assert_eq!(saved.replied_at, Some(101));
    let pending = store.get_decision_clarification(followup).unwrap();
    assert!(pending.reply.is_none());
    assert_eq!(pending.delivery_state, ClarificationDeliveryState::Queued);
}

#[test]
fn clarification_claims_roll_back_if_the_event_cannot_commit() {
    let (store, decision, _, _) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    store.connection().unwrap().execute_batch("CREATE TRIGGER clarification_claim_failure BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT,'fixture event failure'); END;").unwrap();
    assert!(store.claim_clarification_deliveries(101).is_err());
    assert_eq!(
        store.get_decision_clarification(id).unwrap().delivery_state,
        ClarificationDeliveryState::Queued
    );
}

#[test]
fn clarification_free_text_final_answer_cancels_question_atomically() {
    let (store, decision, _, _) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    let answers =
        std::collections::BTreeMap::from([("Answer".into(), vec!["My final answer".into()])]);
    store.connection().unwrap().execute_batch("CREATE TRIGGER clarification_resolution_failure BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT,'fixture event failure'); END;").unwrap();
    assert!(
        store
            .answer_decision_request(decision, &answers, "", "test")
            .is_err()
    );
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        DecisionRequestState::Pending
    );
    assert_eq!(
        store.get_decision_clarification(id).unwrap().delivery_state,
        ClarificationDeliveryState::Queued
    );
    store
        .connection()
        .unwrap()
        .execute_batch("DROP TRIGGER clarification_resolution_failure")
        .unwrap();
    store
        .answer_decision_request(decision, &answers, "", "test")
        .unwrap();
    assert_eq!(
        store.get_decision_clarification(id).unwrap().delivery_state,
        ClarificationDeliveryState::Cancelled
    );
    assert!(
        store
            .claim_clarification_deliveries(102)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn clarification_batch_is_bounded_and_rechecks_session_before_dispatch() {
    let (store, original, worker, session) = setup();
    store
        .ask_decision_clarification(DecisionClarificationId::new(), original, "Explain?", 100)
        .unwrap();
    for index in 0..16 {
        let request = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: worker,
                task_id: None,
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: &format!("Fictional request {index}"),
                summary: "Which?",
                reason: "Explain first",
                risk: "Fixture",
                evidence: "No work executes",
                suggested_action: "Wait",
                allowed_actions: &["Wait".into()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        store
            .ask_decision_clarification(
                DecisionClarificationId::new(),
                request.id,
                "Explain?",
                101 + index,
            )
            .unwrap();
    }
    let claims = store.claim_clarification_deliveries(200).unwrap();
    assert_eq!(claims.len(), 16);
    assert_eq!(store.claim_clarification_deliveries(201).unwrap().len(), 1);
    assert!(
        store
            .claim_clarification_deliveries(202)
            .unwrap()
            .is_empty()
    );
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE worker_sessions SET ended_at=203 WHERE session_id=?1",
            [session.to_string()],
        )
        .unwrap();
    store
        .bind_worker_session(worker, WorkerSessionId::new())
        .unwrap();
    assert!(
        !store
            .clarification_dispatch_is_current(&claims[0], 204)
            .unwrap()
    );
    let mut wrong_session = claims[0].clone();
    wrong_session.session_id = WorkerSessionId::new();
    assert!(
        !store
            .finish_clarification_delivery(&wrong_session, Outcome::Delivered)
            .unwrap()
    );
    assert!(
        store
            .finish_clarification_delivery(&claims[0], Outcome::Uncertain)
            .unwrap()
    );
    assert_eq!(
        store
            .get_decision_clarification(claims[0].clarification.id)
            .unwrap()
            .delivery_state,
        ClarificationDeliveryState::Uncertain
    );
}

fn setup() -> (TaskStore, DecisionRequestId, WorkerId, WorkerSessionId) {
    setup_in(TaskStore::in_memory().unwrap())
}

fn setup_in(store: TaskStore) -> (TaskStore, DecisionRequestId, WorkerId, WorkerSessionId) {
    store.ensure_queen("/fictional/queen").unwrap();
    let queen = store
        .create_worker(
            "Requester",
            ProviderKind::ClaudeCode,
            "/fictional/requester",
            false,
            1,
        )
        .unwrap();
    let session = WorkerSessionId::new();
    store.bind_worker_session(queen.id, session).unwrap();
    let request = store
        .create_decision_request(&NewDecisionRequest {
            requesting_worker_id: queen.id,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: "Which fictional rollout?",
            summary: "Choose the rollout scope.",
            reason: "A choice is needed.",
            risk: "Fictional risk.",
            evidence: "Fixture only.",
            suggested_action: "Wait",
            allowed_actions: &["Wait".into(), "Proceed".into()],
            questions: &[],
            deadline: None,
            requested_command: None,
        })
        .unwrap();
    (store, request.id, queen.id, session)
}

#[test]
fn clarification_question_and_reply_leave_decision_pending_without_resolution_delivery() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    let question = "  Why this option?\n🐝";
    let saved = store
        .ask_decision_clarification(id, decision, question, 100)
        .unwrap();
    assert_eq!(saved.question, question);
    assert_eq!(saved.delivery_state, ClarificationDeliveryState::Queued);
    let saved_question = store
        .ask_decision_clarification(id, decision, question, 101)
        .unwrap();
    assert_eq!(saved_question.asked_at, 100);
    assert!(matches!(
        store.ask_decision_clarification(id, decision, "different", 102),
        Err(TaskStoreError::DecisionClarification(Refusal::Conflict))
    ));
    assert!(matches!(
        store.ask_decision_clarification(DecisionClarificationId::new(), decision, "Another?", 102),
        Err(TaskStoreError::DecisionClarification(
            Refusal::AlreadyWaiting
        ))
    ));
    let reply = "  This is an explanation, not your approval.\n";
    let saved = store
        .reply_decision_clarification(id, worker, session, reply, 103)
        .unwrap();
    assert_eq!(saved.reply.as_deref(), Some(reply));
    assert_eq!(saved.delivery_state, ClarificationDeliveryState::Cancelled);
    assert_eq!(
        store
            .reply_decision_clarification(id, worker, session, reply, 104)
            .unwrap()
            .replied_at,
        Some(103)
    );
    assert!(matches!(
        store.reply_decision_clarification(id, worker, session, "replacement", 105),
        Err(TaskStoreError::DecisionClarification(Refusal::Conflict))
    ));
    let parent = store.get_decision_request(decision).unwrap();
    assert_eq!(parent.state, DecisionRequestState::Pending);
    assert_eq!(parent.allowed_actions, vec!["Wait", "Proceed"]);
    assert!(parent.resolution_action.is_none());
    assert!(parent.delivery_state.is_none());
    assert!(store.claim_decision_deliveries(106).unwrap().is_empty());
    assert_eq!(store.decision_clarifications(decision).unwrap().len(), 1);
    assert!(
        store
            .ask_decision_clarification(
                DecisionClarificationId::new(),
                decision,
                "A follow-up?",
                107
            )
            .is_ok()
    );
}

#[test]
fn clarification_rejects_wrong_worker_and_ended_session() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Explain?", 100)
        .unwrap();
    let other = store
        .create_worker(
            "Other",
            ProviderKind::ClaudeCode,
            "/fictional/other",
            false,
            1,
        )
        .unwrap();
    let other_session = WorkerSessionId::new();
    store.bind_worker_session(other.id, other_session).unwrap();
    assert!(matches!(
        store.reply_decision_clarification(id, other.id, other_session, "Wrong source", 101),
        Err(TaskStoreError::DecisionClarification(Refusal::Unauthorized))
    ));
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE worker_sessions SET ended_at=102 WHERE session_id=?1",
            [session.to_string()],
        )
        .unwrap();
    assert!(matches!(
        store.reply_decision_clarification(id, worker, session, "Stale source", 103),
        Err(TaskStoreError::DecisionClarification(Refusal::Unauthorized))
    ));
    assert!(
        store
            .get_decision_clarification(id)
            .unwrap()
            .reply
            .is_none()
    );
}

#[test]
fn clarification_late_reply_is_history_and_never_reopens_a_resolved_parent() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Why?", 100)
        .unwrap();
    store
        .resolve_decision_request(decision, "Wait", "Final choice", "test")
        .unwrap();
    assert!(matches!(
        store.ask_decision_clarification(DecisionClarificationId::new(), decision, "Another?", 101),
        Err(TaskStoreError::DecisionClarification(
            Refusal::DecisionNotPending
        ))
    ));
    store
        .reply_decision_clarification(id, worker, session, "Late explanation", 102)
        .unwrap();
    let parent = store.get_decision_request(decision).unwrap();
    assert_eq!(parent.state, DecisionRequestState::Resolved);
    assert_eq!(parent.resolution_action.as_deref(), Some("Wait"));
    assert_eq!(store.decision_clarifications(decision).unwrap().len(), 1);
}

#[test]
fn clarification_event_failure_rolls_back_question_and_reply() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    let fail = "CREATE TRIGGER clarification_event_failure BEFORE INSERT ON control_room_events BEGIN SELECT RAISE(ABORT,'fixture event failure'); END;";
    store.connection().unwrap().execute_batch(fail).unwrap();
    assert!(
        store
            .ask_decision_clarification(id, decision, "Why?", 100)
            .is_err()
    );
    assert!(store.decision_clarifications(decision).unwrap().is_empty());
    store
        .connection()
        .unwrap()
        .execute_batch("DROP TRIGGER clarification_event_failure")
        .unwrap();
    store
        .ask_decision_clarification(id, decision, "Why?", 100)
        .unwrap();
    store.connection().unwrap().execute_batch(fail).unwrap();
    assert!(
        store
            .reply_decision_clarification(id, worker, session, "Because", 101)
            .is_err()
    );
    let saved = store.get_decision_clarification(id).unwrap();
    assert!(saved.reply.is_none());
    assert_eq!(saved.delivery_state, ClarificationDeliveryState::Queued);
}

#[test]
fn clarification_history_capacity_preserves_existing_rounds() {
    let (store, decision, worker, session) = setup();
    for round in 0..32 {
        let id = DecisionClarificationId::new();
        store
            .ask_decision_clarification(id, decision, "Why?", round * 2)
            .unwrap();
        store
            .reply_decision_clarification(id, worker, session, "Because", round * 2 + 1)
            .unwrap();
    }
    assert!(matches!(
        store.ask_decision_clarification(
            DecisionClarificationId::new(),
            decision,
            "One more?",
            100
        ),
        Err(TaskStoreError::DecisionClarification(Refusal::Capacity))
    ));
    assert_eq!(store.decision_clarifications(decision).unwrap().len(), 32);
}

#[test]
fn clarification_queen_reply_is_attributed_to_queen_not_the_requester() {
    let (store, decision, requester, _) = setup();
    let queen = store.ensure_queen("/fictional/queen").unwrap();
    let session = WorkerSessionId::new();
    store.bind_worker_session(queen.id, session).unwrap();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Can Queen explain?", 100)
        .unwrap();
    let saved = store
        .reply_decision_clarification(id, queen.id, session, "Here is the explanation.", 101)
        .unwrap();
    assert_eq!(saved.replying_worker_id, Some(queen.id));
    assert_ne!(saved.replying_worker_id, Some(requester));
}

#[test]
fn clarification_global_capacity_and_closed_history_retention_are_explicit() {
    let (store, decision, worker, session) = setup();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision, "Why?", 0)
        .unwrap();
    store
        .reply_decision_clarification(id, worker, session, "Because", 1)
        .unwrap();
    // Seed a full store to exercise global admission independently of the
    // per-decision bound. All payloads remain fictional and inside SQLite tests.
    store.connection().unwrap().execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095)
        INSERT INTO decision_clarifications(id,decision_id,operator_id,question,asked_at,reply,replied_at,replying_worker_id,replying_session_id,delivery_state,round_index)
        SELECT printf('%036d',n.x),c.decision_id,c.operator_id,c.question,c.asked_at,c.reply,c.replied_at,c.replying_worker_id,c.replying_session_id,c.delivery_state,n.x+1
        FROM n CROSS JOIN (SELECT * FROM decision_clarifications LIMIT 1) c;").unwrap();
    assert!(matches!(
        store.ask_decision_clarification(
            DecisionClarificationId::new(),
            decision,
            "More?",
            90 * 86400 + 2
        ),
        Err(TaskStoreError::DecisionClarification(Refusal::Capacity))
    ));
    // A failed admission rolls its pruning back; open evidence is pinned anyway.
    let count = || {
        store
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM decision_clarifications", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count(), 4096);
    store
        .resolve_decision_request(decision, "Wait", "Done", "test")
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE decision_requests SET resolved_at=1 WHERE id=?1",
            [decision.to_string()],
        )
        .unwrap();
    let next = store
        .create_decision_request(&NewDecisionRequest {
            requesting_worker_id: worker,
            task_id: None,
            kind: DecisionRequestKind::Input,
            urgency: DecisionUrgency::Normal,
            title: "Another choice",
            summary: "Choose",
            reason: "A choice",
            risk: "None",
            evidence: "Fictional",
            suggested_action: "Wait",
            allowed_actions: &["Wait".into()],
            questions: &[],
            deadline: None,
            requested_command: None,
        })
        .unwrap();
    store
        .ask_decision_clarification(
            DecisionClarificationId::new(),
            next.id,
            "New question?",
            90 * 86400 + 2,
        )
        .unwrap();
    assert_eq!(count(), 1);
}

#[test]
fn clarification_migration_and_reopen_preserve_parent_and_exact_exchange() {
    let path = std::env::temp_dir().join(format!(
        "swarm-clarification-{}.sqlite3",
        DecisionClarificationId::new()
    ));
    let (store, decision, worker, session) = setup_in(TaskStore::open(&path).unwrap());
    store
        .connection()
        .unwrap()
        .execute_batch("DROP TABLE decision_clarifications; PRAGMA user_version=158;")
        .unwrap();
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        DecisionRequestState::Pending
    );
    assert!(store.decision_clarifications(decision).unwrap().is_empty());
    let id = DecisionClarificationId::new();
    let operator = store
        .ask_decision_clarification(id, decision, "Exact question\n", 100)
        .unwrap()
        .operator_id;
    store
        .reply_decision_clarification(id, worker, session, "Exact reply\n", 101)
        .unwrap();
    drop(store);
    let store = TaskStore::open(&path).unwrap();
    let saved = store.get_decision_clarification(id).unwrap();
    assert_eq!(saved.question, "Exact question\n");
    assert_eq!(saved.reply.as_deref(), Some("Exact reply\n"));
    assert_eq!(saved.operator_id, operator);
    assert_eq!(
        store
            .ask_decision_clarification(id, decision, "Exact question\n", 102)
            .unwrap()
            .asked_at,
        100
    );
    assert_eq!(
        store.get_decision_request(decision).unwrap().state,
        DecisionRequestState::Pending
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn clarification_of_a_command_request_cannot_mint_a_command_grant() {
    let (store, _, worker, session) = setup();
    let decision = store
        .create_decision_request(&NewDecisionRequest {
            requesting_worker_id: worker,
            task_id: None,
            kind: DecisionRequestKind::Approval,
            urgency: DecisionUrgency::Normal,
            title: "Fictional command approval",
            summary: "Explain the command before choosing.",
            reason: "Needs review",
            risk: "Fictional",
            evidence: "No command executes in this test",
            suggested_action: "Wait",
            allowed_actions: &["Wait".into()],
            questions: &[],
            deadline: None,
            requested_command: Some("cargo test"),
        })
        .unwrap();
    let id = DecisionClarificationId::new();
    store
        .ask_decision_clarification(id, decision.id, "Why run cargo test?", 100)
        .unwrap();
    store
        .reply_decision_clarification(
            id,
            worker,
            session,
            "It checks the code; this does not grant permission.",
            101,
        )
        .unwrap();
    let unchanged = store.get_decision_request(decision.id).unwrap();
    assert_eq!(unchanged.state, DecisionRequestState::Pending);
    assert_eq!(unchanged.allowed_actions, decision.allowed_actions);
    assert_eq!(unchanged.requested_command, decision.requested_command);
    assert!(unchanged.resolution_action.is_none());
    let grants: i64 = store
        .connection()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM decision_command_grants", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(grants, 0);
}
