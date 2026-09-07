use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen};
use swarm_domain::{NextMoveOwner, Task, TaskState};

/// Counts cover the full open board, not the bounded recovery-candidate list.
/// Owner counts include ordinary active work; they are not the waiting-only
/// navigation badge. Ownership comes from the same task projection as Queues.
pub struct QueenQueueSnapshot {
    pub open_tasks: usize,
    pub by_state: Vec<(TaskState, usize)>,
    pub by_owner: Vec<(NextMoveOwner, usize)>,
    pub queen_tasks: Vec<Task>,
    pub queen_tasks_truncated: bool,
}

impl TaskService {
    /// Read bounded validated judgments without changing task ownership.
    ///
    /// # Errors
    /// Propagates unavailable or corrupt evidence, never stale cached coverage.
    pub fn queen_review_queue_snapshot(
        &self,
    ) -> Result<swarm_domain::QueenReviewQueueSnapshot, ApplicationError> {
        Ok(self.store.queen_review_queue_snapshot()?)
    }

    /// Operator-authorized adapters supply existing supervisor observations.
    /// This read neither observes terminals again nor certifies recovery success.
    ///
    /// # Errors
    /// Propagates unavailable or corrupt persistence evidence.
    pub fn recovery_queue_snapshot(
        &self,
        observations: &[swarm_domain::RecoveryQueueObservation],
        now: i64,
    ) -> Result<swarm_domain::RecoveryQueueSnapshot, ApplicationError> {
        Ok(self.store.recovery_queue_snapshot(observations, now)?)
    }

    /// Read current board ownership without assigning, starting or resolving work.
    ///
    /// # Errors
    /// Denies non-Queen callers and propagates persistence failures.
    pub fn queen_queue_snapshot(
        &self,
        principal: AgentPrincipal,
    ) -> Result<QueenQueueSnapshot, ApplicationError> {
        require_queen(principal)?;
        // Historical checks order attention only. They do not certify current
        // coverage, which still requires the separately fenced evidence read.
        let checked_at = self.store.queen_review_check_times()?;
        let mut snapshot = QueenQueueSnapshot {
            open_tasks: 0,
            by_state: [
                TaskState::Draft,
                TaskState::Ready,
                TaskState::Active,
                TaskState::Blocked,
                TaskState::Review,
                TaskState::AwaitingRelease,
            ]
            .into_iter()
            .map(|state| (state, 0))
            .collect(),
            by_owner: [
                NextMoveOwner::Queen,
                NextMoveOwner::Worker,
                NextMoveOwner::Operator,
                NextMoveOwner::Blocked,
                NextMoveOwner::Release,
                NextMoveOwner::Nobody,
            ]
            .into_iter()
            .map(|owner| (owner, 0))
            .collect(),
            queen_tasks: Vec::new(),
            queen_tasks_truncated: false,
        };
        for task in self.store.list_board_tasks()? {
            if matches!(task.state, TaskState::Completed | TaskState::Abandoned) {
                continue;
            }
            snapshot.open_tasks += 1;
            for (state, count) in &mut snapshot.by_state {
                if *state == task.state {
                    *count += 1;
                }
            }
            for (owner, count) in &mut snapshot.by_owner {
                if *owner == task.next_move_owner {
                    *count += 1;
                }
            }
            if task.next_move_owner == NextMoveOwner::Queen {
                snapshot.queen_tasks.push(task);
                order_review_tasks(&mut snapshot.queen_tasks, &checked_at);
                if snapshot.queen_tasks.len() > 64 {
                    snapshot.queen_tasks.pop();
                    snapshot.queen_tasks_truncated = true;
                }
            }
        }
        Ok(snapshot)
    }
}

fn order_review_tasks(
    tasks: &mut [Task],
    checked_at: &std::collections::HashMap<swarm_domain::TaskId, i64>,
) {
    tasks.sort_by_key(|task| {
        (
            checked_at.get(&task.id).copied(),
            task.created_at,
            task.id.to_string(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_order_prefers_unchecked_then_oldest_check_without_mutating_tasks() {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let older = store.create_task("Older check", "/workspace/demo").unwrap();
        let recent = store
            .create_task("Recent check", "/workspace/demo")
            .unwrap();
        let unchecked = store
            .create_task("Never checked", "/workspace/demo")
            .unwrap();
        let checks = [(older.id, 100), (recent.id, 200)].into_iter().collect();
        let mut tasks = vec![recent.clone(), older.clone(), unchecked.clone()];
        order_review_tasks(&mut tasks, &checks);
        assert_eq!(
            tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![unchecked.id, older.id, recent.id]
        );
        assert!(tasks.iter().all(|task| task.state == TaskState::Draft));
        assert_eq!(store.get_task(recent.id).unwrap().position, recent.position);
    }

    #[test]
    fn queen_queue_snapshot_counts_beyond_detail_cap_and_reconciles_transitions() {
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let service = TaskService::new(store);
        let principal = AgentPrincipal::from(&queen);
        let mut ids = Vec::new();
        for index in 0..65 {
            ids.push(
                service
                    .store
                    .create_task(&format!("Draft {index}"), "/workspace/petal")
                    .unwrap()
                    .id,
            );
        }
        let snapshot = service.queen_queue_snapshot(principal).unwrap();
        assert_eq!(snapshot.open_tasks, 65);
        assert_eq!(snapshot.by_state[0], (TaskState::Draft, 65));
        assert_eq!(snapshot.by_owner[0], (NextMoveOwner::Queen, 65));
        assert_eq!(snapshot.queen_tasks.len(), 64);
        assert!(snapshot.queen_tasks_truncated);
        assert!(
            service
                .queen_queue_snapshot(AgentPrincipal::from(&worker))
                .is_err()
        );
        service
            .store
            .transition_task(ids[0], TaskState::Abandoned)
            .unwrap();
        let snapshot = service.queen_queue_snapshot(principal).unwrap();
        assert_eq!(snapshot.open_tasks, 64);
        assert!(!snapshot.queen_tasks_truncated);
        assert!(!snapshot.queen_tasks.iter().any(|task| task.id == ids[0]));
        assert_eq!(
            service.store.get_task(ids[1]).unwrap().state,
            TaskState::Draft
        );
    }
}
