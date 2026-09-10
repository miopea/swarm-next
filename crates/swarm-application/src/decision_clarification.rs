//! Operator questions and authenticated worker explanations are not resolutions.
use crate::{AgentPrincipal, ApplicationError, TaskService};
use swarm_domain::{DecisionClarificationId, DecisionRequestId, WorkerRole};
use swarm_persistence::DecisionClarification;

impl TaskService {
    /// Queen sees explanation waits independently of primary task ownership.
    /// # Errors
    /// Denies non-Queen callers and propagates unreadable evidence.
    pub fn queen_clarification_attention(
        &self,
        principal: AgentPrincipal,
    ) -> Result<swarm_persistence::ClarificationAttention, ApplicationError> {
        crate::require_queen(principal)?;
        Ok(self.store.clarification_attention()?)
    }

    /// Only an operator-authenticated adapter may reconcile ambiguous delivery.
    /// # Errors
    /// Refuses stale claims, conflicting retries and exhausted recovery capacity.
    pub fn reconcile_operator_clarification(
        &self,
        request: &swarm_domain::ClarificationReconciliation,
        now: i64,
    ) -> Result<DecisionClarification, ApplicationError> {
        Ok(self.store.reconcile_operator_clarification(request, now)?)
    }

    /// Compact operator inbox; adapters must authenticate the operator first.
    /// # Errors
    /// Propagates persistence failures without inventing an empty inbox.
    pub fn operator_decision_inbox(
        &self,
    ) -> Result<Vec<swarm_domain::DecisionInboxEntry>, ApplicationError> {
        Ok(self.store.decision_inbox()?)
    }

    /// Called only after authenticating an operator credential, never a worker token.
    /// # Errors
    /// Rejects settled decisions, conflicting retries and exhausted bounds.
    pub fn ask_operator_clarification(
        &self,
        id: DecisionClarificationId,
        decision: DecisionRequestId,
        question: &str,
        now: i64,
    ) -> Result<DecisionClarification, ApplicationError> {
        Ok(self
            .store
            .ask_decision_clarification(id, decision, question, now)?)
    }

    /// Reads bounded history as the operator, requester or current Queen.
    /// # Errors
    /// Rejects stale worker credentials and workers outside the exchange.
    pub fn clarification_history(
        &self,
        principal: Option<AgentPrincipal>,
        decision: DecisionRequestId,
    ) -> Result<Vec<DecisionClarification>, ApplicationError> {
        if let Some(principal) = principal {
            let session = principal
                .active_session_id
                .ok_or(ApplicationError::WorkerNotRunning)?;
            let worker = self.store.get_worker_profile(principal.worker_id)?;
            if worker.active_session_id != Some(session) {
                return Err(ApplicationError::WorkerNotRunning);
            }
            let parent = self.store.get_decision_request(decision)?;
            // Use the durable role, never a caller's claimed role label.
            if worker.id != parent.requesting_worker_id && worker.role != WorkerRole::Queen {
                return Err(ApplicationError::NotAuthorized);
            }
        }
        Ok(self.store.decision_clarifications(decision)?)
    }

    /// Records an explanation, preserving both the original decision and reply source.
    /// # Errors
    /// Rejects wrong/current-session identities, conflicting retries or invalid text.
    pub fn reply_agent_clarification(
        &self,
        principal: AgentPrincipal,
        id: DecisionClarificationId,
        reply: &str,
        now: i64,
    ) -> Result<DecisionClarification, ApplicationError> {
        let session = principal
            .active_session_id
            .ok_or(ApplicationError::WorkerNotRunning)?;
        // The persistence transaction validates the current session and actual
        // requester/Queen identity together with the reply write.
        Ok(self
            .store
            .reply_decision_clarification(id, principal.worker_id, session, reply, now)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        DecisionRequestKind, DecisionRequestState, DecisionUrgency, ProviderKind, WorkerSessionId,
    };
    use swarm_persistence::{NewDecisionRequest, TaskStore};

    fn running_worker(store: &TaskStore, name: &str) -> swarm_domain::WorkerProfile {
        let worker = store
            .create_worker(
                name,
                ProviderKind::ClaudeCode,
                &format!("/fictional/{name}"),
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();
        store.get_worker_profile(worker.id).unwrap()
    }

    #[test]
    fn clarification_service_preserves_decision_and_rejects_claimed_queen_role() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/fictional/queen").unwrap();
        let requester = running_worker(&store, "Requester");
        let other = running_worker(&store, "Other");
        let queen_session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, queen_session).unwrap();
        let parent = store
            .create_decision_request(&NewDecisionRequest {
                requesting_worker_id: requester.id,
                task_id: None,
                kind: DecisionRequestKind::Input,
                urgency: DecisionUrgency::Normal,
                title: "Fictional choice",
                summary: "Which option?",
                reason: "Needs context",
                risk: "Fixture only",
                evidence: "Nothing executes",
                suggested_action: "Wait",
                allowed_actions: &["Wait".into()],
                questions: &[],
                deadline: None,
                requested_command: None,
            })
            .unwrap();
        let service = TaskService::new(store.clone());
        let id = DecisionClarificationId::new();
        service
            .ask_operator_clarification(id, parent.id, "Why?", 100)
            .unwrap();
        assert_eq!(
            service
                .queen_clarification_attention(AgentPrincipal::from(&queen))
                .unwrap()
                .total,
            1
        );
        assert!(matches!(
            service.queen_clarification_attention(AgentPrincipal::from(&requester)),
            Err(ApplicationError::NotAuthorized)
        ));
        let forged = AgentPrincipal {
            role: WorkerRole::Queen,
            ..AgentPrincipal::from(&other)
        };
        assert!(matches!(
            service.clarification_history(Some(forged), parent.id),
            Err(ApplicationError::NotAuthorized)
        ));
        assert!(
            service
                .reply_agent_clarification(forged, id, "Forged reply", 101)
                .is_err()
        );
        let principal = AgentPrincipal::from(&store.get_worker_profile(requester.id).unwrap());
        assert_eq!(
            service
                .clarification_history(Some(principal), parent.id)
                .unwrap()
                .len(),
            1
        );
        let stale = AgentPrincipal {
            active_session_id: Some(WorkerSessionId::new()),
            ..principal
        };
        assert!(matches!(
            service.clarification_history(Some(stale), parent.id),
            Err(ApplicationError::WorkerNotRunning)
        ));
        assert!(
            service
                .reply_agent_clarification(stale, id, "Stale reply", 101)
                .is_err()
        );
        let queen_principal = AgentPrincipal::from(&store.get_worker_profile(queen.id).unwrap());
        assert_eq!(
            service
                .clarification_history(Some(queen_principal), parent.id)
                .unwrap()
                .len(),
            1
        );
        let saved = service
            .reply_agent_clarification(queen_principal, id, "An explanation", 102)
            .unwrap();
        assert_eq!(saved.replying_worker_id, Some(queen.id));
        assert_eq!(saved.replying_session_id, Some(queen_session));
        assert_eq!(
            service
                .clarification_history(None, parent.id)
                .unwrap()
                .len(),
            1
        );
        let unchanged = store.get_decision_request(parent.id).unwrap();
        assert_eq!(unchanged.state, DecisionRequestState::Pending);
        assert!(unchanged.resolution_action.is_none());
    }
}
