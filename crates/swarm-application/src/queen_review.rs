use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen};
use swarm_domain::{
    QueenRecoveryFacts, QueenReviewDispositionInput, QueenRunUncovered, QueenTaskReviewEvidence,
    RecordedQueenReviewAssessment, TaskActivityActor, TaskId, UncoveredObligation,
};

impl TaskService {
    /// Record recovery evidence supplied by the server observation adapter.
    ///
    /// # Errors
    /// Refuses non-Queen callers, stale observations and unsupported assessments.
    pub fn record_queen_recovery(
        &self,
        principal: AgentPrincipal,
        input: &swarm_domain::QueenRecoveryRecord,
        observed: &swarm_domain::QueenRecoveryFacts,
        now: i64,
    ) -> Result<swarm_domain::QueenRecoveryIdentity, ApplicationError> {
        require_queen(principal)?;
        Ok(self.store.record_queen_recovery(
            input,
            observed,
            &TaskActivityActor::worker(principal.worker_id),
            now,
        )?)
    }

    /// Read consistent facts for an explicit Queen assessment.
    ///
    /// # Errors
    /// Refuses non-Queen callers or unavailable evidence.
    pub fn read_queen_review_evidence(
        &self,
        principal: AgentPrincipal,
        task_id: TaskId,
    ) -> Result<QueenTaskReviewEvidence, ApplicationError> {
        require_queen(principal)?;
        Ok(self.store.queen_task_review_snapshot(task_id)?)
    }

    /// Record checked waiting evidence without granting authority or resuming work.
    ///
    /// The state check is made here as well as in the store so a refusal
    /// reaches Queen as a refusal. From the store it reads "database integrity
    /// check failed", which points at the database rather than at the move she
    /// should make instead.
    ///
    /// # Errors
    /// Refuses unauthorized callers, stale evidence and invalid source references.
    pub fn record_queen_review_disposition(
        &self,
        principal: AgentPrincipal,
        input: &QueenReviewDispositionInput,
        now: i64,
    ) -> Result<RecordedQueenReviewAssessment, ApplicationError> {
        require_queen(principal)?;
        let task = self.store.get_task(input.task_id)?;
        if let Some(reason) = input.refusal(task.state, task.next_move_owner) {
            return Err(ApplicationError::Refused(reason.to_owned()));
        }
        Ok(self.store.record_queen_review_disposition(
            input,
            &TaskActivityActor::worker(principal.worker_id),
            now,
        )?)
    }

    /// The work a Queen run left unaccounted for, so a run that finishes
    /// incomplete says what is left (ADR 0135).
    ///
    /// In the two weeks to 2026-11-01, about 1,600 runs finished incomplete and
    /// no reply said which tasks were uncovered. A flag that is always on tells
    /// nobody anything.
    ///
    /// # Errors
    /// Refuses non-Queen callers. A storage failure marks the list
    /// `unavailable` instead of failing.
    pub fn queen_run_uncovered(
        &self,
        principal: AgentPrincipal,
        run_id: &str,
        observations: &[QueenRecoveryFacts],
        observations_complete: bool,
        now: i64,
    ) -> Result<QueenRunUncovered, ApplicationError> {
        require_queen(principal)?;
        // Read after the run is recorded: a failed read reports `unavailable`
        // rather than failing the finish (see `QueenRunUncovered::from_coverage`).
        let review = self.store.queen_run_review_coverage(run_id).ok();
        let recovery = self
            .store
            .queen_recovery_coverage(run_id, observations, observations_complete, now)
            .ok();
        Ok(QueenRunUncovered::from_coverage(
            [
                (review, UncoveredObligation::Review),
                (recovery, UncoveredObligation::Recovery),
            ],
            |task_id| {
                self.store
                    .get_task(task_id)
                    .ok()
                    .map(|task| (task.state, task.title))
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{AgentPrincipal, ApplicationError, TaskService};
    use swarm_domain::{
        QueenReviewDispositionInput, QueenReviewDispositionKind, TaskState, WorkerSessionId,
    };

    /// Both refusals reach Queen as refusals that name the move to make, not as
    /// "database integrity check failed" (ADR 0135).
    #[test]
    fn a_review_wait_and_a_draft_promotion_are_refused_with_the_move_to_make() {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let approved = store
            .create_task("Approved fixture change", "/workspace/demo")
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(approved.id, state).unwrap();
        }
        let draft = store
            .create_task("Untriaged fixture", "/workspace/demo")
            .unwrap();
        store.request_queen_automation_run(100).unwrap();
        let run = store.claim_queen_automation(100).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, 100)
            .unwrap();
        let input = QueenReviewDispositionInput {
            task_id: approved.id,
            run_id: run.run_id,
            expected_revision: store
                .queen_task_review_evidence(approved.id)
                .unwrap()
                .evidence_revision,
            kind: QueenReviewDispositionKind::ExternalCondition,
            condition: "Approved; waits for the fixture deploy".into(),
            evidence: "Read the fictional pull request".into(),
            source: "Fictional pull request".into(),
            operator_activity_sequence: None,
            operator_decision_id: None,
        };
        let service = TaskService::new(store);
        let principal = AgentPrincipal::from(&queen);

        match service.record_queen_review_disposition(principal, &input, 101) {
            Err(ApplicationError::Refused(reason)) => {
                assert!(reason.contains("awaiting_release"), "{reason}");
            }
            other => panic!("expected a refusal naming awaiting_release, got {other:?}"),
        }
        match service.promote_task(principal, draft.id) {
            Err(ApplicationError::Refused(reason)) => {
                assert!(reason.contains("swarm_transition_task"), "{reason}");
            }
            other => panic!("expected a refusal naming the transition, got {other:?}"),
        }
        assert_eq!(
            service.store().get_task(draft.id).unwrap().state,
            TaskState::Draft
        );
        let promoted = service.promote_task(principal, approved.id).unwrap();
        assert_eq!(
            promoted.state,
            TaskState::Review,
            "promotion changes no state"
        );
    }
}
