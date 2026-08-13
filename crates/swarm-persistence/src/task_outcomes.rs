use std::str::FromStr;

use rusqlite::params;
use swarm_domain::{ControlRoomEventKind, TaskId, TaskState, WorkerId, WorkerSessionId};

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
