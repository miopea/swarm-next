use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, TaskActivityActor, TaskId, TaskState, WorkerId, WorkerSessionId,
};

use super::{TaskStore, TaskStoreError, insert_control_room_event};

const MAX_OUTCOME_CLAIMS: i64 = 16;
const MAX_OUTCOME_ATTEMPTS: i64 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskOutcomeDispatch {
    pub id: String,
    pub task_id: TaskId,
    pub reporting_worker_id: WorkerId,
    pub reporting_worker_name: String,
    pub recipient_worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub title: String,
    pub target_state: TaskState,
    pub note: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskOutcomeFailure {
    Retryable,
    Uncertain,
}

impl TaskStore {
    /// Claims a bounded batch of current worker outcomes whose Queen is quiet.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn claim_task_outcomes(
        &self,
        now: i64,
    ) -> Result<Vec<TaskOutcomeDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT o.id, o.task_id, o.reporting_worker_id, reporter.name,
                        o.recipient_worker_id, session.session_id, task.title,
                        o.target_state, activity.note
                 FROM task_outcome_deliveries o
                 JOIN tasks task ON task.id = o.task_id AND task.state = o.target_state
                 JOIN task_activity activity ON activity.sequence = o.activity_sequence
                 JOIN worker_profiles reporter ON reporter.id = o.reporting_worker_id
                 JOIN worker_sessions session ON session.worker_id = o.recipient_worker_id
                     AND session.ended_at IS NULL
                 WHERE o.state = 'queued'
                   AND NOT EXISTS (
                       SELECT 1 FROM worker_engagements engagement
                       WHERE engagement.worker_id = o.recipient_worker_id
                         AND engagement.expires_at > ?1
                   )
                 ORDER BY o.updated_at, o.id LIMIT ?2",
            )?;
            statement
                .query_map(params![now, MAX_OUTCOME_CLAIMS], |row| {
                    Ok(TaskOutcomeDispatch {
                        id: row.get(0)?,
                        task_id: parse_id(&row.get::<_, String>(1)?)?,
                        reporting_worker_id: parse_id(&row.get::<_, String>(2)?)?,
                        reporting_worker_name: row.get(3)?,
                        recipient_worker_id: parse_id(&row.get::<_, String>(4)?)?,
                        session_id: parse_id(&row.get::<_, String>(5)?)?,
                        title: row.get(6)?,
                        target_state: TaskState::from_str(&row.get::<_, String>(7)?)
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        note: row.get(8)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for outcome in &candidates {
            let updated = transaction.execute(
                "UPDATE task_outcome_deliveries SET state = 'dispatching', session_id = ?2,
                     attempts = attempts + 1, attempted_at = ?3, updated_at = ?3
                 WHERE id = ?1 AND state = 'queued' AND attempts < ?4",
                params![
                    outcome.id,
                    outcome.session_id.to_string(),
                    now,
                    MAX_OUTCOME_ATTEMPTS
                ],
            )?;
            if updated != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "task outcome claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records an acknowledged Queen handoff.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_task_outcome(&self, id: &str, now: i64) -> Result<bool, TaskStoreError> {
        self.finish_task_outcome(id, now, None)
    }

    /// Records a definitive retryable failure or an ambiguous Queen handoff.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn fail_task_outcome(
        &self,
        id: &str,
        now: i64,
        failure: TaskOutcomeFailure,
    ) -> Result<bool, TaskStoreError> {
        self.finish_task_outcome(id, now, Some(failure))
    }

    /// Returns a claimed Queen handoff to its durable queue without consuming
    /// an attempt when the provider is waiting for operator input.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn defer_task_outcome(&self, id: &str, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_outcome_deliveries
             SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn finish_task_outcome(
        &self,
        id: &str,
        now: i64,
        failure: Option<TaskOutcomeFailure>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, delivered_at) = match failure {
            None => ("delivered", Some(now)),
            Some(TaskOutcomeFailure::Uncertain) => ("uncertain", None),
            Some(TaskOutcomeFailure::Retryable) => {
                let attempts: i64 = transaction.query_row(
                    "SELECT attempts FROM task_outcome_deliveries
                     WHERE id = ?1 AND state = 'dispatching'",
                    [id],
                    |row| row.get(0),
                )?;
                (
                    if attempts >= MAX_OUTCOME_ATTEMPTS {
                        "uncertain"
                    } else {
                        "queued"
                    },
                    None,
                )
            }
        };
        let changed = transaction.execute(
            "UPDATE task_outcome_deliveries
             SET state = ?2, delivered_at = ?3, updated_at = ?4
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, state, delivered_at, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Converts crash-interrupted Queen handoffs to explicit, non-retrying uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_task_outcomes(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_outcome_deliveries SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}

fn parse_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, ProviderKind, TaskOutcomeDeliveryState, TaskPriority};

    struct Fixture {
        store: TaskStore,
        task_id: TaskId,
        worker_session: WorkerSessionId,
        queen_session: WorkerSessionId,
    }

    fn active_assignment() -> Fixture {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let queen_session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, queen_session).unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let worker_session = WorkerSessionId::new();
        store
            .bind_worker_session(worker.id, worker_session)
            .unwrap();
        let task = store
            .create_task_with_details(
                "Polish mobile controls",
                "Keep voice dictation first class.",
                TaskPriority::High,
                "/workspace/petal",
            )
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task(task.id, worker_session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        Fixture {
            store,
            task_id: task.id,
            worker_session,
            queen_session,
        }
    }

    #[test]
    fn worker_handoff_waits_for_quiet_queen_and_preserves_its_note() {
        let fixture = active_assignment();
        fixture
            .store
            .renew_worker_engagement(
                fixture.queen_session,
                Some(PresenceDeviceId::new()),
                100,
                300,
            )
            .unwrap();
        let task = fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Blocked,
                "Need the Android keyboard behavior confirmed.",
                fixture.worker_session,
            )
            .unwrap();
        assert_eq!(
            task.outcome_delivery_state,
            Some(TaskOutcomeDeliveryState::Queued)
        );
        assert!(fixture.store.claim_task_outcomes(101).unwrap().is_empty());

        let outcomes = fixture.store.claim_task_outcomes(401).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].session_id, fixture.queen_session);
        assert_eq!(outcomes[0].reporting_worker_name, "Petal");
        assert_eq!(outcomes[0].target_state, TaskState::Blocked);
        assert_eq!(
            outcomes[0].note,
            "Need the Android keyboard behavior confirmed."
        );
        let activity = fixture
            .store
            .list_task_activity(fixture.task_id, 100)
            .unwrap();
        assert_eq!(activity.events.last().unwrap().note, outcomes[0].note);

        assert!(
            fixture
                .store
                .complete_task_outcome(&outcomes[0].id, 402)
                .unwrap()
        );
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task_id)
                .unwrap()
                .outcome_delivery_state,
            Some(TaskOutcomeDeliveryState::Delivered)
        );
    }

    #[test]
    fn newer_transition_cancels_a_stale_queued_handoff() {
        let fixture = active_assignment();
        fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Blocked,
                "Temporary blocker.",
                fixture.worker_session,
            )
            .unwrap();
        fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Active,
                "Resolved locally.",
                fixture.worker_session,
            )
            .unwrap();

        assert!(fixture.store.claim_task_outcomes(100).unwrap().is_empty());
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task_id)
                .unwrap()
                .outcome_delivery_state,
            None
        );
    }

    #[test]
    fn completed_handoff_is_hidden_after_the_task_moves_on() {
        let fixture = active_assignment();
        fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Blocked,
                "Temporary blocker.",
                fixture.worker_session,
            )
            .unwrap();
        let outcomes = fixture.store.claim_task_outcomes(100).unwrap();
        fixture
            .store
            .complete_task_outcome(&outcomes[0].id, 101)
            .unwrap();

        fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Active,
                "Resolved locally.",
                fixture.worker_session,
            )
            .unwrap();

        assert_eq!(
            fixture
                .store
                .get_task(fixture.task_id)
                .unwrap()
                .outcome_delivery_state,
            None
        );
    }
    #[test]
    fn crash_ambiguity_never_replays_a_queen_handoff() {
        let fixture = active_assignment();
        fixture
            .store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Review,
                "Tests and deployment are complete.",
                fixture.worker_session,
            )
            .unwrap();
        assert_eq!(fixture.store.claim_task_outcomes(100).unwrap().len(), 1);
        assert_eq!(fixture.store.recover_inflight_task_outcomes().unwrap(), 1);
        assert!(fixture.store.claim_task_outcomes(101).unwrap().is_empty());
        assert_eq!(
            fixture
                .store
                .get_task(fixture.task_id)
                .unwrap()
                .outcome_delivery_state,
            Some(TaskOutcomeDeliveryState::Uncertain)
        );
    }
}

/// What a task has to show before it may be called done.
///
/// "Completed" and "deployed" are different claims, and the product's own
/// shipping vocabulary depends on not confusing them. This is the durable
/// answer to which one a task can make.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletionEvidence {
    /// At least one recorded deployment. The task shipped.
    Deployed,
    /// A worker claimed there is nothing to deploy and Queen approved it.
    ExemptionApproved,
    /// A worker claimed there is nothing to deploy. Nobody has agreed yet.
    ExemptionClaimed,
    /// Neither. Nothing has shown this work to be anywhere.
    None,
}

impl CompletionEvidence {
    /// Whether this is enough to close a task without further judgment.
    ///
    /// A claimed-but-unapproved exemption deliberately is not. The worker
    /// asserting its own work needs no evidence is the one claim that cannot
    /// also be the approval of that claim.
    #[must_use]
    pub const fn closes_a_task(&self) -> bool {
        matches!(self, Self::Deployed | Self::ExemptionApproved)
    }
}

impl TaskStore {
    /// What this task can show for itself.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn completion_evidence(
        &self,
        task_id: TaskId,
    ) -> Result<CompletionEvidence, TaskStoreError> {
        let connection = self.connection()?;
        let deployed: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_deployments WHERE task_id = ?1)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if deployed {
            return Ok(CompletionEvidence::Deployed);
        }
        let exemption: Option<Option<i64>> = connection
            .query_row(
                "SELECT approved_at FROM task_completion_exemptions WHERE task_id = ?1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match exemption {
            None => CompletionEvidence::None,
            Some(None) => CompletionEvidence::ExemptionClaimed,
            Some(Some(_)) => CompletionEvidence::ExemptionApproved,
        })
    }

    /// Records a worker's claim that a task has nothing to deploy.
    ///
    /// Replaces an unapproved claim by the same route, so a worker can correct
    /// its own reason. It cannot overwrite one Queen has already approved:
    /// changing the argument after it was accepted would make the approval
    /// vouch for something nobody read.
    ///
    /// # Errors
    /// Rejects an empty reason, or a claim on an already-approved exemption.
    pub fn claim_completion_exemption(
        &self,
        task_id: TaskId,
        reason: &str,
        worker_id: Option<WorkerId>,
        now: i64,
    ) -> Result<CompletionEvidence, TaskStoreError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        let connection = self.connection()?;
        let approved: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_completion_exemptions
             WHERE task_id = ?1 AND approved_at IS NOT NULL)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if approved {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        connection.execute(
            "INSERT INTO task_completion_exemptions
                 (task_id, reason, claimed_by_worker_id, claimed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(task_id) DO UPDATE
                 SET reason = excluded.reason,
                     claimed_by_worker_id = excluded.claimed_by_worker_id,
                     claimed_at = excluded.claimed_at",
            params![
                task_id.to_string(),
                reason,
                worker_id.map(|id| id.to_string()),
                now
            ],
        )?;
        drop(connection);
        self.completion_evidence(task_id)
    }

    /// Approves a claimed exemption, which is what lets the task close.
    ///
    /// # Errors
    /// Returns an error when no exemption has been claimed.
    pub fn approve_completion_exemption(
        &self,
        task_id: TaskId,
        approver: &str,
        now: i64,
    ) -> Result<CompletionEvidence, TaskStoreError> {
        if !matches!(approver, "queen" | "operator") {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{approver} cannot approve a completion exemption"
            )));
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE task_completion_exemptions
             SET approved_at = ?2, approved_by = ?3
             WHERE task_id = ?1 AND approved_at IS NULL",
            params![task_id.to_string(), now, approver],
        )?;
        drop(connection);
        if updated == 0 {
            let evidence = self.completion_evidence(task_id)?;
            if evidence == CompletionEvidence::ExemptionApproved {
                return Ok(evidence);
            }
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        self.completion_evidence(task_id)
    }

    /// The reason a worker gave for a task having nothing to deploy.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn completion_exemption_reason(
        &self,
        task_id: TaskId,
    ) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT reason FROM task_completion_exemptions WHERE task_id = ?1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod completion_evidence_tests {
    use super::*;
    use crate::TaskStore;

    /// A task far enough along to have something to say about deployment.
    /// Recording one is only allowed from Review or Completed.
    fn task(store: &TaskStore) -> TaskId {
        let id = store
            .create_task("Ship the thing", "/projects/app")
            .unwrap()
            .id;
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(id, state).unwrap();
        }
        id
    }

    /// "We need to make sure a task is deployed before we close it, doesn't
    /// matter if it is email, jira or swarm local." A task that shipped can
    /// show it; one that did nothing cannot.
    #[test]
    fn only_a_deployment_or_an_approved_exemption_closes_a_task() {
        let store = TaskStore::in_memory().unwrap();

        let nothing = task(&store);
        assert_eq!(
            store.completion_evidence(nothing).unwrap(),
            CompletionEvidence::None
        );
        assert!(!store.completion_evidence(nothing).unwrap().closes_a_task());

        let shipped = task(&store);
        store
            .record_task_deployment(shipped, "production", "v0.2.0", 1_000)
            .unwrap();
        assert_eq!(
            store.completion_evidence(shipped).unwrap(),
            CompletionEvidence::Deployed
        );
        assert!(store.completion_evidence(shipped).unwrap().closes_a_task());
    }

    /// The worker asserting its own work needs no evidence cannot also be the
    /// one who accepts that assertion.
    #[test]
    fn a_claimed_exemption_does_not_close_a_task_until_it_is_approved() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);

        let claimed = store
            .claim_completion_exemption(
                spike,
                "Investigated; the reported defect does not reproduce.",
                None,
                1_000,
            )
            .unwrap();
        assert_eq!(claimed, CompletionEvidence::ExemptionClaimed);
        assert!(!claimed.closes_a_task());

        let approved = store
            .approve_completion_exemption(spike, "queen", 2_000)
            .unwrap();
        assert_eq!(approved, CompletionEvidence::ExemptionApproved);
        assert!(approved.closes_a_task());
    }

    /// An assertion with no argument behind it is what this gate exists to
    /// stop, so an empty reason is not a claim.
    #[test]
    fn an_exemption_without_a_reason_is_not_a_claim() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);

        assert!(
            store
                .claim_completion_exemption(spike, "   ", None, 1_000)
                .is_err()
        );
        assert_eq!(
            store.completion_evidence(spike).unwrap(),
            CompletionEvidence::None
        );
    }

    /// Rewriting the reason after Queen accepted it would make her approval
    /// vouch for something nobody read.
    #[test]
    fn an_approved_exemption_cannot_have_its_reason_rewritten() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);
        store
            .claim_completion_exemption(spike, "A duplicate of an earlier task.", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(spike, "queen", 2_000)
            .unwrap();

        assert!(
            store
                .claim_completion_exemption(spike, "Actually it shipped, trust me.", None, 3_000)
                .is_err()
        );
        assert_eq!(
            store.completion_exemption_reason(spike).unwrap().as_deref(),
            Some("A duplicate of an earlier task.")
        );
    }

    /// A worker correcting its own argument before anyone accepted it is fine.
    #[test]
    fn an_unapproved_exemption_can_be_restated() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);
        store
            .claim_completion_exemption(spike, "No idea yet.", None, 1_000)
            .unwrap();
        store
            .claim_completion_exemption(spike, "Documentation only; nothing runs.", None, 2_000)
            .unwrap();

        assert_eq!(
            store.completion_exemption_reason(spike).unwrap().as_deref(),
            Some("Documentation only; nothing runs.")
        );
        assert!(
            store
                .approve_completion_exemption(spike, "operator", 3_000)
                .is_ok()
        );
    }

    /// The operator's ruling: the coordinator approves when the evidence is
    /// well-formed, and Queen sees only what needs judgment. Deployed work is
    /// the well-formed case and the common one.
    #[test]
    fn reviewed_work_that_shipped_is_closed_without_asking_queen() {
        let store = TaskStore::in_memory().unwrap();
        let shipped = task(&store);
        store
            .record_task_deployment(shipped, "production", "release 42", 1_000)
            .unwrap();

        let closed = store.complete_reviewed_work_with_deployment().unwrap();

        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].task_id, shipped);
        assert_eq!(closed[0].reference, "release 42");
        assert_eq!(store.get_task(shipped).unwrap().state, TaskState::Completed);

        // The note is derived from the evidence, not a claim about the work.
        let activity = store.list_task_activity(shipped, 10).unwrap();
        assert_eq!(
            activity.events.last().unwrap().note,
            "Running in production as release 42."
        );

        // And it does not close the same work twice.
        assert!(
            store
                .complete_reviewed_work_with_deployment()
                .unwrap()
                .is_empty()
        );
    }

    /// Work with nothing to show, and work whose exemption nobody approved,
    /// both stay in review. The second is the case that matters: a worker
    /// cannot approve its own claim by leaving it lying there.
    #[test]
    fn reviewed_work_without_settled_evidence_is_left_for_queen() {
        let store = TaskStore::in_memory().unwrap();
        let nothing = task(&store);
        let claimed = task(&store);
        store
            .claim_completion_exemption(claimed, "Documentation only.", None, 1_000)
            .unwrap();

        assert!(
            store
                .complete_reviewed_work_with_deployment()
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.get_task(nothing).unwrap().state, TaskState::Review);
        assert_eq!(store.get_task(claimed).unwrap().state, TaskState::Review);

        let waiting = store.reviewed_work_awaiting_judgment().unwrap();
        assert!(waiting.contains(&nothing));
        assert!(waiting.contains(&claimed));

        // Once Queen approves the claim, it is settled and no longer waiting —
        // but the coordinator still does not close it, because there is no
        // deployment. Approving the exemption is the approval.
        store
            .approve_completion_exemption(claimed, "queen", 2_000)
            .unwrap();
        assert!(
            !store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&claimed)
        );
    }

    #[test]
    fn approving_an_exemption_nobody_claimed_is_refused() {
        let store = TaskStore::in_memory().unwrap();
        let unclaimed = task(&store);
        assert!(
            store
                .approve_completion_exemption(unclaimed, "queen", 1_000)
                .is_err()
        );
        assert!(
            store
                .approve_completion_exemption(unclaimed, "the worker", 1_000)
                .is_err()
        );
    }
}

/// One task the coordinator closed, and what it closed it on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeterministicCompletion {
    pub task_id: TaskId,
    pub title: String,
    pub environment: String,
    pub reference: String,
}

impl TaskStore {
    /// Closes reviewed work that can show where it is running.
    ///
    /// The operator's ruling on who approves a completion: the coordinator
    /// does it when the evidence is well-formed, and Queen sees only what needs
    /// judgment. This is the well-formed case, and it is the common one — so
    /// putting Queen on it would mean a model call per completion and a Hive
    /// that stops closing anything when she is unavailable.
    ///
    /// Nothing here decides whether the work was good. It decides that a
    /// deployment was recorded, which is a fact in a table, not an opinion. A
    /// task with no evidence, or with an exemption nobody has approved, is left
    /// in review for Queen.
    ///
    /// # Errors
    /// Returns a persistence error. A task that cannot be closed is skipped
    /// rather than aborting the pass, because one stuck row should not stop
    /// every other completion.
    pub fn complete_reviewed_work_with_deployment(
        &self,
    ) -> Result<Vec<DeterministicCompletion>, TaskStoreError> {
        let candidates = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT task.id, task.title, deployment.environment, deployment.reference
                 FROM tasks task
                 JOIN task_deployments deployment ON deployment.task_id = task.id
                 WHERE task.state = ?1
                   AND task.removed_at IS NULL
                   AND deployment.id = (
                       SELECT newest.id FROM task_deployments newest
                       WHERE newest.task_id = task.id
                       ORDER BY newest.deployed_at DESC, newest.recorded_at DESC, newest.id DESC
                       LIMIT 1
                   )
                 ORDER BY task.updated_at",
            )?;
            let rows = statement.query_map([TaskState::Review.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut closed = Vec::new();
        for (id, title, environment, reference) in candidates {
            let Ok(task_id) = TaskId::from_str(&id) else {
                continue;
            };
            // The note is derived from the evidence rather than written about
            // it. "Verified" would be a claim this pass is not entitled to
            // make; where it is running is what was actually established.
            let note = format!("Running in {environment} as {reference}.");
            match self.transition_task_with_note_as(
                task_id,
                TaskState::Completed,
                &note,
                &TaskActivityActor::system(),
            ) {
                Ok(_) => closed.push(DeterministicCompletion {
                    task_id,
                    title,
                    environment,
                    reference,
                }),
                Err(TaskStoreError::NotFound | TaskStoreError::InvalidTransition { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(closed)
    }

    /// Reviewed work the coordinator will not close: no deployment, or an
    /// exemption nobody has approved.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn reviewed_work_awaiting_judgment(&self) -> Result<Vec<TaskId>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT task.id FROM tasks task
             WHERE task.state = ?1
               AND task.removed_at IS NULL
               AND NOT EXISTS (SELECT 1 FROM task_deployments d WHERE d.task_id = task.id)
               AND NOT EXISTS (
                   SELECT 1 FROM task_completion_exemptions e
                   WHERE e.task_id = task.id AND e.approved_at IS NOT NULL
               )
             ORDER BY task.updated_at",
        )?;
        let rows = statement.query_map([TaskState::Review.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|id| TaskId::from_str(&id).ok())
            .collect())
    }
}
