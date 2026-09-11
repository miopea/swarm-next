//! Durable source acknowledgement is separate from operator authority.
use crate::TaskService;
use swarm_domain::{MAX_NATIVE_INTERVIEW_BATCH, NativeInterviewEvidence, OperatorSubmissionId};
use swarm_persistence::NativeInterviewStoreError;

/// Only successful durable admission can create an acknowledgement receipt.
/// This proves storage, not provider consumption or permission to act.
#[derive(Debug)]
pub struct NativeSourceReceipt {
    id: OperatorSubmissionId,
}

impl NativeSourceReceipt {
    #[must_use]
    pub fn id(&self) -> OperatorSubmissionId {
        self.id
    }
}

#[derive(Debug, Default)]
pub struct NativeSourceAdmission {
    pub receipts: Vec<NativeSourceReceipt>,
    pub newly_stored: usize,
    pub refused: usize,
    pub storage_unavailable: bool,
}

impl TaskService {
    /// The caller must obtain these sources from the trusted local engine, never
    /// an agent or browser payload. Exact retries produce the same acknowledgement
    /// identity without manufacturing a statement, decision or another delivery.
    /// A rejected source cannot starve valid siblings; storage failure stops this
    /// pass and leaves every unsaved source in the engine for subsequent recovery.
    #[must_use]
    pub fn retain_native_sources(
        &self,
        entries: &[NativeInterviewEvidence],
        now: i64,
    ) -> NativeSourceAdmission {
        let mut result = NativeSourceAdmission::default();
        if entries.len() > MAX_NATIVE_INTERVIEW_BATCH {
            result.refused = entries.len();
            return result;
        }
        for source in entries {
            match self.store.record_native_interview(source, now) {
                Ok(created) => {
                    result.newly_stored += usize::from(created);
                    result.receipts.push(NativeSourceReceipt { id: source.id });
                }
                Err(NativeInterviewStoreError::Invalid | NativeInterviewStoreError::Conflict) => {
                    result.refused += 1;
                }
                Err(_) => {
                    result.storage_unavailable = true;
                    break;
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        NativeInterviewOption, NativeInterviewQuestion, PresenceDeviceId, ProviderConversationId,
        WorkerSessionId,
    };
    use swarm_persistence::TaskStore;

    fn fixture(store: &TaskStore) -> NativeInterviewEvidence {
        let worker = store.ensure_queen("/fictional").unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        NativeInterviewEvidence {
            final_result: None,
            id: OperatorSubmissionId::new(),
            session_id: session,
            conversation: ProviderConversationId::new(),
            selection_revision: 1,
            tool_use_id: "toolu_fictional".into(),
            devices: vec![PresenceDeviceId::new()],
            first_write_sequence: 1,
            submit_sequence: 2,
            questions: vec![NativeInterviewQuestion {
                header: "Jar".into(),
                question: "Which jar?".into(),
                options: vec![
                    NativeInterviewOption {
                        label: "Amber".into(),
                        description: "Keep closed".into(),
                    },
                    NativeInterviewOption {
                        label: "Blue".into(),
                        description: "Do not move".into(),
                    },
                ],
                multi_select: false,
            }],
            answers: [("Which jar?".into(), "Amber".into())].into(),
        }
    }

    /// Builds a pending decision whose questions match the captured source
    /// exactly, which is the only shape the bridge will act on.
    fn pending_decision(
        store: &TaskStore,
        source: &NativeInterviewEvidence,
    ) -> swarm_domain::DecisionRequestId {
        let _ = source;
        let question = swarm_domain::DecisionQuestion {
            header: "Jar".into(),
            question: "Which jar?".into(),
            options: vec!["Amber".into(), "Blue".into()],
            option_descriptions: [
                ("Amber".to_owned(), "Keep closed".to_owned()),
                ("Blue".to_owned(), "Do not move".to_owned()),
            ]
            .into(),
            multi_select: false,
        };
        // ensure_queen is idempotent, so this is the same worker the fixture
        // bound the session to.
        let worker = store.ensure_queen("/fictional").unwrap().id;
        store
            .create_decision_request(&swarm_persistence::NewDecisionRequest {
                requesting_worker_id: worker,
                task_id: None,
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Jar",
                summary: "Choose a jar",
                reason: "Need a jar",
                risk: "",
                evidence: "",
                suggested_action: "Choose",
                allowed_actions: &[],
                questions: std::slice::from_ref(&question),
                deadline: None,
                requested_command: None,
            })
            .unwrap()
            .id
    }

    /// An answer the engine confirmed resolves the decision, and a replay of the
    /// same source does not resolve it a second time.
    #[test]
    fn a_confirmed_native_answer_resolves_once_and_a_replay_does_not_resolve_again() {
        let store = TaskStore::in_memory().unwrap();
        let mut source = fixture(&store);
        source.final_result = Some(swarm_domain::NativeInterviewFinalResult::ExactBatch);
        let service = TaskService::new(store.clone());
        assert_eq!(
            service
                .retain_native_sources(std::slice::from_ref(&source), 100)
                .newly_stored,
            1
        );
        let decision = pending_decision(&store, &source);

        assert_eq!(
            service
                .link_native_answer(source.id, decision, 101)
                .unwrap(),
            NativeAnswerLink::Resolved
        );
        // THE REPLAY. Statement IDs are random, so this is the case that would
        // mint a second set and resolve twice if the pass were not resumable.
        assert_eq!(
            service
                .link_native_answer(source.id, decision, 102)
                .unwrap(),
            NativeAnswerLink::AlreadyLinked
        );
    }

    /// THE NEGATIVES, each failing for its own reason rather than a shared one.
    ///
    /// These are the whole safety argument. An automatic trigger is only
    /// defensible because a capture that is not demonstrably the operator's
    /// submitted answer resolves nothing.
    #[test]
    fn unconfirmed_and_wrong_session_answers_resolve_nothing() {
        let store = TaskStore::in_memory().unwrap();

        // UNCHECKED IS NOT SUCCESS. final_result absent is what every older
        // stored source has, and what a capture that merely watched keystrokes
        // would have.
        let unchecked = fixture(&store);
        let service = TaskService::new(store.clone());
        let _ = service.retain_native_sources(std::slice::from_ref(&unchecked), 100);
        let decision = pending_decision(&store, &unchecked);
        assert_eq!(
            service
                .link_native_answer(unchecked.id, decision, 101)
                .unwrap(),
            NativeAnswerLink::Unverified,
            "an unconfirmed capture must never resolve a decision"
        );

        // WRONG SESSION. Confirmed, well-formed, identical questions — and a
        // real live session belonging to a DIFFERENT worker. This is the case
        // that must not be rescued by the questions happening to match.
        let stranger = store
            .create_worker(
                "Stranger",
                swarm_domain::ProviderKind::ClaudeCode,
                "/fictional-stranger",
                false,
                1,
            )
            .unwrap();
        let stranger_session = WorkerSessionId::new();
        store
            .bind_worker_session(stranger.id, stranger_session)
            .unwrap();
        let mut foreign = unchecked.clone();
        foreign.final_result = Some(swarm_domain::NativeInterviewFinalResult::ExactBatch);
        foreign.session_id = stranger_session;
        foreign.id = OperatorSubmissionId::new();
        assert_eq!(
            service
                .retain_native_sources(std::slice::from_ref(&foreign), 102)
                .newly_stored,
            1,
            "the foreign source is well-formed and IS retained; only linking must refuse"
        );
        assert_ne!(
            service
                .link_native_answer(foreign.id, decision, 103)
                .unwrap(),
            NativeAnswerLink::Resolved,
            "an answer from another session must not resolve this decision"
        );

        // And the decision is still pending after both attempts.
        assert_eq!(
            store.get_decision_request(decision).unwrap().state,
            swarm_domain::DecisionRequestState::Pending
        );
    }

    #[test]
    fn acknowledgement_identity_requires_saved_source_and_survives_service_replacement() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        let admitted = TaskService::new(store.clone())
            .retain_native_sources(std::slice::from_ref(&source), 100);
        assert_eq!(admitted.newly_stored, 1);
        assert_eq!(admitted.receipts[0].id(), source.id);
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
        let retried = TaskService::new(store.clone()).retain_native_sources(&[source], 101);
        assert_eq!(retried.newly_stored, 0);
        assert_eq!(retried.receipts.len(), 1);
        assert!(store.list_decision_requests().unwrap().is_empty());
        assert!(store.claim_decision_deliveries(102).unwrap().is_empty());
    }

    #[test]
    fn invalid_and_conflicting_sources_cannot_gain_receipts_or_starve_valid_sources() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        let mut invalid = source.clone();
        invalid.session_id = WorkerSessionId::new();
        let service = TaskService::new(store.clone());
        let admitted = service.retain_native_sources(&[invalid, source.clone()], 100);
        assert_eq!(admitted.refused, 1);
        assert_eq!(admitted.receipts.len(), 1);
        let mut changed = source.clone();
        changed.answers.insert("Which jar?".into(), "Blue".into());
        let refused = service.retain_native_sources(&[changed], 101);
        assert_eq!(refused.refused, 1);
        assert!(refused.receipts.is_empty());
        assert_eq!(
            store.native_interview(source.id).unwrap().unwrap().source,
            source
        );
    }

    #[test]
    fn oversized_batch_is_refused_before_any_storage_write() {
        let store = TaskStore::in_memory().unwrap();
        let source = fixture(&store);
        let result = TaskService::new(store.clone())
            .retain_native_sources(&vec![source.clone(); MAX_NATIVE_INTERVIEW_BATCH + 1], 100);
        assert!(result.receipts.is_empty());
        assert!(store.native_interview(source.id).unwrap().is_none());
    }
}

/// What a linkage attempt did, or why it did nothing.
///
/// Every refusal is a distinct variant on purpose: the operator is told an
/// answer was seen and could not be used, and a single "refused" would make
/// that message unwriteable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAnswerLink {
    /// Resolved the decision. The only outcome that clears a Needs You item.
    Resolved,
    /// This source already resolved it. A replay, not a second answer.
    AlreadyLinked,
    /// The engine did not confirm an exact final batch, so nothing here is
    /// known to be the operator's submitted answer.
    Unverified,
    /// Some answers are held, and the set is not complete yet.
    Incomplete,
    /// The decision moved, its questions changed, or the session ended.
    NoLongerApplicable,
    /// Bound to a different decision already, or contradicts stored evidence.
    Conflicting,
}

impl TaskService {
    /// Turns an authenticated native interview into a resolved decision.
    ///
    /// ⚠️ THE APPLICATION OWNS EXACTLY ONE CHECK HERE, and it is the first one.
    /// ADR 0065 assigns authenticating the human source to this layer, and
    /// `final_result` is the only thing that carries it: the engine sets
    /// `ExactBatch` solely after comparing the exact final batch against what
    /// it observed. Absence means UNCHECKED and must never read as success —
    /// older stored sources have it absent, and a capture that merely watched
    /// keystrokes never earns it.
    ///
    /// Every other safety property is already enforced beneath this and is
    /// deliberately not re-implemented: same worker and session, the decision
    /// still pending, the session still active, and the decision's questions
    /// matching the captured snapshot exactly.
    ///
    /// RESUMABLE RATHER THAN TRANSACTIONAL. Statement IDs are random, so a pass
    /// that recorded some answers and then died could not recognise its own
    /// work. Reading the recorded answers back makes each step replayable, which
    /// achieves what a single cross-module transaction would without reaching
    /// through three modules to get it.
    ///
    /// # Errors
    /// Propagates persistence failures. A refusal is an `Ok` variant, because
    /// "this answer cannot be used" is an outcome to report, not a fault.
    pub fn link_native_answer(
        &self,
        source_id: OperatorSubmissionId,
        decision_id: swarm_domain::DecisionRequestId,
        now: i64,
    ) -> Result<NativeAnswerLink, swarm_persistence::TaskStoreError> {
        let Some(stored) = self.store.native_interview(source_id)? else {
            return Ok(NativeAnswerLink::NoLongerApplicable);
        };
        if stored.source.final_result != Some(swarm_domain::NativeInterviewFinalResult::ExactBatch)
        {
            return Ok(NativeAnswerLink::Unverified);
        }
        let worker = stored.worker_id;
        let session = stored.source.session_id;

        match self
            .store
            .bind_native_interview(source_id, decision_id, worker, session)
        {
            Ok(_) => {}
            Err(NativeInterviewStoreError::Conflict) => return Ok(NativeAnswerLink::Conflicting),
            Err(NativeInterviewStoreError::Invalid | NativeInterviewStoreError::Capacity) => {
                return Ok(NativeAnswerLink::NoLongerApplicable);
            }
            Err(NativeInterviewStoreError::Store(error)) => return Err(error),
            Err(NativeInterviewStoreError::Sql(error)) => {
                return Err(swarm_persistence::TaskStoreError::from(error));
            }
        }
        self.record_and_resolve(&stored, decision_id, worker, session, now)
    }

    /// Records any answer not already stored, then asks for resolution.
    ///
    /// Reuses a statement already on file for the same question and answer
    /// rather than minting a second one, so a retry after a partial pass
    /// completes the set instead of duplicating it.
    fn record_and_resolve(
        &self,
        stored: &swarm_persistence::StoredNativeInterview,
        decision_id: swarm_domain::DecisionRequestId,
        worker: swarm_domain::WorkerId,
        session: swarm_domain::WorkerSessionId,
        now: i64,
    ) -> Result<NativeAnswerLink, swarm_persistence::TaskStoreError> {
        let existing = match self
            .store
            .recorded_statement_answers(decision_id, worker, session)
        {
            Ok(rows) => rows,
            Err(swarm_persistence::OperatorStatementError::Store(error)) => return Err(error),
            Err(_) => return Ok(NativeAnswerLink::NoLongerApplicable),
        };

        let mut ids = Vec::with_capacity(stored.source.questions.len());
        for question in &stored.source.questions {
            let Some(answer) = stored.source.answers.get(&question.question) else {
                return Ok(NativeAnswerLink::Incomplete);
            };
            if let Some((id, _, _)) = existing.iter().find(|(_, stored_question, stored_answer)| {
                stored_question == &question.question && stored_answer == answer
            }) {
                ids.push(*id);
                continue;
            }
            let Some(evidence) = swarm_domain::OperatorAnswerEvidence::new(
                decision_target(decision_id, worker, session, question),
                answer.clone(),
                swarm_domain::OperatorAnswerConsumption::Confirmed,
            ) else {
                return Ok(NativeAnswerLink::NoLongerApplicable);
            };
            let id = swarm_domain::OperatorStatementId::new();
            match self.store.record_operator_statement(id, &evidence, now) {
                Ok(_) => ids.push(id),
                Err(swarm_persistence::OperatorStatementError::Conflict) => {
                    return Ok(NativeAnswerLink::Conflicting);
                }
                Err(swarm_persistence::OperatorStatementError::Store(error)) => {
                    return Err(error);
                }
                Err(_) => return Ok(NativeAnswerLink::NoLongerApplicable),
            }
        }

        match self
            .store
            .resolve_operator_statement_interview(decision_id, worker, session, &ids)
        {
            Ok(true) => Ok(NativeAnswerLink::Resolved),
            Ok(false) => Ok(NativeAnswerLink::AlreadyLinked),
            Err(swarm_persistence::OperatorStatementError::Conflict) => {
                Ok(NativeAnswerLink::Conflicting)
            }
            Err(swarm_persistence::OperatorStatementError::Store(error)) => Err(error),
            // Invalid here means the decision moved, its questions changed, or
            // the set is not yet complete -- all "cannot be used", not a fault.
            Err(_) => Ok(NativeAnswerLink::Incomplete),
        }
    }
}

/// The exact target a statement must bind to.
///
/// Total on purpose: every captured question maps to a decision question, and
/// whether it matches the PENDING decision is settled by bind, which compares
/// the whole snapshot. Returning an Option here would imply a second opinion
/// about applicability that this function is not entitled to.
fn decision_target(
    decision_id: swarm_domain::DecisionRequestId,
    worker_id: swarm_domain::WorkerId,
    session_id: swarm_domain::WorkerSessionId,
    question: &swarm_domain::NativeInterviewQuestion,
) -> swarm_domain::OperatorAnswerTarget {
    swarm_domain::OperatorAnswerTarget {
        decision_id,
        worker_id,
        session_id,
        question: swarm_domain::DecisionQuestion {
            header: question.header.clone(),
            question: question.question.clone(),
            options: question.options.iter().map(|o| o.label.clone()).collect(),
            option_descriptions: question
                .options
                .iter()
                .filter(|o| !o.description.is_empty())
                .map(|o| (o.label.clone(), o.description.clone()))
                .collect(),
            multi_select: question.multi_select,
        },
    }
}
