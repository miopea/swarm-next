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

    /// A recorded deployment supersedes an unapproved claim that nothing
    /// shipped, because the two cannot both be true.
    ///
    /// Five tasks on this board carry both right now and none was ever looked
    /// at. The sharpest still reads "PR #418 is open" for work that merged and
    /// deployed — and that task was later cited as the gate for the step after
    /// it. The record says nothing shipped.
    ///
    /// The reason is PREFIXED rather than replaced: it was true when written,
    /// and what it said is how anyone later understands why the task looked
    /// finished without shipping anything.
    #[test]
    fn a_deployment_supersedes_an_unapproved_claim_that_nothing_shipped() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Close the write hole", "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        store
            .claim_completion_exemption(task.id, "Nothing shipped: PR #418 is open.", None, 1_000)
            .unwrap();

        store
            .record_task_deployment(task.id, "production", "f2059bdb", 2_000)
            .unwrap();

        let connection = store.connection().unwrap();
        let (reason, superseded): (String, Option<i64>) = connection
            .query_row(
                "SELECT reason, superseded_at FROM task_completion_exemptions WHERE task_id = ?1",
                [task.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(superseded, Some(2_000));
        assert!(reason.starts_with("SUPERSEDED"), "{reason}");
        // The original argument survives, because it explains the history.
        assert!(reason.contains("PR #418 is open"), "{reason}");
    }

    /// An APPROVED exemption is left alone.
    ///
    /// Queen accepted that argument. Quietly rewriting an accepted decision is
    /// a different and worse act than leaving a stale claim standing, and the
    /// deployment gate already prefers the deployment either way.
    #[test]
    fn an_approved_exemption_is_not_rewritten_by_a_later_deployment() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("A documented spike", "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        store
            .claim_completion_exemption(task.id, "An investigation with no code.", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(task.id, "queen", 1_500)
            .unwrap();

        store
            .record_task_deployment(task.id, "production", "abc123", 2_000)
            .unwrap();

        let connection = store.connection().unwrap();
        let (reason, superseded): (String, Option<i64>) = connection
            .query_row(
                "SELECT reason, superseded_at FROM task_completion_exemptions WHERE task_id = ?1",
                [task.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(superseded, None, "an accepted decision is not rewritten");
        assert_eq!(reason, "An investigation with no code.");
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

    /// A reviewer's dissent survives the close it could not prevent.
    ///
    /// Observed 2026-08-25, and nobody did anything wrong. Queen held 01a03944
    /// in Review for a stated reason — its own author had written that an
    /// acceptance criterion was NOT MET. Fifty-eight seconds later this sweep
    /// closed it on a deployment the worker had recorded because the task's own
    /// `next_step` prompt told it to. Three correct actions, one wrong outcome.
    ///
    /// The operator ruled the sweep keeps winning: closing shipped work with no
    /// human round trip is what makes unattended running possible, and a hold
    /// the sweep obeys can strand work whenever a reviewer forgets it. What was
    /// wrong was not the override but the ERASURE — the board showed Completed
    /// with evidence and no trace that anyone had disagreed.
    #[test]
    fn a_reviewers_hold_survives_the_sweep_that_closes_over_it() {
        let store = TaskStore::in_memory().unwrap();
        let held = task(&store);
        store
            .record_task_deployment(held, "production", "release 42", 1_000)
            .unwrap();
        store
            .hold_reviewed_work(
                held,
                &TaskActivityActor::system(),
                "the real-Hive criterion is unmet and its author said so",
                1_000,
            )
            .unwrap();

        let closed = store.complete_reviewed_work_with_deployment().unwrap();

        // The sweep still wins. That is the ruling, not a compromise.
        assert_eq!(closed.len(), 1);
        assert_eq!(store.get_task(held).unwrap().state, TaskState::Completed);

        // And the reason is on the task, in the same write that closed it —
        // not in a note somebody has to win a race to add afterwards.
        let activity = store.list_task_activity(held, 10).unwrap();
        let note = &activity.events.last().unwrap().note;
        assert!(note.contains("release 42"), "{note}");
        assert!(note.contains("over a reviewer's hold"), "{note}");
        assert!(note.contains("its author said so"), "{note}");
    }

    /// And with no hold, nothing is invented — the ordinary case is untouched.
    #[test]
    fn shipped_work_nobody_held_closes_with_nothing_to_explain() {
        let store = TaskStore::in_memory().unwrap();
        let shipped = task(&store);
        store
            .record_task_deployment(shipped, "production", "release 42", 1_000)
            .unwrap();

        store.complete_reviewed_work_with_deployment().unwrap();

        let activity = store.list_task_activity(shipped, 10).unwrap();
        assert_eq!(
            activity.events.last().unwrap().note,
            "Running in production as release 42."
        );
    }

    /// A withdrawn hold is a withdrawn opinion, and must leave no residue.
    #[test]
    fn a_released_hold_stops_being_reported() {
        let store = TaskStore::in_memory().unwrap();
        let shipped = task(&store);
        store
            .record_task_deployment(shipped, "production", "release 42", 1_000)
            .unwrap();
        store
            .hold_reviewed_work(
                shipped,
                &TaskActivityActor::system(),
                "checking one thing",
                1_000,
            )
            .unwrap();
        assert!(store.release_reviewed_work_hold(shipped).unwrap());

        store.complete_reviewed_work_with_deployment().unwrap();

        let activity = store.list_task_activity(shipped, 10).unwrap();
        assert_eq!(
            activity.events.last().unwrap().note,
            "Running in production as release 42."
        );
    }

    /// A hold that does not say why is indistinguishable from silence, and
    /// would close work with "a reviewer disagreed" and no reason — worse than
    /// no hold, because it looks like information.
    #[test]
    fn a_hold_must_say_why() {
        let store = TaskStore::in_memory().unwrap();
        let shipped = task(&store);

        assert!(
            store
                .hold_reviewed_work(shipped, &TaskActivityActor::system(), "   ", 1_000)
                .is_err()
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
    /// Records why a reviewer does not consider this work finished.
    ///
    /// Replaces any existing hold: "is this finished" has one current answer.
    ///
    /// # Errors
    /// Returns an error when the reason is blank or the hold cannot be stored.
    pub fn hold_reviewed_work(
        &self,
        task_id: TaskId,
        actor: &TaskActivityActor,
        reason: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(TaskStoreError::IntegrityFailure(
                "a hold must say why, or it is indistinguishable from silence".into(),
            ));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO task_review_holds (task_id, actor_kind, actor_id, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id) DO UPDATE SET
                 actor_kind = excluded.actor_kind,
                 actor_id = excluded.actor_id,
                 reason = excluded.reason,
                 created_at = excluded.created_at",
            params![
                task_id.to_string(),
                actor.kind.to_string(),
                actor.id.as_deref(),
                reason,
                now
            ],
        )?;
        Ok(())
    }

    /// Withdraws a hold, so ordinary shipped work closes with nothing to say.
    ///
    /// # Errors
    /// Returns an error when the hold cannot be removed.
    pub fn release_reviewed_work_hold(&self, task_id: TaskId) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "DELETE FROM task_review_holds WHERE task_id = ?1",
            [task_id.to_string()],
        )? > 0)
    }

    /// The standing hold on a task, if a reviewer set one.
    ///
    /// # Errors
    /// Returns an error when the hold cannot be read.
    pub fn reviewed_work_hold(&self, task_id: TaskId) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT reason FROM task_review_holds WHERE task_id = ?1",
                [task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?)
    }

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
    /// Records why a reviewer does not consider this work finished.
    ///
    /// Replaces any existing hold: "is this finished" has one current answer.
    ///
    /// # Errors
    /// Returns an error when the task is unknown or the hold cannot be stored.
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
            // THE SWEEP CARRIES THE DISSENT, IT DOES NOT RACE IT.
            //
            // The operator ruled that the sweep keeps winning — closing shipped
            // work without a human round trip is what makes unattended running
            // possible, and a hold the sweep obeys can strand work whenever a
            // reviewer forgets it. But a reviewer's reason must survive the
            // close, and writing it separately is what failed: on 2026-08-25 a
            // reviewer recorded evidence whose text pointed at a completion note
            // she then could not write, because this sweep had already closed
            // the task in the intervening second. The board was left with a
            // reference to information that did not exist, which reads as a
            // pointer rather than as a gap. So the note is assembled here, in
            // the same pass that closes the task, and nothing has to win a race.
            let held = self.reviewed_work_hold(task_id).unwrap_or_default();
            let note = match held
                .as_deref()
                .map(str::trim)
                .filter(|held| !held.is_empty())
            {
                Some(held) => format!(
                    "Running in {environment} as {reference}. Closed over a reviewer's hold, which said: {held}"
                ),
                None => format!("Running in {environment} as {reference}."),
            };
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

/// A reviewer's stated reason for not considering shipped work finished.
///
/// There was no way to say it. A reviewer holding work in Review cannot
/// transition it — it is already in Review — and nothing else annotates a task,
/// so a hold existed only in prose between sessions. On 2026-08-25 Queen held
/// 01a03944 for a stated reason, the shipped-work sweep closed it fifty-eight
/// seconds later on evidence the worker had been instructed to record, and the
/// board kept no trace that a reviewer had disagreed. Reading it afterwards, the
/// disagreement was not overruled; it was absent.
///
/// One hold per task, replaced rather than accumulated: this answers "is this
/// finished", which has one current answer.
pub(super) fn migrate_review_holds(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_review_holds (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             actor_kind TEXT NOT NULL,
             actor_id TEXT,
             reason TEXT NOT NULL,
             created_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         PRAGMA user_version = 91;",
    )
}
