//! Engine-owned ordering of native invocations and generation-checked input.
use std::collections::{HashMap, VecDeque};

use swarm_domain::{OperatorSubmissionId, PresenceDeviceId, WorkerSessionId};

use crate::{
    NativeInterviewObservation, NativeInterviewPhase, TerminalInputKind, TerminalWriteActor,
    TerminalWriteAuditEntry, TerminalWriteResult,
};

const MAX_PENDING: usize = 32;
const MAX_READY: usize = 32;
const MAX_DEVICES: usize = 4;

pub use swarm_domain::NativeInterviewEvidence;

struct Pending {
    admission_ticket: Option<OperatorSubmissionId>,
    selection_revision: u64,
    observation: NativeInterviewObservation,
    devices: Vec<PresenceDeviceId>,
    first_write_sequence: Option<u64>,
    last_write_sequence: Option<u64>,
    submit_sequence: Option<u64>,
    tainted: bool,
}

struct Prepared {
    ticket: OperatorSubmissionId,
    selection_revision: u64,
    observation: NativeInterviewObservation,
    input_changed: bool,
}

/// The registry owns this under the same ordering lock as its PTY write audit.
/// At most 32 prepared/pending plus 32 completed bounded observations (under 4 MiB of
/// encoded source payload) exist. No background timer or keystroke text store.
#[derive(Default)]
pub(super) struct NativeInterviewCapture {
    prepared: HashMap<WorkerSessionId, Prepared>,
    pending: HashMap<WorkerSessionId, Pending>,
    ready: VecDeque<NativeInterviewEvidence>,
}

impl std::fmt::Debug for NativeInterviewCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeInterviewCapture")
            .field("pending_count", &self.pending.len())
            .field("prepared_count", &self.prepared.len())
            .field("retained_count", &self.ready.len())
            .finish()
    }
}

impl NativeInterviewCapture {
    /// The helper must receive this ticket before it can request admission.
    /// Any intervening input invalidates a not-yet-admitted preparation. This
    /// closes late-callback races without assuming a timeout proves no effect.
    pub fn prepare(
        &mut self,
        session: WorkerSessionId,
        revision: u64,
        observation: NativeInterviewObservation,
    ) -> Option<OperatorSubmissionId> {
        if observation.phase != NativeInterviewPhase::Requested {
            return None;
        }
        if let Some(pending) = self.pending.get(&session)
            && pending.selection_revision == revision
            && pending.observation == observation
        {
            return pending.admission_ticket;
        }
        if let Some(prepared) = self.prepared.get(&session)
            && prepared.selection_revision == revision
            && prepared.observation == observation
        {
            return Some(prepared.ticket);
        }
        if self.ready.len() == MAX_READY
            || (!self.prepared.contains_key(&session)
                && self.prepared.len() + self.pending.len() >= MAX_PENDING)
        {
            return None;
        }
        let ticket = OperatorSubmissionId::new();
        self.prepared.insert(
            session,
            Prepared {
                ticket,
                selection_revision: revision,
                observation,
                input_changed: false,
            },
        );
        Some(ticket)
    }

    pub fn begin_prepared(
        &mut self,
        session: WorkerSessionId,
        revision: u64,
        ticket: OperatorSubmissionId,
        observation: NativeInterviewObservation,
    ) -> bool {
        if let Some(pending) = self.pending.get(&session)
            && pending.admission_ticket == Some(ticket)
            && pending.selection_revision == revision
            && pending.observation == observation
        {
            return true;
        }
        let Some(prepared) = self.prepared.get(&session) else {
            return false;
        };
        if prepared.ticket != ticket {
            return false;
        }
        let prepared = self
            .prepared
            .remove(&session)
            .expect("prepared ticket exists");
        if prepared.input_changed
            || prepared.selection_revision != revision
            || prepared.observation != observation
        {
            return false;
        }
        if !self.observe_at_revision(session, revision, observation) {
            return false;
        }
        let Some(pending) = self.pending.get_mut(&session) else {
            return false;
        };
        pending.admission_ticket = Some(ticket);
        true
    }

    /// Caller has authenticated the live process and current conversation.
    pub fn observe_at_revision(
        &mut self,
        session_id: WorkerSessionId,
        selection_revision: u64,
        observation: NativeInterviewObservation,
    ) -> bool {
        if let Some(existing) = self.ready.iter().find(|entry| {
            entry.session_id == session_id
                && entry.conversation == observation.conversation
                && entry.tool_use_id == observation.tool_use_id
        }) {
            return observation.phase == NativeInterviewPhase::Completed
                && existing.selection_revision == selection_revision
                && existing.questions == observation.questions()
                && existing.answers == *observation.answers();
        }
        match observation.phase {
            NativeInterviewPhase::Requested => {
                if let Some(pending) = self.pending.get(&session_id)
                    && pending.observation == observation
                    && pending.selection_revision == selection_revision
                {
                    // A callback retry cannot reset contamination or reuse input.
                    return true;
                }
                if self.ready.len() == MAX_READY
                    || (!self.pending.contains_key(&session_id)
                        && self.pending.len() == MAX_PENDING)
                {
                    return false;
                }
                self.pending.insert(
                    session_id,
                    Pending {
                        admission_ticket: None,
                        selection_revision,
                        observation,
                        devices: Vec::new(),
                        first_write_sequence: None,
                        last_write_sequence: None,
                        submit_sequence: None,
                        tainted: false,
                    },
                );
                true
            }
            NativeInterviewPhase::Completed => {
                self.complete(session_id, selection_revision, &observation)
            }
        }
    }

    fn complete(
        &mut self,
        session_id: WorkerSessionId,
        selection_revision: u64,
        observation: &NativeInterviewObservation,
    ) -> bool {
        let Some(pending) = self.pending.get(&session_id) else {
            return false;
        };
        // An old invocation's result must not consume a newer question.
        if pending.observation.tool_use_id != observation.tool_use_id
            || pending.observation.conversation != observation.conversation
        {
            return false;
        }
        let pending = self
            .pending
            .remove(&session_id)
            .expect("pending invocation exists");
        let (Some(first_write_sequence), Some(submit_sequence)) =
            (pending.first_write_sequence, pending.submit_sequence)
        else {
            return false;
        };
        if pending.selection_revision != selection_revision
            || pending.tainted
            || pending.devices.is_empty()
            || pending.last_write_sequence != Some(submit_sequence)
            || !observation.completes(&pending.observation)
            || self.ready.len() == MAX_READY
        {
            return false;
        }
        self.ready.push_back(NativeInterviewEvidence {
            id: OperatorSubmissionId::new(),
            session_id,
            conversation: observation.conversation,
            selection_revision,
            tool_use_id: observation.tool_use_id.clone(),
            devices: pending.devices,
            first_write_sequence,
            submit_sequence,
            questions: observation.questions().to_vec(),
            answers: observation.answers().clone(),
        });
        true
    }

    #[cfg(test)]
    fn observe(
        &mut self,
        session_id: WorkerSessionId,
        observation: NativeInterviewObservation,
    ) -> bool {
        self.observe_at_revision(session_id, 1, observation)
    }

    /// `controlled` is supplied only by the registry's successful v4 input path,
    /// never from wire provenance or an agent-provided actor label.
    pub fn record_write(&mut self, entry: &TerminalWriteAuditEntry, controlled: bool) {
        if let Some(prepared) = self.prepared.get_mut(&entry.session_id) {
            prepared.input_changed = true;
        }
        let Some(pending) = self.pending.get_mut(&entry.session_id) else {
            return;
        };
        if pending.tainted {
            return;
        }
        let TerminalWriteActor::Operator {
            device_id: Some(device),
        } = entry.actor
        else {
            pending.tainted = true;
            return;
        };
        if !controlled
            || entry.result != TerminalWriteResult::Acknowledged
            || entry.input_kind == TerminalInputKind::Interrupt
            || entry.sequence == u64::MAX
            || pending
                .last_write_sequence
                .is_some_and(|last| entry.sequence <= last)
        {
            pending.tainted = true;
            return;
        }
        if !pending.devices.contains(&device) {
            if pending.devices.len() == MAX_DEVICES {
                pending.tainted = true;
                return;
            }
            pending.devices.push(device);
        }
        pending.first_write_sequence.get_or_insert(entry.sequence);
        pending.last_write_sequence = Some(entry.sequence);
        if entry.input_kind == TerminalInputKind::Submit {
            pending.submit_sequence = Some(entry.sequence);
        }
    }

    pub fn invalidate_pending(&mut self, session: WorkerSessionId) {
        self.prepared.remove(&session);
        self.pending.remove(&session);
    }

    pub fn retained(&self) -> Vec<NativeInterviewEvidence> {
        self.ready.iter().cloned().collect()
    }

    pub fn acknowledge(&mut self, id: OperatorSubmissionId) {
        self.ready.retain(|entry| entry.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_claude_interview;
    use serde_json::json;

    fn observation(completed: bool, id: &str) -> NativeInterviewObservation {
        let questions = json!([{"question":"Which fictional jar?","header":"Jar",
            "options":[{"label":"Amber"},{"label":"Blue"}]}]);
        let mut event = json!({"session_id":"00000000-0000-0000-0000-000000000001",
            "tool_use_id":id, "tool_name":"AskUserQuestion", "tool_input":{"questions":questions},
            "hook_event_name":if completed {"PostToolUse"} else {"PreToolUse"}});
        if completed {
            event["tool_response"] = json!({"questions":questions,
            "answers":{"Which fictional jar?":"Amber"}});
        }
        read_claude_interview(&serde_json::to_vec(&event).unwrap()).unwrap()
    }

    fn input(session_id: WorkerSessionId, sequence: u64) -> TerminalWriteAuditEntry {
        TerminalWriteAuditEntry {
            sequence,
            session_id,
            actor: TerminalWriteActor::Operator {
                device_id: Some(PresenceDeviceId::new()),
            },
            input_kind: TerminalInputKind::Submit,
            byte_count: 1,
            result: TerminalWriteResult::Acknowledged,
            occurred_at: 0,
        }
    }

    #[test]
    fn captured_result_survives_reads_until_exact_id_acknowledgement() {
        let mut capture = NativeInterviewCapture::default();
        let session = WorkerSessionId::new();
        assert!(capture.observe(session, observation(false, "toolu_one")));
        capture.record_write(&input(session, 1), true);
        assert!(capture.observe(session, observation(true, "toolu_one")));
        let first = capture.retained();
        assert_eq!(first.len(), 1);
        assert_eq!(capture.retained()[0].id, first[0].id);
        assert!(capture.observe(session, observation(true, "toolu_one")));
        assert_eq!(capture.retained().len(), 1);
        capture.invalidate_pending(session);
        assert_eq!(capture.retained().len(), 1);
        capture.acknowledge(OperatorSubmissionId::new());
        assert_eq!(capture.retained().len(), 1);
        capture.acknowledge(first[0].id);
        capture.acknowledge(first[0].id);
        assert!(capture.retained().is_empty());
        assert!(!capture.observe(session, observation(true, "toolu_one")));
        assert!(!format!("{:?}", first[0]).contains("Amber"));
    }

    #[test]
    fn no_input_uncontrolled_labels_automation_and_uncertainty_cannot_become_answers() {
        for mode in 0..6 {
            let mut capture = NativeInterviewCapture::default();
            let session = WorkerSessionId::new();
            capture.observe(session, observation(false, "toolu_one"));
            let mut write = input(session, 1);
            match mode {
                2 => write.actor = TerminalWriteActor::SwarmCoordination,
                3 => write.result = TerminalWriteResult::Rejected,
                4 => write.input_kind = TerminalInputKind::Interrupt,
                5 => write.input_kind = TerminalInputKind::Navigation,
                _ => {}
            }
            if mode != 0 {
                capture.record_write(&write, mode != 1);
            }
            assert!(
                !capture.observe(session, observation(true, "toolu_one")),
                "mode {mode}"
            );
            assert!(capture.retained().is_empty());
        }
    }

    #[test]
    fn duplicate_request_cannot_erase_mixed_input_and_old_result_cannot_take_new_input() {
        let mut capture = NativeInterviewCapture::default();
        let session = WorkerSessionId::new();
        capture.observe(session, observation(false, "toolu_one"));
        capture.record_write(&input(session, 1), false);
        capture.observe(session, observation(false, "toolu_one"));
        capture.record_write(&input(session, 2), true);
        assert!(!capture.observe(session, observation(true, "toolu_one")));
        capture.observe(session, observation(false, "toolu_two"));
        capture.record_write(&input(session, 3), true);
        assert!(!capture.observe(session, observation(true, "toolu_one")));
        assert!(capture.observe(session, observation(true, "toolu_two")));
    }

    #[test]
    fn bounded_device_handoff_is_allowed_but_input_after_submit_is_not_a_receipt() {
        let mut capture = NativeInterviewCapture::default();
        let session = WorkerSessionId::new();
        capture.observe(session, observation(false, "toolu_one"));
        let mut navigation = input(session, 1);
        navigation.input_kind = TerminalInputKind::Navigation;
        capture.record_write(&navigation, true);
        capture.record_write(&input(session, 2), true);
        assert!(capture.observe(session, observation(true, "toolu_one")));
        assert_eq!(capture.retained()[0].devices.len(), 2);
        capture.observe(session, observation(false, "toolu_two"));
        capture.record_write(&input(session, 3), true);
        navigation.sequence = 4;
        capture.record_write(&navigation, true);
        assert!(!capture.observe(session, observation(true, "toolu_two")));
    }

    #[test]
    fn capacity_refuses_capture_without_evicting_unacknowledged_evidence() {
        let mut capture = NativeInterviewCapture::default();
        for index in 0..MAX_READY {
            let session = WorkerSessionId::new();
            let id = format!("toolu_{index}");
            assert!(capture.observe(session, observation(false, &id)));
            capture.record_write(&input(session, 1), true);
            assert!(capture.observe(session, observation(true, &id)));
        }
        assert!(!capture.observe(WorkerSessionId::new(), observation(false, "toolu_full")));
        assert_eq!(capture.retained().len(), MAX_READY);
        let mut pending = NativeInterviewCapture::default();
        for _ in 0..MAX_PENDING {
            assert!(pending.observe(WorkerSessionId::new(), observation(false, "toolu_pending")));
        }
        assert!(!pending.observe(WorkerSessionId::new(), observation(false, "toolu_full")));
    }

    #[test]
    fn delayed_begin_cannot_claim_input_after_the_preparation_receipt() {
        let mut capture = NativeInterviewCapture::default();
        let session = WorkerSessionId::new();
        let request = observation(false, "toolu_one");
        let ticket = capture.prepare(session, 1, request.clone()).unwrap();
        capture.record_write(&input(session, 1), true);
        assert!(!capture.begin_prepared(session, 1, ticket, request));
        assert!(!capture.observe(session, observation(true, "toolu_one")));
        assert!(capture.retained().is_empty());
    }

    #[test]
    fn ticket_is_exact_and_retries_do_not_reset_admitted_or_prepared_input() {
        let mut capture = NativeInterviewCapture::default();
        let session = WorkerSessionId::new();
        let request = observation(false, "toolu_one");
        let ticket = capture.prepare(session, 1, request.clone()).unwrap();
        assert!(!capture.begin_prepared(session, 1, OperatorSubmissionId::new(), request.clone()));
        assert!(capture.begin_prepared(session, 1, ticket, request.clone()));
        capture.record_write(&input(session, 1), false);
        assert_eq!(capture.prepare(session, 1, request.clone()), Some(ticket));
        assert!(capture.begin_prepared(session, 1, ticket, request));
        capture.record_write(&input(session, 2), true);
        assert!(!capture.observe(session, observation(true, "toolu_one")));
    }
}
