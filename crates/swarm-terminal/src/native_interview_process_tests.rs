use super::*;
use crate::{ProviderLifecycleAcceptance, ProviderLifecycleGate, ProviderSessionStartObservation};
use serde_json::json;
use swarm_domain::{
    PresenceDeviceId, ProviderConversationId, ProviderSessionStartKind, TerminalViewId,
};

const CAPABILITY: [u8; 32] = [42; 32];

fn observe(
    registry: &SessionRegistry,
    session: WorkerSessionId,
    capability: &[u8; 32],
    payload: &[u8],
) -> Result<bool, SessionRegistryError> {
    if crate::read_claude_interview(payload)
        .is_some_and(|o| o.phase == crate::NativeInterviewPhase::Requested)
    {
        let Some(ticket) = registry.prepare_native_interview(session, capability, payload)? else {
            return Ok(false);
        };
        registry.begin_native_interview(session, capability, ticket, payload)
    } else {
        let accepted = registry.observe_native_interview(session, capability, payload)?;
        if accepted && registry.native_interview_evidence()?.is_empty() {
            let observation = crate::read_claude_interview(payload).unwrap();
            let questions = observation.questions();
            let batch = serde_json::to_vec(&json!({"hook_event_name":"PostToolBatch",
                "session_id":observation.conversation, "tool_calls":[{
                "tool_name":"AskUserQuestion", "tool_use_id":observation.tool_use_id,
                "tool_input":{"questions":questions},
                "tool_response":"Your questions have been answered: \"Which fictional jar?\"=\"Amber\". You can now continue with these answers in mind."
            }]})).unwrap();
            registry.observe_native_interview(session, capability, &batch)
        } else {
            Ok(accepted)
        }
    }
}

#[test]
fn delayed_admission_cannot_claim_input_that_preceded_the_helper_round_trip() {
    let (registry, session, conversation, grant) = fixture();
    let request = payload(conversation, false);
    assert!(
        !registry
            .observe_native_interview(session.id(), &CAPABILITY, &request)
            .unwrap()
    );
    let ticket = registry
        .prepare_native_interview(session.id(), &CAPABILITY, &request)
        .unwrap()
        .unwrap();
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        !registry
            .begin_native_interview(session.id(), &CAPABILITY, ticket, &request)
            .unwrap()
    );
    assert!(
        !registry
            .observe_native_interview(session.id(), &CAPABILITY, &payload(conversation, true))
            .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
}

#[test]
fn provisional_result_requires_an_authenticated_final_callback() {
    let (registry, session, conversation, grant) = fixture();
    assert!(
        observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .unwrap()
    );
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        registry
            .observe_native_interview(session.id(), &CAPABILITY, &payload(conversation, true))
            .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
    let observation = crate::read_claude_interview(&payload(conversation, true)).unwrap();
    let batch = serde_json::to_vec(&json!({"hook_event_name":"PostToolBatch", "session_id":conversation,
        "tool_calls":[{"tool_name":"AskUserQuestion", "tool_use_id":"toolu_fixture",
        "tool_input":{"questions":observation.questions()},
        "tool_response":"Your questions have been answered: \"Which fictional jar?\"=\"Amber\". You can now continue with these answers in mind."}]})).unwrap();
    assert!(
        !registry
            .observe_native_interview(session.id(), &[99; 32], &batch)
            .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
    assert!(
        registry
            .observe_native_interview(session.id(), &CAPABILITY, &batch)
            .unwrap()
    );
    assert_eq!(registry.native_interview_evidence().unwrap().len(), 1);
}

fn fixture() -> (
    SessionRegistry,
    Arc<ProcessTerminalSession>,
    ProviderConversationId,
    TerminalControlGrant,
) {
    let (registry, session) = control_tests::fixture();
    let conversation = ProviderConversationId::new();
    *session.provider_lifecycle.lock().unwrap() =
        Some(ProviderLifecycleGate::new(session.id(), CAPABILITY));
    assert_eq!(
        session
            .observe_provider_start(
                &CAPABILITY,
                ProviderSessionStartObservation {
                    conversation,
                    kind: ProviderSessionStartKind::New,
                }
            )
            .unwrap(),
        ProviderLifecycleAcceptance::Accepted
    );
    let identity = TerminalControlIdentity {
        device: PresenceDeviceId::new(),
        view: TerminalViewId::new(),
    };
    let grant = registry
        .claim_control(session.id(), identity, None, TerminalSize::new(24, 80))
        .unwrap();
    (registry, session, conversation, grant)
}

fn payload(conversation: ProviderConversationId, completed: bool) -> Vec<u8> {
    let questions = json!([{"question":"Which fictional jar?", "header":"Jar",
        "options":[{"label":"Amber"},{"label":"Blue"}]}]);
    let mut event = json!({"hook_event_name":if completed {"PostToolUse"} else {"PreToolUse"},
        "session_id":conversation, "tool_use_id":"toolu_fixture", "tool_name":"AskUserQuestion",
        "tool_input":{"questions":questions}});
    if completed {
        event["tool_response"] =
            json!({"questions":questions, "answers":{"Which fictional jar?":"Amber"}});
    }
    serde_json::to_vec(&event).unwrap()
}

#[test]
fn actual_controlled_input_and_native_result_are_retained_across_reader_replacement() {
    let (registry, session, conversation, grant) = fixture();
    assert!(
        observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .unwrap()
    );
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"Amber")
        .unwrap();
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    let evidence = registry.native_interview_evidence().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].devices, vec![grant.identity.device]);
    let audit = registry.recent_write_audit(2).unwrap();
    assert_eq!(evidence[0].submit_sequence, audit[0].sequence);
    assert_eq!(evidence[0].first_write_sequence, audit[1].sequence);
    assert_eq!(
        registry.native_interview_evidence().unwrap()[0].id,
        evidence[0].id
    );
    registry.stop(session.id()).unwrap();
    assert_eq!(
        registry.native_interview_evidence().unwrap()[0].id,
        evidence[0].id
    );
    registry
        .acknowledge_native_interview(evidence[0].id)
        .unwrap();
    assert!(registry.native_interview_evidence().unwrap().is_empty());
}

#[test]
fn wrong_capability_session_conversation_and_resume_boundary_refuse_capture() {
    let (registry, session, conversation, grant) = fixture();
    assert!(
        !observe(
            &registry,
            session.id(),
            &[0; 32],
            &payload(conversation, false)
        )
        .unwrap()
    );
    assert!(
        observe(
            &registry,
            WorkerSessionId::new(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .is_err()
    );
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(ProviderConversationId::new(), false)
        )
        .unwrap()
    );
    assert!(
        observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .unwrap()
    );
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        session
            .observe_resume_end(&CAPABILITY, conversation)
            .unwrap()
    );
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    session.stop().unwrap();
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
}

#[test]
fn uncertain_or_automation_writes_cannot_be_erased_by_retrying_the_native_request() {
    let (registry, session, conversation, grant) = fixture();
    observe(
        &registry,
        session.id(),
        &CAPABILITY,
        &payload(conversation, false),
    )
    .unwrap();
    assert!(
        registry
            .write_controlled(session.id(), grant.identity, grant.generation + 1, b"\r")
            .is_err()
    );
    observe(
        &registry,
        session.id(),
        &CAPABILITY,
        &payload(conversation, false),
    )
    .unwrap();
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    observe(
        &registry,
        session.id(),
        &CAPABILITY,
        &payload(conversation, false),
    )
    .unwrap();
    assert!(
        registry
            .write_local(session.id(), b"\r", TerminalWriteProvenance::coordination())
            .is_err()
    );
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
}

#[test]
fn returning_to_the_same_conversation_does_not_resurrect_an_old_input_window() {
    let (registry, session, conversation, grant) = fixture();
    observe(
        &registry,
        session.id(),
        &CAPABILITY,
        &payload(conversation, false),
    )
    .unwrap();
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(
        session
            .observe_resume_end(&CAPABILITY, conversation)
            .unwrap()
    );
    assert_eq!(
        session
            .observe_provider_start(
                &CAPABILITY,
                ProviderSessionStartObservation {
                    conversation,
                    kind: ProviderSessionStartKind::Resumed,
                }
            )
            .unwrap(),
        ProviderLifecycleAcceptance::ConversationChanged
    );
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
}

#[test]
fn unsupported_authenticated_result_invalidates_pending_input_without_replaying_it() {
    let (registry, session, conversation, grant) = fixture();
    observe(
        &registry,
        session.id(),
        &CAPABILITY,
        &payload(conversation, false),
    )
    .unwrap();
    registry
        .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
        .unwrap();
    assert!(!observe(&registry, session.id(), &CAPABILITY, b"unsupported").unwrap());
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    assert_eq!(registry.recent_write_audit(10).unwrap().len(), 1);
}

#[test]
fn unavailable_capture_does_not_turn_successful_input_into_an_unsent_error() {
    let (registry, session, _, grant) = fixture();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = registry.native_interviews.lock().unwrap();
        panic!("fictional capture failure");
    }));
    assert!(
        registry
            .write_controlled(session.id(), grant.identity, grant.generation, b"\r")
            .is_ok()
    );
    assert!(registry.native_interview_evidence().is_err());
    assert_eq!(
        registry.recent_write_audit(1).unwrap()[0].result,
        TerminalWriteResult::Acknowledged
    );
}

#[test]
fn an_operator_label_on_legacy_input_is_not_generation_checked_provenance() {
    let (registry, session) = control_tests::fixture();
    let conversation = ProviderConversationId::new();
    *session.provider_lifecycle.lock().unwrap() =
        Some(ProviderLifecycleGate::new(session.id(), CAPABILITY));
    session
        .observe_provider_start(
            &CAPABILITY,
            ProviderSessionStartObservation {
                conversation,
                kind: ProviderSessionStartKind::New,
            },
        )
        .unwrap();
    assert!(
        observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, false)
        )
        .unwrap()
    );
    registry
        .write_local(
            session.id(),
            b"\r",
            TerminalWriteProvenance::operator(Some(PresenceDeviceId::new()), b"\r"),
        )
        .unwrap();
    assert_eq!(
        registry.recent_write_audit(1).unwrap()[0].result,
        TerminalWriteResult::Acknowledged
    );
    assert!(
        !observe(
            &registry,
            session.id(),
            &CAPABILITY,
            &payload(conversation, true)
        )
        .unwrap()
    );
    assert!(registry.native_interview_evidence().unwrap().is_empty());
}
