use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{
    CommitRepositoryState, CommitSettlement, CommitVerdict, ControlRoomEventKind,
    TaskActivityActor, TaskCommit, TaskCommitReport, TaskId, TaskState, WorkerId, WorkerSessionId,
    commit_settlement,
};

use super::{TaskStore, TaskStoreError, insert_control_room_event};

const MAX_OUTCOME_CLAIMS: i64 = 16;
const MAX_OUTCOME_ATTEMPTS: i64 = 3;
const MAX_REVIEW_SETTLEMENT_CANDIDATES: usize = 64;

/// One bounded coordinator page. The owner resumes at `next_cursor`, or wraps
/// to the beginning when it is None. Unresolved candidates still advance it.
pub struct ReviewedSettlementPage {
    pub closed: Vec<TaskId>,
    pub checked: usize,
    pub next_cursor: Option<TaskId>,
}

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
        // Claiming is internal delivery ownership, not new task progress.
        // Completion, failure and recovery publish the observable transition.
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
        // A guarded deferral leaves the same handoff pending. Its blocker is
        // published by the refusal owner, not by every retry of this queue.
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

    #[test]
    fn held_outcome_retries_are_quiet_but_delivery_and_recovery_publish() {
        for recover in [false, true] {
            let fixture = active_assignment();
            let store = &fixture.store;
            store
                .transition_worker_task(
                    fixture.task_id,
                    TaskState::Blocked,
                    "Need a ruling",
                    fixture.worker_session,
                )
                .unwrap();
            let cursor = store.list_control_room_events(0).unwrap().next_cursor;
            for now in 100..110 {
                let outcome = store.claim_task_outcomes(now).unwrap().remove(0);
                assert!(store.defer_task_outcome(&outcome.id, now).unwrap());
            }
            let outcome = store.claim_task_outcomes(110).unwrap().remove(0);
            assert!(
                store
                    .list_control_room_events(cursor)
                    .unwrap()
                    .events
                    .is_empty()
            );
            if recover {
                assert_eq!(store.recover_inflight_task_outcomes().unwrap(), 1);
                assert_eq!(store.recover_inflight_task_outcomes().unwrap(), 0);
            } else {
                assert!(store.complete_task_outcome(&outcome.id, 111).unwrap());
            }
            let page = store.list_control_room_events(cursor).unwrap();
            assert_eq!(page.events.len(), 1);
            assert_eq!(page.events[0].kind, ControlRoomEventKind::TasksChanged);
        }
    }

    #[test]
    fn outcome_holds_follow_exact_delivery_and_task_state() {
        let fixture = active_assignment();
        let store = &fixture.store;
        store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Blocked,
                "Need a ruling",
                fixture.worker_session,
            )
            .unwrap();
        let first = store.claim_task_outcomes(100).unwrap().remove(0);
        let subject = format!("outcome-delivery:{}", first.id);
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &format!("task-outcome:{}", fixture.task_id),
                Some(first.recipient_worker_id),
                None,
                "legacy",
                100,
            )
            .unwrap();
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &subject,
                Some(first.recipient_worker_id),
                Some(first.session_id),
                "first",
                101,
            )
            .unwrap();
        store.defer_task_outcome(&first.id, 102).unwrap();
        assert_eq!(
            store
                .standing_coordinator_refusals(10_000, 0)
                .unwrap()
                .len(),
            1
        );
        store
            .transition_task(fixture.task_id, TaskState::Active)
            .unwrap();
        assert!(
            store
                .standing_coordinator_refusals(10_000, 0)
                .unwrap()
                .is_empty()
        );
        store
            .transition_worker_task(
                fixture.task_id,
                TaskState::Blocked,
                "Different question",
                fixture.worker_session,
            )
            .unwrap();
        let next = store.claim_task_outcomes(103).unwrap().remove(0);
        assert_ne!(first.id, next.id);
        let next_subject = format!("outcome-delivery:{}", next.id);
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD_UNSENT_TEXT,
                &next_subject,
                Some(next.recipient_worker_id),
                Some(next.session_id),
                "next",
                104,
            )
            .unwrap();
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &subject,
                Some(first.recipient_worker_id),
                Some(first.session_id),
                "late old",
                105,
            )
            .unwrap();
        let holds = store.standing_coordinator_refusals(10_000, 0).unwrap();
        assert_eq!(holds.len(), 1);
        assert_eq!(holds[0].subject, next_subject);
        store.complete_task_outcome(&next.id, 106).unwrap();
        store
            .record_coordinator_refusal(
                crate::REFUSAL_DELIVERY_HELD,
                &format!("task-outcome:{}", fixture.task_id),
                Some(next.recipient_worker_id),
                None,
                "late legacy",
                107,
            )
            .unwrap();
        assert!(
            store
                .standing_coordinator_refusals(10_000, 0)
                .unwrap()
                .is_empty()
        );
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
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
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

    /// The whole spine: every waiting state says who owes the next move.
    ///
    /// Derived rather than stored, so it cannot drift from the state and the
    /// assignment it describes. The one stored input is Queen handing reviewed
    /// work back, because that is a decision somebody made rather than a
    /// consequence of anything.
    #[test]
    fn every_waiting_state_names_who_owes_the_next_move() {
        use swarm_domain::NextMoveOwner;
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Some work", "/workspace").unwrap();

        // Unassigned and unstarted: Queen routes it.
        store.transition_task(task.id, TaskState::Ready).unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Queen
        );

        let worker = store
            .create_worker("Petal", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store.assign_task(task.id, session).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Worker
        );

        // NOT Queen. Blocked is the harder reason — a task waiting on another
        // task — and naming Queen here would bury those in her queue.
        store.transition_task(task.id, TaskState::Blocked).unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Blocked
        );

        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Queen,
            "finished work waits on Queen to judge it"
        );

        // THE SEND-BACK. The task does not move; the debt does.
        store
            .return_review_to_worker(task.id, "Say which SHA this shipped as.", 1_000)
            .unwrap();
        let returned = store.get_task(task.id).unwrap();
        assert_eq!(
            returned.state,
            TaskState::Review,
            "it must NOT move backwards"
        );
        assert_eq!(returned.next_move_owner, NextMoveOwner::Worker);
        let messages = store.task_messages(task.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Say which SHA this shipped as.");
        assert_eq!(messages[0].recipient_worker_id, Some(worker.id.to_string()));
        assert_eq!(messages[0].delivered_at, None);

        store.answer_returned_review(task.id, 2_000).unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Queen,
            "answering hands the move back"
        );

        // Waiting on an event, not on a person.
        store
            .transition_task(task.id, TaskState::AwaitingRelease)
            .unwrap();
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            NextMoveOwner::Release
        );
    }

    /// Work cannot be handed back before anyone has finished it.
    #[test]
    fn only_reviewed_work_can_be_returned_to_its_worker() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Some work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();

        assert!(
            store
                .return_review_to_worker(task.id, "Evidence please.", 1_000)
                .is_err(),
            "active work is already the worker's move; there is nothing to hand back"
        );
    }

    #[test]
    fn failed_review_message_does_not_transfer_responsibility_and_can_retry() {
        for failed_table in ["task_messages", "control_room_events"] {
            let fixture = active_assignment();
            let store = &fixture.store;
            let task_id = fixture.task_id;
            store.transition_task(task_id, TaskState::Review).unwrap();
            let event_count = || -> i64 {
                store
                    .connection()
                    .unwrap()
                    .query_row("SELECT COUNT(*) FROM control_room_events", [], |row| {
                        row.get(0)
                    })
                    .unwrap()
            };
            let before_events = event_count();
            store
                .connection()
                .unwrap()
                .execute_batch(&format!(
                    "CREATE TEMP TRIGGER reject_review_message BEFORE INSERT ON {failed_table}
             BEGIN SELECT RAISE(ABORT, 'injected hand-back failure'); END;"
                ))
                .unwrap();
            assert!(
                store
                    .return_review_to_worker(task_id, "Which SHA shipped?", 100)
                    .is_err()
            );
            assert_eq!(
                store.get_task(task_id).unwrap().next_move_owner,
                swarm_domain::NextMoveOwner::Queen
            );
            assert!(store.task_messages(task_id).unwrap().is_empty());
            let markers: i64 = store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM task_returned_reviews WHERE task_id = ?1",
                    [task_id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(markers, 0);
            assert_eq!(event_count(), before_events);
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TRIGGER reject_review_message")
                .unwrap();
            store
                .return_review_to_worker(task_id, "Which SHA shipped?", 101)
                .unwrap();
            assert_eq!(
                store.get_task(task_id).unwrap().next_move_owner,
                swarm_domain::NextMoveOwner::Worker
            );
            assert_eq!(store.task_messages(task_id).unwrap().len(), 1);
            assert_eq!(event_count(), before_events + 1);
        }
    }

    #[test]
    fn invalid_review_message_preserves_the_previous_request_and_answer() {
        let fixture = active_assignment();
        let store = &fixture.store;
        let task_id = fixture.task_id;
        store.transition_task(task_id, TaskState::Review).unwrap();
        store
            .return_review_to_worker(task_id, "Which SHA shipped?", 100)
            .unwrap();
        store.answer_returned_review(task_id, 101).unwrap();
        // A schema-valid character count can still exceed the message byte cap.
        let oversized = "é".repeat(2_001);
        assert!(matches!(
            store.return_review_to_worker(task_id, &oversized, 102),
            Err(TaskStoreError::InvalidTaskMessage { .. })
        ));
        let marker: (String, i64, Option<i64>) = store.connection().unwrap().query_row(
            "SELECT request, returned_at, answered_at FROM task_returned_reviews WHERE task_id = ?1",
            [task_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(marker, ("Which SHA shipped?".to_owned(), 100, Some(101)));
        assert_eq!(
            store.get_task(task_id).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        assert_eq!(store.task_messages(task_id).unwrap().len(), 1);
    }

    #[test]
    fn unassigned_review_has_no_worker_to_owe_an_answer() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Review", "/workspace").unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(task.id, state).unwrap();
        }
        assert!(matches!(
            store.return_review_to_worker(task.id, "Which SHA?", 100),
            Err(TaskStoreError::WorkerNotFound)
        ));
        assert_eq!(
            store.get_task(task.id).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        assert!(store.task_messages(task.id).unwrap().is_empty());
    }

    /// Evidence is readable for a task whose activity log shows none.
    ///
    /// The exact shape that misled a reader on the real board: an exemption
    /// claimed and approved, a task still in review, and an activity log
    /// containing created/ready/assigned/active/review and nothing else. The
    /// log was accurate; the conclusion drawn from it — that no evidence
    /// existed — was not.
    #[test]
    fn evidence_is_readable_even_though_no_activity_event_records_it() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("A spike", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task.id, "An investigation with no code.", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(task.id, "queen", "Read the handoff.", 1_500)
            .unwrap();

        // THE LOG STILL SAYS NOTHING, and that is deliberate: nothing was
        // backfilled, so every earlier reading of a record like this one stays
        // reproducible.
        let events = store.list_task_activity(task.id, 50).unwrap();
        assert!(
            !events.events.iter().any(|event| format!("{:?}", event.kind)
                .to_lowercase()
                .contains("exempt")),
            "no exemption event should have been invented"
        );

        // The evidence is reported anyway, because it is read from where it
        // actually lives.
        let evidence = store.task_evidence_record(task.id).unwrap();
        let exemption = evidence.exemption.expect("the claim is readable");
        assert_eq!(exemption.claimed_at, 1_000);
        assert_eq!(exemption.approved_at, Some(1_500));
        assert_eq!(exemption.approved_by.as_deref(), Some("queen"));
        assert!(evidence.deployments.is_empty());
    }

    /// And a task with no evidence does not grow a phantom claim.
    ///
    /// The negative half. A reader has to be able to tell "none recorded" from
    /// "recorded somewhere this does not look", which is the whole defect.
    #[test]
    fn a_task_with_no_evidence_reports_none_rather_than_inventing_it() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Ordinary work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();

        let evidence = store.task_evidence_record(task.id).unwrap();
        assert!(evidence.exemption.is_none());
        assert!(evidence.deployments.is_empty());
    }

    /// An unapproved claim reads as standing, not settled.
    ///
    /// This is the distinction that cost another worker a wrong correction:
    /// their claim WAS recorded, and the attention row still standing against
    /// it is what an unapproved claim looks like rather than a missing one.
    #[test]
    fn a_claim_awaiting_approval_is_readable_as_unapproved() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("A documented spike", "/workspace")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task.id, "Nothing to deploy.", None, 2_000)
            .unwrap();

        let exemption = store
            .task_evidence_record(task.id)
            .unwrap()
            .exemption
            .expect("a claim with no approval is still a record");
        assert_eq!(exemption.claimed_at, 2_000);
        assert_eq!(
            exemption.approved_at, None,
            "standing, not settled — and the attention asking for it stays up"
        );
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
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task.id, "An investigation with no code.", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(task.id, "queen", "Read the handoff.", 1_500)
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

/// Everything recorded about a task's completion evidence, for reading rather
/// than deciding.
///
/// Evidence does not live in `task_activity`: claiming an exemption, approving
/// one, and recording a deployment all write their own tables and no event. So
/// a reader asking "what happened to this task" through the activity log sees
/// an accurate list of transitions and no evidence at all, which is how an
/// approval that existed came to be reported as missing.
///
/// DERIVED ON READ, NEVER WRITTEN. Backfilling events for past approvals would
/// make every previous reading of those records unreproducible; this reports
/// what the evidence tables already hold, so records written before it existed
/// read correctly too.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskEvidenceRecord {
    pub exemption: Option<CompletionExemptionRecord>,
    pub deployments: Vec<crate::TaskDeploymentRecord>,
}

/// A no-deployment claim and whatever has since happened to it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletionExemptionRecord {
    pub reason: String,
    pub claimed_by_worker_id: Option<String>,
    pub claimed_at: i64,
    /// Set when a coordinator agreed. Until then the claim is standing, not
    /// settled, and the attention asking for it to be judged stays up.
    pub approved_at: Option<i64>,
    pub approved_by: Option<String>,
    /// What the approver said this rests on. `None` for approvals recorded
    /// before citing one was required — absent, not empty.
    pub approved_basis: Option<String>,
    pub superseded_at: Option<i64>,
    /// Set when the claim was taken back. Distinct from `superseded_at`: that
    /// one means a deployment contradicted the claim, this one means its author
    /// or a coordinator retracted it while nothing shipped.
    pub withdrawn_at: Option<i64>,
    pub withdrawn_by: Option<String>,
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
        // A WITHDRAWN CLAIM IS NOT EVIDENCE, INCLUDING AN APPROVED ONE. The row
        // stays — the honest history is that it was claimed and then taken back,
        // and the approval stays legible beside it — but it stops answering for
        // the task. This is read as "no valid claim on this task", which is the
        // state a worker who realised their claim was wrong previously had no
        // way to say: their only options were to leave it standing or record
        // another false one.
        //
        // Deliberately NOT keyed on superseded_at. That column means a
        // deployment contradicted the claim, and a task with a deployment has
        // already returned Deployed above, so folding the two here would be
        // dead code that reads like a rule.
        let exemption: Option<(Option<i64>, Option<i64>)> = connection
            .query_row(
                "SELECT approved_at, withdrawn_at
                 FROM task_completion_exemptions WHERE task_id = ?1",
                [task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(match exemption {
            None | Some((_, Some(_))) => CompletionEvidence::None,
            Some((None, None)) => CompletionEvidence::ExemptionClaimed,
            Some((Some(_), None)) => CompletionEvidence::ExemptionApproved,
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
        // THE FACTS AND THE CLAIM MUST NOT DISAGREE. A worker saying its task
        // had nothing to deploy, over commits that reached a ref and touched
        // code, is the one case where a person genuinely adds something --
        // refusing it here is what earns the automation everywhere else.
        //
        // ONLY ON `BuiltCode`. `Unknown` is not a contradiction: nobody
        // reported, or something could not be checked, and refusing on a
        // question never asked would block every worker in a workspace that is
        // not a checkout. The refusal is narrow on purpose.
        //
        // The operator is not stranded by this. `approve_completion_exemption`
        // and `record_task_unverifiable` are both still open to them, which is
        // what makes this a route to a person rather than a dead end.
        match commit_settlement(self.task_commit_report(task_id)?.as_ref()) {
            CommitSettlement::BuiltCode => {
                return Err(TaskStoreError::CommitsContradictNoDeployment);
            }
            // ⚠️ AND NOBODY REPORTING IS NOT THE SAME AS A REPORT THAT SETTLES
            // NOTHING. Folding these together is what made reporting the only
            // route to a refusal. Unestablished still claims, which is what
            // keeps a non-checkout workspace able to record an outcome at all.
            CommitSettlement::NotReported => {
                return Err(TaskStoreError::CommitsNotReported);
            }
            CommitSettlement::Unestablished
            | CommitSettlement::NothingBuilt
            | CommitSettlement::DocumentationOnly => {}
        }
        let connection = self.connection()?;
        let approved: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_completion_exemptions
             WHERE task_id = ?1 AND approved_at IS NOT NULL AND withdrawn_at IS NULL)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if approved {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        // A CLAIM MADE ON A TASK THAT ALREADY DEPLOYED IS BORN SUPERSEDED, and
        // it has to carry the same mark as one superseded from the other
        // direction.
        //
        // The deployment side of this was fixed first and only covered
        // claim-then-deploy. Three claims corrected by hand on 2026-08-26 took
        // the other route — deploy-then-claim — and left superseded_at NULL, so
        // the contradiction query still returned them. A human reading the
        // reason could see the correction; the query could not, which is the
        // entire thing a structured field exists to avoid.
        //
        // Decided from the deployment record rather than from the reason text.
        // All three corrections happen to begin "SUPERSEDES", and matching on
        // that would be reading prose to determine a fact the store already
        // holds — and would break the moment somebody phrased it differently.
        let deployed: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_deployments WHERE task_id = ?1)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        let superseded_at = deployed.then_some(now);
        connection.execute(
            "INSERT INTO task_completion_exemptions
                 (task_id, reason, claimed_by_worker_id, claimed_at, superseded_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id) DO UPDATE
                 SET reason = excluded.reason,
                     claimed_by_worker_id = excluded.claimed_by_worker_id,
                     claimed_at = excluded.claimed_at,
                     superseded_at = excluded.superseded_at,
                     -- A RE-CLAIM AFTER A WITHDRAWAL IS A NEW CLAIM, so the
                     -- withdrawal must not survive it. Leaving these set would
                     -- mint a claim that is born retracted: completion_evidence
                     -- reads withdrawn_at and would report None for a claim
                     -- somebody just made, which is a worse silence than the one
                     -- withdrawal exists to fix.
                     withdrawn_at = NULL,
                     withdrawn_by = NULL",
            params![
                task_id.to_string(),
                reason,
                worker_id.map(|id| id.to_string()),
                now,
                superseded_at
            ],
        )?;
        drop(connection);
        self.completion_evidence(task_id)
    }

    /// Takes back a no-deployment claim, so the task has no valid claim again.
    ///
    /// THE HONEST STATE HAD NO WAY TO BE SAID. A worker who realised its claim
    /// was wrong had two options and both were false: leave it standing, or
    /// record another one. One such claim on this board carries a note whose
    /// first line reads DO NOT APPROVE THIS CLAIM, because a note was the only
    /// instrument there was.
    ///
    /// WHO MAY. The worker that claimed it, because a retraction by its author
    /// needs no adjudication. Queen or the operator for anyone's, because the
    /// author is often gone or stood down by the time the claim is found wrong
    /// — and because the worst case is a claim a coordinator already APPROVED,
    /// which no worker should be able to undo and which its author cannot reach.
    /// An approved claim is therefore coordinator-only, and that is the one rule
    /// here that is about authority rather than shape.
    ///
    /// NOTHING IS REWRITTEN. The reason, the claimant and the approval all stay
    /// exactly as recorded; only `withdrawn_at` and `withdrawn_by` are added.
    /// "Claimed, then withdrawn" is the history, and it is more useful to a
    /// later reader than a row that was quietly made to look like it never
    /// existed.
    ///
    /// # Errors
    /// Returns an error when no claim exists, when it is already withdrawn, or
    /// when the actor may not withdraw this particular claim.
    pub fn withdraw_completion_exemption(
        &self,
        task_id: TaskId,
        actor: &str,
        now: i64,
    ) -> Result<CompletionEvidence, TaskStoreError> {
        let connection = self.connection()?;
        let existing: Option<(Option<i64>, Option<i64>, Option<String>)> = connection
            .query_row(
                "SELECT approved_at, withdrawn_at, claimed_by_worker_id
                 FROM task_completion_exemptions WHERE task_id = ?1",
                [task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((approved_at, withdrawn_at, claimed_by)) = existing else {
            return Err(TaskStoreError::IntegrityFailure(
                "there is no no-deployment claim on this task to withdraw".to_owned(),
            ));
        };
        if withdrawn_at.is_some() {
            return Err(TaskStoreError::IntegrityFailure(
                "this no-deployment claim has already been withdrawn".to_owned(),
            ));
        }
        let coordinator = matches!(actor, "queen" | "operator");
        let author = claimed_by.as_deref() == Some(actor);
        if approved_at.is_some() && !coordinator {
            return Err(TaskStoreError::IntegrityFailure(
                "an approved no-deployment claim is withdrawn by Queen or the operator, \
                 not by the worker that claimed it"
                    .to_owned(),
            ));
        }
        if !coordinator && !author {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{actor} did not make this no-deployment claim and cannot withdraw it"
            )));
        }
        let updated = connection.execute(
            "UPDATE task_completion_exemptions
             SET withdrawn_at = ?2, withdrawn_by = ?3
             WHERE task_id = ?1 AND withdrawn_at IS NULL",
            params![task_id.to_string(), now, actor],
        )?;
        drop(connection);
        if updated == 0 {
            return Err(TaskStoreError::IntegrityFailure(
                "this no-deployment claim has already been withdrawn".to_owned(),
            ));
        }
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
        basis: &str,
        now: i64,
    ) -> Result<CompletionEvidence, TaskStoreError> {
        // AN APPROVAL THAT CITES NOTHING IS A CLICK. The rule says somebody
        // other than the author checked; a basis is what makes that a claim
        // anyone can later find wrong. "I could not verify" is a legitimate
        // basis and is accepted; saying nothing is not.
        let basis = basis.trim();
        if basis.is_empty() {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        // "coordinator" IS NAMED RATHER THAN BORROWED. The deterministic pass
        // approves what it settled on facts in tables, and the record says so
        // -- writing "queen" would be the sweep claiming a person looked, which
        // is the vocabulary this design is not allowed to weaken.
        if !matches!(approver, "queen" | "operator" | "coordinator") {
            return Err(TaskStoreError::IntegrityFailure(format!(
                "{approver} cannot approve a completion exemption"
            )));
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            "UPDATE task_completion_exemptions
             SET approved_at = ?2, approved_by = ?3, approved_basis = ?4
             WHERE task_id = ?1 AND approved_at IS NULL",
            params![task_id.to_string(), now, approver, basis],
        )?;
        drop(connection);
        if updated == 0 {
            let evidence = self.completion_evidence(task_id)?;
            if evidence == CompletionEvidence::ExemptionApproved {
                return Ok(evidence);
            }
            // NOTHING TO APPROVE IS NOT AN INADEQUATE BASIS. Zero rows updated
            // with no approved exemption means no claim exists -- the worker
            // never made one, or it was refused. Reporting that as
            // CompletionEvidenceRequired sent an approver back to rewrite a
            // basis that was never the problem, four times.
            return Err(TaskStoreError::NoCompletionExemptionToApprove);
        }
        self.completion_evidence(task_id)
    }

    /// Hands reviewed work back to its worker with a named request.
    ///
    /// THE TASK DOES NOT MOVE. Returning it to Ready is what invalidated a
    /// valid evidence claim on 2026-09-01, because Ready means UNSTARTED to
    /// everything that reads it; returning it to Active makes finished work
    /// look unfinished. What changes is who owes the next move.
    ///
    /// The next-move marker and the request message commit together. The current
    /// assignee is read inside that transaction, never supplied by the caller.
    ///
    /// # Errors
    /// Refuses unassigned work, work not in review, invalid messages, and
    /// persistence failures. A failed message leaves the next mover unchanged.
    pub fn return_review_to_worker(
        &self,
        task_id: TaskId,
        request: &str,
        now: i64,
    ) -> Result<super::TaskMessage, TaskStoreError> {
        let request = request.trim();
        if request.is_empty() {
            return Err(TaskStoreError::CompletionEvidenceRequired);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, worker_id): (String, Option<String>) = transaction
            .query_row(
                "SELECT state, assigned_worker_id FROM tasks WHERE id = ?1 AND removed_at IS NULL",
                [task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        if state != TaskState::Review.to_string() {
            return Err(TaskStoreError::InvalidTransition {
                from: TaskState::from_str(&state).unwrap_or(TaskState::Draft),
                to: TaskState::Review,
            });
        }
        let worker_id = worker_id.ok_or(TaskStoreError::WorkerNotFound)?;
        let worker_id = WorkerId::from_str(&worker_id)
            .map_err(|_| TaskStoreError::IntegrityFailure("invalid review assignee".to_owned()))?;
        crate::message_delivery::cancel_queued_review(&transaction, task_id)?;
        transaction.execute(
            "INSERT INTO task_returned_reviews (task_id, request, returned_at, answered_at)
             VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(task_id) DO UPDATE
               SET request = excluded.request,
                   returned_at = excluded.returned_at,
                   answered_at = NULL",
            params![task_id.to_string(), request, now],
        )?;
        let message = Self::insert_task_message(
            &transaction,
            task_id,
            super::MessageEnd::queen(),
            super::MessageEnd::worker(worker_id),
            request,
            now,
        )?;
        transaction.execute(
            "UPDATE task_returned_reviews SET request_message_id = ?2,
                 request_worker_id = ?3, answer_message_id = NULL WHERE task_id = ?1",
            params![task_id.to_string(), message.id, worker_id.to_string()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(message)
    }

    /// Marks a returned review answered, so the next move is Queen's again.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    #[cfg(test)]
    fn answer_returned_review(&self, task_id: TaskId, now: i64) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_returned_reviews SET answered_at = ?2
             WHERE task_id = ?1 AND answered_at IS NULL",
            params![task_id.to_string(), now],
        )?;
        Ok(())
    }

    /// Reads a task's completion evidence: the claim, its approval, and any
    /// recorded deployments.
    ///
    /// Companion to the activity log rather than part of it. See
    /// [`TaskEvidenceRecord`] for why this is derived rather than written.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn task_evidence_record(
        &self,
        task_id: TaskId,
    ) -> Result<TaskEvidenceRecord, TaskStoreError> {
        let exemption = {
            let connection = self.connection()?;
            connection
                .query_row(
                    "SELECT reason, claimed_by_worker_id, claimed_at, approved_at,
                            approved_by, superseded_at, approved_basis,
                            withdrawn_at, withdrawn_by
                     FROM task_completion_exemptions WHERE task_id = ?1",
                    [task_id.to_string()],
                    |row| {
                        Ok(CompletionExemptionRecord {
                            reason: row.get(0)?,
                            claimed_by_worker_id: row.get(1)?,
                            claimed_at: row.get(2)?,
                            approved_at: row.get(3)?,
                            approved_by: row.get(4)?,
                            superseded_at: row.get(5)?,
                            approved_basis: row.get(6)?,
                            withdrawn_at: row.get(7)?,
                            withdrawn_by: row.get(8)?,
                        })
                    },
                )
                .optional()?
        };
        Ok(TaskEvidenceRecord {
            exemption,
            deployments: self.task_deployments(task_id)?,
        })
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
    use swarm_domain::ProviderKind;

    /// A task far enough along to have something to say about deployment.
    /// Recording one is only allowed from Review or Completed.
    /// A reviewed task on which a no-deployment claim is LEGAL.
    ///
    /// ⚠️ IT REPORTS AN EMPTY COMMIT LIST, and that is not incidental setup.
    /// Since 2026-09-04 a claim needs a commit report to stand on — silence used
    /// to be the one route that always passed, which made reporting your commits
    /// the only way to get refused. An empty list is the documented way to say
    /// "nothing was built"; it settles as `NothingBuilt` and the claim proceeds.
    ///
    /// The tests using this are about what happens TO a claim — approval,
    /// withdrawal, supersession, readback — not about whether one may be made.
    /// The two that assert the precondition build their own task instead, so
    /// this helper cannot quietly satisfy the thing they exist to check.
    ///
    /// Upsert, not replace: a later real report adds to this rather than being
    /// masked by it, so tests that report actual commits are unaffected.
    fn task(store: &TaskStore) -> TaskId {
        let id = store
            .create_task("Ship the thing", "/projects/app")
            .unwrap()
            .id;
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(id, state).unwrap();
        }
        store
            .record_task_commits(id, "/projects/app", CommitRepositoryState::Read, &[], 900)
            .unwrap();
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
            .approve_completion_exemption(spike, "queen", "Read the handoff.", 2_000)
            .unwrap();
        assert_eq!(approved, CompletionEvidence::ExemptionApproved);
        assert!(approved.closes_a_task());
    }

    /// THE HONEST STATE IS NOW SAYABLE, and this is the test that says it.
    ///
    /// A claim is a fact about a MOMENT and nothing carried that expiry. The
    /// worker with the clearest case wrote "Investigation only — no code
    /// written", which was true when the ticket was investigation-only and false
    /// the moment the same worker was told to build it. Their two options were
    /// to leave it standing or record another false one; "there is no valid
    /// claim on this task" could not be expressed at all.
    #[test]
    fn a_withdrawn_claim_stops_being_evidence_and_the_record_still_says_it_happened() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);
        store
            .claim_completion_exemption(spike, "Investigation only, no code written.", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(spike, "queen", "Read the handoff.", 2_000)
            .unwrap();
        assert!(
            store.completion_evidence(spike).unwrap().closes_a_task(),
            "the approved claim is evidence until it is taken back"
        );

        let after = store
            .withdraw_completion_exemption(spike, "queen", 3_000)
            .unwrap();
        assert_eq!(
            after,
            CompletionEvidence::None,
            "a withdrawn claim answers for nothing"
        );
        assert!(!after.closes_a_task());

        // NOTHING IS REWRITTEN. "Claimed, then withdrawn" is the honest history
        // and it is more use to a later reader than a row quietly made to look
        // as though it never existed.
        let record = store
            .task_evidence_record(spike)
            .unwrap()
            .exemption
            .expect("the row survives its own withdrawal");
        assert_eq!(record.reason, "Investigation only, no code written.");
        assert_eq!(record.approved_at, Some(2_000), "the approval still stands");
        assert_eq!(record.withdrawn_at, Some(3_000));
        assert_eq!(record.withdrawn_by.as_deref(), Some("queen"));
        assert_eq!(
            record.superseded_at, None,
            "nothing shipped, so this is a retraction and NOT a supersession"
        );
    }

    /// WITHDRAWAL AND SUPERSESSION ARE DIFFERENT ACTS and the store has to keep
    /// them apart. `superseded_at` already means one thing — a recorded
    /// deployment contradicted the claim — written from both directions by the
    /// store itself as a statement of fact. A withdrawal is a person retracting
    /// while nothing shipped. Reusing the column would have been the easy
    /// implementation and would have destroyed the distinction this task named
    /// in its own title.
    #[test]
    fn a_deployment_supersedes_and_a_person_withdraws_and_the_two_do_not_share_a_column() {
        let store = TaskStore::in_memory().unwrap();
        let shipped = task(&store);
        store
            .claim_completion_exemption(shipped, "Nothing to deploy.", None, 1_000)
            .unwrap();
        store
            .record_task_deployment(shipped, "production", "sha abc123", 1_100)
            .unwrap();
        let superseded = store
            .task_evidence_record(shipped)
            .unwrap()
            .exemption
            .expect("the claim is kept and marked");
        assert_eq!(superseded.superseded_at, Some(1_100));
        assert_eq!(
            superseded.withdrawn_at, None,
            "a deployment contradicts a claim; it does not retract it"
        );
    }

    /// An approved claim is the worst case and the one no worker may undo.
    ///
    /// Approving is the single act that removes a task from the detector
    /// watching it, so an approval taken back has to be a coordinator's. The
    /// author cannot reach it, and by then the author is often gone anyway.
    #[test]
    fn who_may_withdraw_depends_on_whether_a_coordinator_already_agreed() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Sorrel",
                ProviderKind::ClaudeCode,
                "/workspace/sorrel",
                false,
                1,
            )
            .unwrap();
        let stranger = store
            .create_worker(
                "Bracken",
                ProviderKind::ClaudeCode,
                "/workspace/bracken",
                false,
                2,
            )
            .unwrap();

        let own = task(&store);
        store
            .claim_completion_exemption(own, "No code.", Some(worker.id), 1_000)
            .unwrap();
        assert!(
            store
                .withdraw_completion_exemption(own, &stranger.id.to_string(), 1_100)
                .is_err(),
            "a worker cannot retract somebody else's claim"
        );
        store
            .withdraw_completion_exemption(own, &worker.id.to_string(), 1_200)
            .expect("its author needs nobody's permission to take it back");

        let agreed = task(&store);
        store
            .claim_completion_exemption(agreed, "No code.", Some(worker.id), 1_000)
            .unwrap();
        store
            .approve_completion_exemption(agreed, "queen", "Read the handoff.", 1_100)
            .unwrap();
        assert!(
            store
                .withdraw_completion_exemption(agreed, &worker.id.to_string(), 1_200)
                .is_err(),
            "a worker does not undo a coordinator's approval"
        );
        store
            .withdraw_completion_exemption(agreed, "queen", 1_300)
            .expect("the coordinator who approved it can take it back");
        assert!(
            store
                .withdraw_completion_exemption(agreed, "queen", 1_400)
                .is_err(),
            "withdrawing twice is not a thing that happened twice"
        );
    }

    /// A RE-CLAIM AFTER A WITHDRAWAL IS A NEW CLAIM, not a resurrection of the
    /// old one. Without clearing the withdrawal the upsert mints a claim that is
    /// born retracted — `completion_evidence` would read it as None while a
    /// worker had just made it, which is a worse silence than the one this whole
    /// change exists to fix.
    #[test]
    fn a_claim_made_after_a_withdrawal_is_live() {
        let store = TaskStore::in_memory().unwrap();
        let spike = task(&store);
        store
            .claim_completion_exemption(spike, "First, and wrong.", None, 1_000)
            .unwrap();
        store
            .withdraw_completion_exemption(spike, "queen", 1_100)
            .unwrap();
        let again = store
            .claim_completion_exemption(spike, "Second, and true.", None, 1_200)
            .unwrap();
        assert_eq!(
            again,
            CompletionEvidence::ExemptionClaimed,
            "the new claim stands on its own"
        );
        let record = store
            .task_evidence_record(spike)
            .unwrap()
            .exemption
            .unwrap();
        assert_eq!(record.withdrawn_at, None);
        assert_eq!(record.withdrawn_by, None);
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
            .approve_completion_exemption(spike, "queen", "Read the handoff.", 2_000)
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
                .approve_completion_exemption(spike, "operator", "Read the handoff.", 3_000)
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
    /// A DEPLOYMENT THAT SAYS IT IS PARTIAL IS NOT COMPLETION EVIDENCE.
    ///
    /// 2026-09-02, 02:08. A worker moved B7 to Review with a handoff opening
    /// "TWO OF THE THREE ACCEPTANCE LINES ARE NOT MET AND I AM NOT CLAIMING
    /// THEM", recorded a TRUE deployment for the half that had shipped, and
    /// this sweep closed the whole ticket one second later. Completed is
    /// terminal. The worker did everything right; the record could not express
    /// partial delivery, so a deployment meant "done" whatever they wrote.
    ///
    /// THIS IS NOT THE REVIEWER'S-HOLD CASE ABOVE, and the distinction is the
    /// fix. A hold is somebody's OPINION that work is unfinished, and the
    /// operator ruled the sweep must override it — a hold the sweep obeys can
    /// strand work whenever a reviewer forgets it. This is the EVIDENCE
    /// declaring its own scope, written by the author of the deployment in the
    /// same act. The sweep's premise is "a deployment means this shipped";
    /// against a partial one that premise is absent, so there is nothing to act
    /// on rather than something to override.
    #[test]
    fn a_partial_deployment_does_not_close_the_task_it_only_half_delivers() {
        let store = TaskStore::in_memory().unwrap();
        let partial = task(&store);
        store
            .record_partial_task_deployment(partial, "production", "the platform half only", 1_000)
            .unwrap();

        let closed = store.complete_reviewed_work_with_deployment().unwrap();

        assert!(
            closed.is_empty(),
            "a deployment that says it delivers part of the task is not evidence the task is done"
        );
        assert_eq!(
            store.get_task(partial).unwrap().state,
            TaskState::Review,
            "it stays in Review for the coordinator, which costs one action and is recoverable — \
             Completed is terminal and is not"
        );
    }

    /// And the sweep still closes FULLY delivered work with nobody in the loop.
    /// Trading one silent failure for a queue of manual approvals would be the
    /// worse outcome, and it is the thing the ticket explicitly warned against.
    #[test]
    fn a_whole_delivery_still_closes_itself() {
        let store = TaskStore::in_memory().unwrap();
        let whole = task(&store);
        store
            .record_task_deployment(whole, "production", "release 42", 1_000)
            .unwrap();

        let closed = store.complete_reviewed_work_with_deployment().unwrap();

        assert_eq!(
            closed.len(),
            1,
            "unattended closing is what makes this work"
        );
        assert_eq!(store.get_task(whole).unwrap().state, TaskState::Completed);
    }

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

    /// Work that owes somebody a reply is not closed automatically.
    ///
    /// THE OPERATOR'S RULE: "there should always be a reply included if a reply
    /// is needed." Before this, the coordinator settled any reviewed task with a
    /// recorded deployment — correctly by the rule it had — and an email task
    /// whose worker had not written the reply went straight to completed. The
    /// operator then found a card offering them a blank box and a "Write the
    /// reply" button, on work that was finished and deployed.
    ///
    /// It happened on 01a04f90 minutes after the gate that made drafting
    /// possible at Review was shipped, by the worker that shipped it — which is
    /// the argument for a rule in the query rather than a habit in a worker.
    ///
    /// SKIPPED, NOT REFUSED: the task stays in Review, where the Hive already
    /// surfaces it, and the next tick closes it once a reply exists. Refusing
    /// would stall every other completion behind one unanswered thread.
    #[test]
    fn work_that_owes_a_reply_is_left_in_review_rather_than_closed() {
        let store = TaskStore::in_memory().unwrap();
        let plain = task(&store);
        let emailed = task(&store);
        // The email link is what makes somebody the audience for this work.
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO email_message_links (
                     task_id, integration_id, message_id, conversation_id,
                     internet_message_id, sender_name, sender_address, received_at, web_url
                 ) VALUES (?1, 'outlook', 'm-1', 'c-1', '<m1@test>', 'Bradford', 'b@test', 1, 'https://example.test')",
                [emailed.to_string()],
            )
            .unwrap();
        for id in [plain, emailed] {
            store
                .record_task_deployment(id, "production", "release 42", 1_000)
                .unwrap();
        }

        let closed = store.complete_reviewed_work_with_deployment().unwrap();

        // The ordinary task closes on its evidence, exactly as before.
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].task_id, plain);
        assert_eq!(store.get_task(plain).unwrap().state, TaskState::Completed);
        // The emailed one waits for its reply, with the same evidence.
        assert_eq!(store.get_task(emailed).unwrap().state, TaskState::Review);

        // AND IT IS NOT STUCK. Once the reply exists the next tick closes it —
        // a rule that could only be satisfied by never using email would be a
        // worse defect than the one it replaced.
        store
            .prepare_email_reply(emailed, "Fixed, and running on your Hive.")
            .unwrap();
        let closed_now = store.complete_reviewed_work_with_deployment().unwrap();
        assert_eq!(closed_now.len(), 1);
        assert_eq!(closed_now[0].task_id, emailed);
        assert_eq!(store.get_task(emailed).unwrap().state, TaskState::Completed);
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
            .approve_completion_exemption(claimed, "queen", "Read the handoff.", 2_000)
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
                .approve_completion_exemption(unclaimed, "queen", "Read the handoff.", 1_000)
                .is_err()
        );
        assert!(
            store
                .approve_completion_exemption(unclaimed, "the worker", "Read the handoff.", 1_000)
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
                 -- ONLY A DEPLOYMENT THAT CLAIMS THE WHOLE TASK COUNTS.
                 --
                 -- This sweep's premise is that a deployment record means the
                 -- work shipped, which is why closing on it needs no human. A
                 -- deployment recorded as partial does not carry that premise,
                 -- so there is nothing here to act on. It is not an override
                 -- being obeyed -- see the reviewer's-hold case, which the
                 -- operator ruled this sweep keeps winning -- it is the
                 -- evidence declaring its own scope.
                 --
                 -- A task carrying BOTH a partial and a whole deployment still
                 -- closes, on the whole one, which is the honest reading.
                 JOIN task_deployments deployment ON deployment.task_id = task.id
                   AND deployment.delivers_whole_task = 1
                 WHERE task.state = ?1
                   AND task.removed_at IS NULL
                   AND deployment.id = (
                       SELECT newest.id FROM task_deployments newest
                       WHERE newest.task_id = task.id
                       ORDER BY newest.deployed_at DESC, newest.recorded_at DESC, newest.id DESC
                       LIMIT 1
                   )
                   -- WORK THAT OWES SOMEBODY A REPLY IS NOT WELL-FORMED, so the
                   -- coordinator leaves it in review rather than closing it.
                   --
                   -- This is the path that actually closed 01a04f90: a worker
                   -- recorded its deployment, handed off, and the coordinator
                   -- settled it seconds later — correctly, by the rule it had.
                   -- The operator then found a card offering them a blank box
                   -- and a Write the reply button, on work that was finished and
                   -- deployed. Their words: \"there should always be a reply
                   -- included if a reply is needed.\"
                   --
                   -- SKIPPED, NOT REFUSED. Erroring here would stall every other
                   -- completion behind one unanswered thread; leaving it in
                   -- review is already a state the Hive surfaces, and a worker
                   -- can write the reply and let the next tick close it.
                   AND NOT EXISTS (
                       SELECT 1 FROM email_message_links link
                       WHERE link.task_id = task.id
                         AND NOT EXISTS (
                             SELECT 1 FROM email_reply_deliveries reply
                             WHERE reply.task_id = task.id
                         )
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
                   WHERE e.task_id = task.id AND e.approved_at IS NOT NULL AND e.withdrawn_at IS NULL
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

#[cfg(test)]
mod awaiting_judgment_subject_tests {
    use super::*;
    use crate::TaskStore;

    fn reviewed(store: &TaskStore, title: &str) -> TaskId {
        let task = store.create_task(title, "/workspace/petal").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        task.id
    }

    /// THE EXACT DEFECT THAT PRODUCED 49 WHERE THE ANSWER WAS 31.
    ///
    /// The original query counted unapproved exemption claims without excluding
    /// tasks that ALSO carried a deployment record. Nineteen properly closed,
    /// fully evidenced tasks were counted as unverified because a rotted claim
    /// sat beside real evidence — and the wrong number was then relayed to the
    /// operator as the sharpest fact in the complaint.
    ///
    /// This fails if the population ever admits that row again.
    #[test]
    fn a_stale_claim_beside_a_real_deployment_is_not_waiting_on_anyone() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed(&store, "Shipped, and carries a rotted claim");
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task, "Thought there was nothing to deploy", None, 1_000)
            .unwrap();
        store
            .record_task_deployment(task, "production", "sha abc123", 1_100)
            .unwrap();

        assert!(
            !store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&task),
            "a deployment settles the task; the unapproved claim beside it is untidy, not unsettled"
        );
    }

    #[test]
    fn work_the_coordinator_settled_is_not_waiting_on_anyone() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed(&store, "Investigation the sweep closed");
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task, "Nothing was built", None, 1_000)
            .unwrap();
        store
            .approve_completion_exemption(task, "coordinator", "Read the handoff.", 1_100)
            .unwrap();

        assert!(
            !store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&task),
            "an approved exemption settles it, whoever approved"
        );
    }

    /// And the population is not empty of the thing it IS about, which is the
    /// other way a count can be wrong while every exclusion is correct.
    #[test]
    fn work_carrying_nothing_is_waiting_on_someone() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed(&store, "Nobody has settled this");
        assert!(
            store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&task)
        );

        // An unapproved claim on its own does NOT settle it, which is the
        // distinction the whole evidence model rests on.
        // A claim needs a commit report to stand on since 2026-09-04;
        // an empty list is the documented "nothing was built".
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        store
            .claim_completion_exemption(task, "Nothing to deploy", None, 1_000)
            .unwrap();
        assert!(
            store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&task),
            "a claim nobody approved leaves the work waiting"
        );
    }
}

#[cfg(test)]
mod settlement_tests {
    use super::*;
    use crate::TaskStore;

    /// A reviewed task that has REPORTED NOTHING. Used by the two tests that
    /// assert the reporting precondition, which need exactly that.
    fn reviewed_task(store: &TaskStore, title: &str) -> TaskId {
        let task = store.create_task(title, "/workspace/petal").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.transition_task(task.id, TaskState::Review).unwrap();
        task.id
    }

    /// A reviewed task that has ALREADY REPORTED, so a no-deployment claim on it
    /// is legal.
    ///
    /// ⚠️ USE THIS WHEREVER THE TEST IS ABOUT WHAT HAPPENS TO A CLAIM, not about
    /// whether one may be made. Since 2026-09-04 a claim needs a commit report
    /// to stand on; an empty list is the documented way to say nothing was
    /// built, and it settles as `NothingBuilt`. Tests of approval, withdrawal
    /// and supersession all take that route so they exercise the shape the
    /// tools actually permit.
    ///
    /// `reviewed_task` deliberately still reports NOTHING, because the tests
    /// that assert the precondition need a task that has not reported.
    fn claimable_task(store: &TaskStore, title: &str) -> TaskId {
        let task = reviewed_task(store, title);
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                900,
            )
            .unwrap();
        task
    }

    fn commit(paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: format!("sha{}", paths.len()),
            verdict: CommitVerdict::Present,
            subject: "did a thing".to_owned(),
            changed_paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn settlement_pages_advance_past_unresolved_work_and_wrap() {
        let store = TaskStore::in_memory().unwrap();
        let mut ids: Vec<_> = (0..=MAX_REVIEW_SETTLEMENT_CANDIDATES)
            .map(|index| reviewed_task(&store, &format!("Candidate {index}")))
            .collect();
        ids.sort_by_key(ToString::to_string);
        let last = *ids.last().unwrap();
        store
            .record_task_commits(
                last,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                1_000,
            )
            .unwrap();
        let first = store
            .settle_reviewed_work_without_deployment_page(2_000, None)
            .unwrap();
        assert_eq!(first.checked, MAX_REVIEW_SETTLEMENT_CANDIDATES);
        assert!(first.closed.is_empty());
        assert!(first.next_cursor.is_some());
        let second = store
            .settle_reviewed_work_without_deployment_page(2_001, first.next_cursor)
            .unwrap();
        assert_eq!(second.checked, 1);
        assert_eq!(second.closed, vec![last]);
        assert!(second.next_cursor.is_none());
        assert_eq!(store.get_task(ids[0]).unwrap().state, TaskState::Review);
        let wrapped = store
            .settle_reviewed_work_without_deployment_page(2_002, second.next_cursor)
            .unwrap();
        assert_eq!(wrapped.checked, MAX_REVIEW_SETTLEMENT_CANDIDATES);
        assert!(wrapped.closed.is_empty());
        // An exhausted cursor returns a bounded empty page, not a restart loop.
        let end = store
            .settle_reviewed_work_without_deployment_page(2_003, wrapped.next_cursor)
            .unwrap();
        assert_eq!(end.checked, 0);
        assert!(end.next_cursor.is_none());
    }

    #[test]
    fn work_that_built_nothing_closes_without_a_human() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Investigate a report");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                1_000,
            )
            .unwrap();

        let settled = store
            .settle_reviewed_work_without_deployment(2_000)
            .unwrap();

        assert_eq!(settled, vec![task]);
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Completed);
        // AND IT RECORDS WHY IT WAS ENTITLED TO. Without this the task closes
        // carrying nothing, which is the shape the board asks somebody to
        // chase -- the clicking would come back one layer up.
        assert_eq!(
            store.completion_evidence(task).unwrap(),
            CompletionEvidence::ExemptionApproved
        );
    }

    #[test]
    fn documentation_only_work_closes_without_a_human() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Write the design up");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit(&["docs/41-verification.md"])],
                1_000,
            )
            .unwrap();

        assert_eq!(
            store
                .settle_reviewed_work_without_deployment(2_000)
                .unwrap(),
            vec![task]
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Completed);
    }

    /// The dangerous default, asserted directly.
    ///
    /// A task nobody reported must NOT settle. If it did, work whose worker
    /// simply forgot to report would close itself as an investigation that
    /// produced nothing -- on a question never asked.
    #[test]
    fn work_nobody_reported_is_left_alone() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Nobody said anything about this");

        assert!(
            store
                .settle_reviewed_work_without_deployment(2_000)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
        assert!(
            store
                .reviewed_work_awaiting_judgment()
                .unwrap()
                .contains(&task),
            "it must still reach a person"
        );
    }

    #[test]
    fn work_that_built_code_is_left_for_a_person() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Shipped a fix");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit(&["crates/swarm-api/src/lib.rs"])],
                1_000,
            )
            .unwrap();

        assert!(
            store
                .settle_reviewed_work_without_deployment(2_000)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
    }

    /// THE REFUSAL. A worker saying "nothing to deploy" over commits that
    /// touched code is the one case a person genuinely improves.
    #[test]
    fn a_claim_the_commits_contradict_is_refused() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Claimed nothing shipped");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit(&["crates/swarm-api/src/lib.rs"])],
                1_000,
            )
            .unwrap();

        assert!(matches!(
            store.claim_completion_exemption(task, "Nothing to deploy", None, 2_000),
            Err(TaskStoreError::CommitsContradictNoDeployment)
        ));
        assert_eq!(
            store.completion_evidence(task).unwrap(),
            CompletionEvidence::None,
            "a refused claim must leave no record behind"
        );
    }

    /// ⚠️ AN APPROVAL WITH NOTHING TO APPROVE MUST NOT BLAME THE BASIS.
    ///
    /// A refused claim leaves NO exemption row, so a later approval updates
    /// zero rows. That used to return `CompletionEvidenceRequired` — "completed
    /// work requires concise verification evidence" — which points at the one
    /// thing the approver got right.
    ///
    /// Measured: Queen attempted this four times on 01a06b37-eac8, rewriting
    /// the basis each time, before working out that the worker's claim had been
    /// REFUSED and there was nothing to countersign. The remedy is never a
    /// better basis; it is a claim existing, or the operator writing the task
    /// off as unverifiable.
    #[test]
    fn approving_a_claim_that_was_never_recorded_says_so_rather_than_blaming_the_basis() {
        let store = TaskStore::in_memory().unwrap();
        let task = claimable_task(&store, "Claim was refused");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit(&["src/app/api/log/route.ts"])],
                1_000,
            )
            .unwrap();
        // Refused, exactly as it was on the real task, so no row exists.
        assert!(matches!(
            store.claim_completion_exemption(task, "Comment-only", None, 2_000),
            Err(TaskStoreError::CommitsContradictNoDeployment)
        ));

        let approving =
            store.approve_completion_exemption(task, "queen", "A basis that is fine", 3_000);
        assert!(
            matches!(
                approving,
                Err(TaskStoreError::NoCompletionExemptionToApprove)
            ),
            "an approver with a good basis must be told the CLAIM is missing: {approving:?}"
        );

        // And the other error keeps its own meaning: an empty basis IS the
        // basis being wrong, and must not be relabelled by this change.
        let task = claimable_task(&store, "Claim exists");
        store
            .claim_completion_exemption(task, "Investigation only", None, 2_000)
            .unwrap();
        assert!(matches!(
            store.approve_completion_exemption(task, "queen", "   ", 3_000),
            Err(TaskStoreError::CompletionEvidenceRequired)
        ));
    }

    /// The refusal is NARROW, and THIS is the case that makes it so — the only
    /// one, since 2026-09-04.
    ///
    /// A report that cannot be checked is not a contradiction. The workspace may
    /// not be a checkout at all, and refusing here would block every worker
    /// outside version control from ever recording an outcome. That is the
    /// original intent of this test and it is unchanged.
    ///
    /// ⚠️ ITS OTHER HALF MOVED, AND DELIBERATELY. This test also asserted that a
    /// task with NO report may claim. That is now refused — see
    /// `an_unreported_task_cannot_claim_that_nothing_was_deployed` below. The
    /// two used to be one `Unknown`, which is what made reporting your commits
    /// the only route to a refusal.
    #[test]
    fn a_report_that_settles_nothing_is_not_refused() {
        let store = TaskStore::in_memory().unwrap();
        let unreadable = reviewed_task(&store, "Not a git checkout");
        store
            .record_task_commits(
                unreadable,
                "/workspace/plain",
                CommitRepositoryState::NotARepository,
                &[TaskCommit {
                    sha: "aaa1111".to_owned(),
                    verdict: CommitVerdict::Unchecked,
                    subject: String::new(),
                    changed_paths: Vec::new(),
                }],
                1_000,
            )
            .unwrap();
        store
            .claim_completion_exemption(unreadable, "Nothing to deploy", None, 2_000)
            .expect("a workspace with no repository must not be refused");
    }

    /// ⚠️ THE INCENTIVE RAN BACKWARDS AND THIS IS WHERE IT IS TURNED ROUND.
    ///
    /// Measured on two tasks that made the SAME comment-only `.ts` change
    /// minutes apart: rcg-admin had no commit report and its no-deployment claim
    /// was approved; rcg-wifi-portal reported one commit and was refused on it.
    /// They did not receive different judgements of the same facts — they
    /// presented different facts, and the honest one was penalised.
    ///
    /// The remedy in the error is genuinely one call. An EMPTY list settles as
    /// `NothingBuilt` and passes, which the third assertion here proves, so a
    /// worker that truly built nothing is never stuck.
    ///
    /// FALSE NEGATIVE, STATED RATHER THAN DISCOVERED LATER: a worker that DID
    /// build code and reports an empty list still passes. That is a lie rather
    /// than a gap and it was equally possible before; what changes is that the
    /// omission is now an explicit recorded claim instead of a silent absence.
    #[test]
    fn an_unreported_task_cannot_claim_that_nothing_was_deployed() {
        let store = TaskStore::in_memory().unwrap();
        let unreported = reviewed_task(&store, "Nobody reported commits");
        assert!(
            matches!(
                store.claim_completion_exemption(unreported, "Investigation only", None, 2_000),
                Err(TaskStoreError::CommitsNotReported)
            ),
            "silence must not be the cheap route past a check that reporting triggers"
        );
        assert_eq!(
            store.completion_evidence(unreported).unwrap(),
            CompletionEvidence::None,
            "a refused claim must leave no record behind"
        );

        // AND THE ONE CALL THE ERROR NAMES ACTUALLY WORKS. Reporting an empty
        // list is a documented answer meaning nothing was built; it settles as
        // NothingBuilt and the same claim then passes.
        store
            .record_task_commits(
                unreported,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                2_500,
            )
            .unwrap();
        store
            .claim_completion_exemption(unreported, "Investigation only", None, 3_000)
            .expect("an explicit 'nothing was built' must pass");
    }

    /// A STATE THE PRODUCT DELIBERATELY REPORTS, kept reachable.
    ///
    /// Email work owing a reply is SKIPPED by this sweep, exactly as the
    /// deployment sweep skips it. The previous attempt at a rule in this area
    /// was backed out within the hour for making such a state unreachable.
    #[test]
    fn work_owing_a_reply_is_skipped_rather_than_settled() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_task(&store, "Answer the question that came in");
        store
            .record_task_commits(
                task,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                1_000,
            )
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO email_message_links
                     (id, task_id, integration_id, message_id, conversation_id,
                      sender_name, sender_address, received_at, web_url)
                 VALUES ('link-1', ?1, 'integration-1', 'message-1', 'conversation-1',
                         'Someone', 'someone@example.test', 1000, 'https://example.test/1')",
                [task.to_string()],
            )
            .unwrap();

        assert!(
            store
                .settle_reviewed_work_without_deployment(2_000)
                .unwrap()
                .is_empty(),
            "work owing a reply must stay in review"
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
    }
}

#[cfg(test)]
mod commit_report_tests {
    use super::*;
    use crate::TaskStore;

    fn commit(sha: &str, verdict: CommitVerdict, paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict,
            subject: "feat: something".to_owned(),
            changed_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }
    }

    /// The distinction the whole record exists to preserve.
    ///
    /// "The worker says nothing was built" and "nobody has said anything" are
    /// different answers. If they collapse, unreported work reads as an
    /// investigation that produced nothing -- and the next step in this design
    /// closes that automatically, on a question never asked.
    #[test]
    fn reporting_nothing_is_an_answer_and_never_reporting_is_not() {
        let store = TaskStore::in_memory().unwrap();
        let unreported = store
            .create_task("Never asked", "/workspace/petal")
            .unwrap();
        let reported = store
            .create_task("Asked and answered", "/workspace/petal")
            .unwrap();

        assert!(
            store.task_commit_report(unreported.id).unwrap().is_none(),
            "a task nobody reported must have no report at all"
        );

        let report = store
            .record_task_commits(
                reported.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[],
                1_000,
            )
            .unwrap();
        assert!(report.commits.is_empty());
        assert!(
            store.task_commit_report(reported.id).unwrap().is_some(),
            "reporting nothing must leave a record that the question was answered"
        );
    }

    #[test]
    fn the_verdict_and_the_paths_survive_the_round_trip() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Built something", "/workspace/petal")
            .unwrap();
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[
                    commit(
                        "aaa1111",
                        CommitVerdict::Present,
                        &["docs/one.md", "docs/two.md"],
                    ),
                    commit("bbb2222", CommitVerdict::Missing, &[]),
                ],
                1_000,
            )
            .unwrap();

        let report = store.task_commit_report(task.id).unwrap().unwrap();
        assert_eq!(report.repository_state, CommitRepositoryState::Read);
        assert_eq!(report.commits.len(), 2);
        assert_eq!(report.commits[0].verdict, CommitVerdict::Present);
        assert_eq!(
            report.commits[0].changed_paths,
            vec!["docs/one.md".to_owned(), "docs/two.md".to_owned()]
        );
        assert_eq!(report.commits[1].verdict, CommitVerdict::Missing);
        assert!(report.commits[1].changed_paths.is_empty());
    }

    /// A later report ADDS; it does not erase what was reported before.
    ///
    /// A worker may report as it goes, and a second call naming two commits
    /// must not discard the one it named an hour ago.
    #[test]
    fn a_second_report_appends_rather_than_replacing() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Built in stages", "/workspace/petal")
            .unwrap();
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit("aaa1111", CommitVerdict::Present, &["src/a.rs"])],
                1_000,
            )
            .unwrap();
        let report = store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit("bbb2222", CommitVerdict::Present, &["src/b.rs"])],
                2_000,
            )
            .unwrap();

        let shas: Vec<_> = report.commits.iter().map(|c| c.sha.as_str()).collect();
        assert_eq!(shas, vec!["aaa1111", "bbb2222"]);
    }

    /// THE SNAPSHOT PROPERTY, which is the reason this is stored rather than
    /// computed. Nothing recomputes a verdict, so a squash or rebase weeks
    /// later cannot turn correct work red. The only thing that rewrites a
    /// verdict is the worker reporting that SHA again.
    #[test]
    fn a_stored_verdict_is_never_recomputed_only_re_reported() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("Squashed later", "/workspace/petal")
            .unwrap();
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit("aaa1111", CommitVerdict::Present, &["src/a.rs"])],
                1_000,
            )
            .unwrap();

        // Read it back many times: no read path may consult git or change it.
        for _ in 0..3 {
            let report = store.task_commit_report(task.id).unwrap().unwrap();
            assert_eq!(report.commits[0].verdict, CommitVerdict::Present);
        }

        // Re-reporting the same SHA is the ONE way the verdict moves, and it is
        // the case that matters: reported before it was pushed, reported again
        // once a ref reached it.
        let report = store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[commit("aaa1111", CommitVerdict::Unreachable, &["src/a.rs"])],
                3_000,
            )
            .unwrap();
        assert_eq!(report.commits.len(), 1, "re-reporting must not duplicate");
        assert_eq!(report.commits[0].verdict, CommitVerdict::Unreachable);
    }

    #[test]
    fn a_workspace_without_a_repository_still_records_a_report() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task("No checkout here", "/workspace/plain")
            .unwrap();
        let report = store
            .record_task_commits(
                task.id,
                "/workspace/plain",
                CommitRepositoryState::NotARepository,
                &[commit("aaa1111", CommitVerdict::Unchecked, &[])],
                1_000,
            )
            .unwrap();
        assert_eq!(
            report.repository_state,
            CommitRepositoryState::NotARepository
        );
        assert_eq!(report.commits[0].verdict, CommitVerdict::Unchecked);
    }
}

impl TaskStore {
    /// Closes reviewed work that never needed deployment evidence.
    ///
    /// The companion to `complete_reviewed_work_with_deployment`, for the other
    /// well-formed case: work whose commits show there was nothing to deploy.
    /// Both exist for the same reason — the operator's complaint was EFFORT,
    /// and a case a rule can settle on facts already in tables should not cost
    /// a person a decision.
    ///
    /// IT RECORDS WHY IT WAS ENTITLED TO CLOSE. Closing silently would leave
    /// these tasks carrying no evidence at all, which is precisely the shape
    /// the board asks somebody to chase — the clicking would come back one
    /// layer up, with the coordinator generating it. So the exemption is
    /// claimed and approved as `coordinator`, naming the derived facts, and
    /// anyone can later ask who approved this and get a true answer.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn settle_reviewed_work_without_deployment_page(
        &self,
        now: i64,
        after: Option<TaskId>,
    ) -> Result<ReviewedSettlementPage, TaskStoreError> {
        let candidates = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT task.id FROM tasks task
                 WHERE task.state = ?1
                   AND (?2 IS NULL OR task.id > ?2)
                   AND task.removed_at IS NULL
                   -- Work carrying a deployment is the OTHER sweep's business.
                   AND NOT EXISTS (
                       SELECT 1 FROM task_deployments d WHERE d.task_id = task.id
                   )
                   -- Already settled one way or another; nothing to decide.
                   AND NOT EXISTS (
                       SELECT 1 FROM task_completion_exemptions e
                       WHERE e.task_id = task.id AND e.approved_at IS NOT NULL AND e.withdrawn_at IS NULL
                   )
                   -- SKIPPED, NOT REFUSED, exactly as the deployment sweep
                   -- treats it: work owing somebody a reply is not well-formed,
                   -- and stalling every other completion behind one unanswered
                   -- thread would be worse than leaving this one in review.
                   AND NOT EXISTS (
                       SELECT 1 FROM email_message_links link
                       WHERE link.task_id = task.id
                         AND NOT EXISTS (
                             SELECT 1 FROM email_reply_deliveries reply
                             WHERE reply.task_id = task.id
                         )
                   )
                 ORDER BY task.id LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![
                    TaskState::Review.to_string(),
                    after.map(|id| id.to_string()),
                    i64::try_from(MAX_REVIEW_SETTLEMENT_CANDIDATES).map_err(|_| {
                        TaskStoreError::IntegrityFailure(
                            "invalid review candidate bound".to_owned(),
                        )
                    })?
                ],
                |row| {
                    TaskId::from_str(&row.get::<_, String>(0)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let checked = candidates.len();
        let next_cursor = if checked == MAX_REVIEW_SETTLEMENT_CANDIDATES {
            candidates.last().copied()
        } else {
            None
        };
        let mut closed = Vec::new();
        for task_id in candidates {
            let settlement = commit_settlement(self.task_commit_report(task_id)?.as_ref());
            // ONLY THESE TWO. `Unknown` is left alone -- it is the state of a
            // task nobody reported, and closing on it would be closing on a
            // question never asked. `BuiltCode` is left alone too: work that
            // built something and recorded no deployment is exactly what a
            // person should look at.
            let reason = match settlement {
                CommitSettlement::NothingBuilt => {
                    "Settled automatically: the worker reported that this task produced no commits, so there is nothing to deploy."
                }
                CommitSettlement::DocumentationOnly => {
                    "Settled automatically: every commit recorded for this task touches documentation only, so there is nothing to deploy."
                }
                CommitSettlement::BuiltCode
                | CommitSettlement::NotReported
                | CommitSettlement::Unestablished => continue,
            };
            self.claim_completion_exemption(task_id, reason, None, now)?;
            // THE BASIS WRITES ITSELF HERE, and this is the machine half of
            // the rule rather than an exception to it. What was checked is a
            // fact the pass just computed from the task's own commits, so the
            // citation is not a formality — it is the derivation.
            let basis = match settlement {
                CommitSettlement::NothingBuilt => {
                    "Derived: the task recorded no commits, so there is nothing that could have shipped."
                }
                CommitSettlement::DocumentationOnly => {
                    "Derived: every recorded commit touches documentation only."
                }
                CommitSettlement::BuiltCode
                | CommitSettlement::NotReported
                | CommitSettlement::Unestablished => continue,
            };
            self.approve_completion_exemption(task_id, "coordinator", basis, now)?;
            match self.transition_task_with_note_as(
                task_id,
                TaskState::Completed,
                reason,
                &TaskActivityActor::system(),
            ) {
                Ok(_) => closed.push(task_id),
                Err(TaskStoreError::NotFound | TaskStoreError::InvalidTransition { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(ReviewedSettlementPage {
            closed,
            checked,
            next_cursor,
        })
    }

    #[cfg(test)]
    fn settle_reviewed_work_without_deployment(
        &self,
        now: i64,
    ) -> Result<Vec<TaskId>, TaskStoreError> {
        Ok(self
            .settle_reviewed_work_without_deployment_page(now, None)?
            .closed)
    }
}

impl TaskStore {
    /// Records what a worker says its task produced, with the verdicts already
    /// reached, and returns the report as stored.
    ///
    /// APPEND, NOT REPLACE. A worker may report as it goes, and a later call
    /// adding two commits must not erase the three it reported an hour ago.
    /// Re-reporting the same SHA overwrites that row's verdict, which is what
    /// you want when a commit was reported before it was pushed and reachable.
    ///
    /// AN EMPTY LIST IS AN ANSWER. It writes the report row and no commit rows,
    /// which is a worker saying "nothing was built". A task with no report row
    /// at all has been asked nothing. Anything reading this record has to keep
    /// those apart, so this never invents a row for a task nobody reported.
    ///
    /// # Errors
    /// Returns an error when the task does not exist or persistence is
    /// unavailable.
    pub fn record_task_commits(
        &self,
        task_id: TaskId,
        workspace: &str,
        repository_state: CommitRepositoryState,
        commits: &[TaskCommit],
        now: i64,
    ) -> Result<TaskCommitReport, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        transaction.execute(
            "INSERT INTO task_commit_reports (task_id, workspace, repository_state, reported_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(task_id) DO UPDATE SET
                 workspace = excluded.workspace,
                 repository_state = excluded.repository_state,
                 reported_at = excluded.reported_at",
            params![
                task_id.to_string(),
                workspace,
                repository_state.to_string(),
                now
            ],
        )?;
        for commit in commits {
            transaction.execute(
                "INSERT INTO task_commits (task_id, sha, verdict, subject, changed_paths, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(task_id, sha) DO UPDATE SET
                     verdict = excluded.verdict,
                     subject = excluded.subject,
                     changed_paths = excluded.changed_paths,
                     recorded_at = excluded.recorded_at",
                params![
                    task_id.to_string(),
                    commit.sha,
                    commit.verdict.to_string(),
                    commit.subject,
                    commit.changed_paths.join("\n"),
                    now
                ],
            )?;
        }
        transaction.commit()?;
        // RELEASED BEFORE READING BACK. `connection()` hands out a MutexGuard
        // over a single connection and the mutex is not reentrant, so calling
        // the reader while this guard is alive deadlocks the caller against
        // itself -- a futex wait with no error, no timeout and no output, which
        // reads from outside exactly like a slow test suite.
        drop(connection);
        self.task_commit_report(task_id)?
            .ok_or(TaskStoreError::NotFound)
    }

    /// What this task's worker reported, or `None` if nobody has reported.
    ///
    /// `None` and a report holding no commits are DIFFERENT ANSWERS and the
    /// caller must treat them so — see `record_task_commits`.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn task_commit_report(
        &self,
        task_id: TaskId,
    ) -> Result<Option<TaskCommitReport>, TaskStoreError> {
        let connection = self.connection()?;
        let header = connection
            .query_row(
                "SELECT workspace, repository_state, reported_at
                 FROM task_commit_reports WHERE task_id = ?1",
                [task_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((workspace, repository_state, reported_at)) = header else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT sha, verdict, subject, changed_paths
             FROM task_commits WHERE task_id = ?1 ORDER BY recorded_at, sha",
        )?;
        let commits = statement
            .query_map([task_id.to_string()], |row| {
                let paths: String = row.get(3)?;
                Ok(TaskCommit {
                    sha: row.get(0)?,
                    verdict: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(CommitVerdict::Unchecked),
                    subject: row.get(2)?,
                    changed_paths: paths
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(str::to_owned)
                        .collect(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(TaskCommitReport {
            task_id,
            workspace,
            repository_state: repository_state
                .parse()
                .unwrap_or(CommitRepositoryState::NotARepository),
            reported_at,
            commits,
        }))
    }
}

/// A SEVENTEENTH READER WOULD SILENTLY IGNORE WITHDRAWALS, so this counts them.
///
/// "An approved exemption is evidence" is written in SIXTEEN separate SQL
/// queries across four files — detectors, email reply gates, task listings, and
/// the guard that refuses a second claim. Withdrawal is only real if every one
/// of them honours it; miss one and the feature works in the places somebody
/// tested and not in the place that matters, which is the failure shape this
/// repository keeps producing.
///
/// Finding that out cost a real mistake: the first cut of this change updated
/// ONE detector, because that is the one the task asked about.
///
/// THREE SITES ARE DELIBERATELY EXEMPT, and all three are the SAME trigger body
/// as it exists before schema 123. A trigger created at 108 cannot reference a
/// column added at 123 — every fresh database runs 108 first and the trigger
/// then aborts the next write with "no such column", which is exactly what this
/// change did on its first run. Two are the historical migrations; the third is
/// the schema step's `undo_sql`, which puts that same older trigger back when a
/// test rewinds the database, because a rewound database must look like one that
/// never ran 123.
///
/// `migrate_claim_withdrawal` recreates the live trigger WITH the clause once the
/// column exists, so the end state is guarded while the history stays replayable.
#[cfg(test)]
mod every_reader_honours_a_withdrawal {
    /// Sources scanned, in the order the count below reads them.
    const SOURCES: &[(&str, &str)] = &[
        ("task_outcomes.rs", include_str!("task_outcomes.rs")),
        ("coordinator.rs", include_str!("coordinator.rs")),
        ("email.rs", include_str!("email.rs")),
        ("lib.rs", include_str!("lib.rs")),
    ];

    /// The three pre-123 trigger bodies described above.
    const EXEMPT: usize = 3;

    #[test]
    fn every_query_that_reads_an_approval_also_reads_the_withdrawal() {
        let mut unguarded: Vec<String> = Vec::new();
        let mut total = 0usize;
        for (name, source) in SOURCES {
            // SPLIT SO THE SCANNER DOES NOT FIND ITSELF. This module is one of
            // the files it reads, and written as one literal the needle matched
            // its own source and reported this test as an unguarded query.
            for (offset, _) in source.match_indices(concat!("approved_at", " IS NOT NULL")) {
                total += 1;
                let tail = &source[offset..source.len().min(offset + 160)];
                if !tail.contains("withdrawn_at IS NULL") {
                    let line = source[..offset].lines().count();
                    unguarded.push(format!("{name}:{line}"));
                }
            }
        }
        assert!(
            total >= 16,
            "the readers went from 16 to {total}; if a query was deleted, lower the floor deliberately"
        );
        assert_eq!(
            unguarded.len(),
            EXEMPT,
            "a query reads an approval without reading whether it was withdrawn: {unguarded:?}. \
             Add `AND <alias>withdrawn_at IS NULL` beside it, or — if it is a trigger in a \
             migration older than 123 — recreate the trigger from migrate_claim_withdrawal instead."
        );
    }
}
