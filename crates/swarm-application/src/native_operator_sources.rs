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
