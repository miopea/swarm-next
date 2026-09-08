use crate::{AgentPrincipal, ApplicationError, TaskService, require_queen};
use swarm_domain::{NextMoveOwner, Task, TaskState};

/// Shared task membership is a reconciliation candidate, never semantic equality.
pub struct PendingDecisionOverlap {
    pub task_id: swarm_domain::TaskId,
    pub decision_ids: Vec<swarm_domain::DecisionRequestId>,
    pub pending_count: usize,
}

pub struct PendingDecisionOverlapSnapshot {
    pub groups: Vec<PendingDecisionOverlap>,
    pub truncated: bool,
}

/// Counts cover the full open board, not the bounded recovery-candidate list.
/// Owner counts include ordinary active work; they are not the waiting-only
/// navigation badge. Ownership comes from the same task projection as Queues.
pub struct QueenQueueSnapshot {
    pub open_tasks: usize,
    pub by_state: Vec<(TaskState, usize)>,
    pub by_owner: Vec<(NextMoveOwner, usize)>,
    pub queen_tasks: Vec<Task>,
    pub queen_tasks_truncated: bool,
    /// Uncovered attention only; covered waits remain in ownership counts.
    pub review_focus: Vec<swarm_domain::TaskId>,
}

impl TaskService {
    /// Reserve bounded attention for a claimed Queen run; never move task state.
    ///
    /// # Errors
    /// Denies non-Queen callers, stale claims and unavailable queue evidence.
    pub fn reserve_queen_review_focus(
        &self,
        principal: AgentPrincipal,
        run_id: &str,
        session_id: swarm_domain::WorkerSessionId,
    ) -> Result<Vec<swarm_domain::TaskId>, ApplicationError> {
        require_queen(principal)?;
        let queue = self.queen_queue_snapshot(principal)?;
        Ok(self
            .store
            .reserve_queen_review_focus(run_id, session_id, &queue.review_focus)?)
    }

    /// Identify overlapping operator requests without altering any request or grant.
    ///
    /// # Errors
    /// Denies non-Queen callers and propagates persistence failures.
    pub fn queen_pending_decision_overlaps(
        &self,
        principal: AgentPrincipal,
    ) -> Result<PendingDecisionOverlapSnapshot, ApplicationError> {
        require_queen(principal)?;
        // Persistence returns pending requests first; its read cap equals its
        // enforced pending-request cap. Historical records cannot crowd these out.
        let decisions = self.store.list_decision_requests()?;
        let mut membership = std::collections::HashMap::<_, Vec<_>>::new();
        for decision in decisions {
            if decision.state != swarm_domain::DecisionRequestState::Pending {
                continue;
            }
            let tasks = decision
                .task_id
                .into_iter()
                .chain(decision.linked_tasks.iter().map(|link| link.task_id))
                .collect::<std::collections::HashSet<_>>();
            for task in tasks {
                membership.entry(task).or_default().push(decision.id);
            }
        }
        let mut groups = membership
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(task_id, mut ids)| {
                ids.sort_by_key(ToString::to_string);
                PendingDecisionOverlap {
                    task_id,
                    pending_count: ids.len(),
                    decision_ids: ids,
                }
            })
            .collect::<Vec<_>>();
        groups.sort_by_key(|group| group.task_id.to_string());
        let mut truncated = groups.len() > 32;
        groups.truncate(32);
        for group in &mut groups {
            truncated |= group.decision_ids.len() > 16;
            group.decision_ids.truncate(16);
        }
        Ok(PendingDecisionOverlapSnapshot { groups, truncated })
    }

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
        let judgments = self.store.queen_review_queue_snapshot()?;
        let tasks = self.store.list_board_tasks()?;
        let covered = covered_review_tasks(&tasks, &judgments);
        let investigated = unchanged_investigations(&tasks, &judgments);
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
            review_focus: Vec::new(),
        };
        for task in tasks {
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
                order_review_tasks(
                    &mut snapshot.queen_tasks,
                    &checked_at,
                    &covered,
                    &investigated,
                );
                if snapshot.queen_tasks.len() > 64 {
                    snapshot.queen_tasks.pop();
                    snapshot.queen_tasks_truncated = true;
                }
            }
        }
        snapshot.review_focus = snapshot
            .queen_tasks
            .iter()
            .filter(|task| !covered.contains(&task.id))
            .map(|task| task.id)
            .collect();
        Ok(snapshot)
    }
}

fn covered_review_tasks(
    tasks: &[Task],
    judgments: &swarm_domain::QueenReviewQueueSnapshot,
) -> std::collections::HashSet<swarm_domain::TaskId> {
    use swarm_domain::{QueenReviewAssessmentStatus as Status, QueenReviewDispositionKind as Kind};
    judgments
        .items
        .iter()
        .filter_map(|item| {
            let previous = item.previous_assessment.as_ref()?;
            // Separate reads must agree on the complete projection, even for updates
            // in the same second. Missing/truncated entries remain attention work.
            let current = tasks.iter().find(|task| task.id == item.task.id)?;
            if *current != item.task
                || current.next_move_owner != NextMoveOwner::Queen
                || previous.assessment.kind == Kind::InsufficientEvidence
            {
                return None;
            }
            (previous.status == Status::CoveredForCurrentRun
                || (previous.status == Status::NoActiveReview
                    && previous.assessment.kind == Kind::OperatorDeferral))
                .then_some(current.id)
        })
        .collect()
}

fn unchanged_investigations(
    tasks: &[Task],
    judgments: &swarm_domain::QueenReviewQueueSnapshot,
) -> std::collections::HashSet<swarm_domain::TaskId> {
    judgments
        .items
        .iter()
        .filter_map(|item| {
            let previous = item.previous_assessment.as_ref()?;
            let current = tasks.iter().find(|task| task.id == item.task.id)?;
            (*current == item.task
                && previous.status
                    == swarm_domain::QueenReviewAssessmentStatus::InsufficientEvidence
                && previous.assessment.kind
                    == swarm_domain::QueenReviewDispositionKind::InsufficientEvidence)
                .then_some(current.id)
        })
        .collect()
}

fn order_review_tasks(
    tasks: &mut [Task],
    checked_at: &std::collections::HashMap<swarm_domain::TaskId, i64>,
    covered: &std::collections::HashSet<swarm_domain::TaskId>,
    investigated: &std::collections::HashSet<swarm_domain::TaskId>,
) {
    tasks.sort_by_key(|task| {
        (
            covered.contains(&task.id),
            investigated.contains(&task.id),
            checked_at.get(&task.id).copied(),
            task.created_at,
            task.id.to_string(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judgments(
        task: &Task,
        status: swarm_domain::QueenReviewAssessmentStatus,
        kind: swarm_domain::QueenReviewDispositionKind,
    ) -> swarm_domain::QueenReviewQueueSnapshot {
        swarm_domain::QueenReviewQueueSnapshot {
            checked_at: 10,
            truncated: false,
            items: vec![swarm_domain::QueenTaskReviewEvidence {
                task: task.clone(),
                obligation: swarm_domain::QueenReviewObligation {
                    task_id: task.id,
                    evidence_revision: "a".repeat(64),
                },
                current_run_id: None,
                previous_assessment: Some(swarm_domain::QueenReviewAssessmentEvidence {
                    status,
                    recorded_at: 1,
                    assessment: swarm_domain::QueenReviewDispositionInput {
                        task_id: task.id,
                        run_id: swarm_domain::TaskId::new().to_string(),
                        expected_revision: "a".repeat(64),
                        kind,
                        condition: "Existing explicit operator deferral".into(),
                        evidence: "Verified source".into(),
                        source: "Original task decision".into(),
                        operator_activity_sequence: None,
                        operator_decision_id: None,
                    },
                }),
            }],
        }
    }

    #[test]
    fn reusable_deferrals_do_not_repeat_as_focus_ahead_of_unresolved_work() {
        use swarm_domain::{
            QueenReviewAssessmentStatus as Status, QueenReviewDispositionKind as Kind,
        };
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let held = store
            .create_task("Already checked deferral", "/workspace/demo")
            .unwrap();
        let unresolved = store
            .create_task("Needs an actual next action", "/workspace/demo")
            .unwrap();
        let checks = [(held.id, 1), (unresolved.id, 2)].into_iter().collect();
        for status in [Status::NoActiveReview, Status::CoveredForCurrentRun] {
            let mut tasks = vec![held.clone(), unresolved.clone()];
            let covered =
                covered_review_tasks(&tasks, &judgments(&held, status, Kind::OperatorDeferral));
            // Repeating the selection cannot put the unchanged covered item back first.
            for _ in 0..3 {
                order_review_tasks(
                    &mut tasks,
                    &checks,
                    &covered,
                    &std::collections::HashSet::new(),
                );
                assert_eq!(tasks[0].id, unresolved.id);
                assert_eq!(tasks.len(), 2);
                assert_eq!(
                    tasks
                        .iter()
                        .filter(|task| !covered.contains(&task.id))
                        .map(|task| task.id)
                        .collect::<Vec<_>>(),
                    vec![unresolved.id]
                );
            }
            assert_eq!(store.get_task(held.id).unwrap(), held);
        }
    }

    #[test]
    fn focus_retains_stale_missing_and_uncovered_evidence() {
        use swarm_domain::{
            QueenReviewAssessmentStatus as Status, QueenReviewDispositionKind as Kind,
        };
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let task = store.create_task("Review me", "/workspace/demo").unwrap();
        for (status, kind) in [
            (Status::NoActiveReview, Kind::ExternalCondition),
            (Status::FreshExternalCheckRequired, Kind::ExternalCondition),
            (Status::EvidenceChanged, Kind::OperatorDeferral),
            (Status::InsufficientEvidence, Kind::InsufficientEvidence),
            (Status::CoveredForCurrentRun, Kind::InsufficientEvidence),
        ] {
            assert!(
                covered_review_tasks(std::slice::from_ref(&task), &judgments(&task, status, kind))
                    .is_empty()
            );
        }
        let mut snapshot = judgments(&task, Status::CoveredForCurrentRun, Kind::ExternalCondition);
        assert!(covered_review_tasks(std::slice::from_ref(&task), &snapshot).contains(&task.id));
        let mut changed = task.clone();
        changed.description = "Changed within the same timestamp".into();
        assert!(covered_review_tasks(&[changed], &snapshot).is_empty());
        snapshot.items.clear();
        snapshot.truncated = true;
        assert!(covered_review_tasks(&[task], &snapshot).is_empty());
    }

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
        order_review_tasks(
            &mut tasks,
            &checks,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        );
        assert_eq!(
            tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![unchecked.id, older.id, recent.id]
        );
        assert!(tasks.iter().all(|task| task.state == TaskState::Draft));
        assert_eq!(store.get_task(recent.id).unwrap().position, recent.position);
    }

    #[test]
    fn unchanged_missing_evidence_does_not_monopolize_focus_or_become_coverage() {
        use swarm_domain::{
            QueenReviewAssessmentStatus as Status, QueenReviewDispositionKind as Kind,
        };
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let missing = store
            .create_task("Already investigated", "/workspace/demo")
            .unwrap();
        let other = store
            .create_task("External condition needs checking", "/workspace/demo")
            .unwrap();
        let checks = [(missing.id, 1), (other.id, 2)].into_iter().collect();
        let evidence = judgments(
            &missing,
            Status::InsufficientEvidence,
            Kind::InsufficientEvidence,
        );
        let mut tasks = vec![missing.clone(), other.clone()];
        let investigated = unchanged_investigations(&tasks, &evidence);
        let covered = covered_review_tasks(&tasks, &evidence);
        assert!(covered.is_empty());
        order_review_tasks(&mut tasks, &checks, &covered, &investigated);
        assert_eq!(
            tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![other.id, missing.id]
        );
        // Missing or changed evidence immediately removes the lower priority.
        let changed = judgments(
            &missing,
            Status::EvidenceChanged,
            Kind::InsufficientEvidence,
        );
        assert!(unchanged_investigations(&tasks, &changed).is_empty());
        let mut edited = missing.clone();
        edited.description = "New facts in the same second".into();
        assert!(unchanged_investigations(&[edited], &evidence).is_empty());
        assert_eq!(store.get_task(missing.id).unwrap(), missing);
    }

    fn persisted_deferral_fixture() -> (TaskService, AgentPrincipal, Task, String) {
        use swarm_domain::{
            QueenReviewDispositionInput, QueenReviewDispositionKind, TaskActivityActor,
            WorkerSessionId,
        };
        let store = swarm_persistence::TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let held = store
            .create_task("Wait for my interview", "/workspace/demo")
            .unwrap();
        store.transition_task(held.id, TaskState::Ready).unwrap();
        store
            .transition_task_with_note(
                held.id,
                TaskState::Blocked,
                "Operator requested an interview",
            )
            .unwrap();
        store
            .append_task_correction(
                held.id,
                "Wait until I return for the interview",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        let sequence = store.list_task_activity(held.id, 1).unwrap().events[0].sequence;
        store.request_queen_automation_run(100).unwrap();
        let run = store.claim_queen_automation(100).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, 100)
            .unwrap();
        store
            .record_queen_review_disposition(
                &QueenReviewDispositionInput {
                    task_id: held.id,
                    run_id: run.run_id.clone(),
                    expected_revision: store
                        .queen_task_review_evidence(held.id)
                        .unwrap()
                        .evidence_revision,
                    kind: QueenReviewDispositionKind::OperatorDeferral,
                    condition: "Wait for the operator's interview".into(),
                    evidence: "Read the original task correction".into(),
                    source: "Authenticated task correction".into(),
                    operator_activity_sequence: Some(sequence),
                    operator_decision_id: None,
                },
                &TaskActivityActor::operator(),
                101,
            )
            .unwrap();
        let service = TaskService::new(store);
        let principal = AgentPrincipal::from(&queen);
        (service, principal, held, run.run_id)
    }

    #[test]
    fn persisted_deferral_leaves_focus_but_not_ownership_and_returns_when_evidence_changes() {
        use swarm_domain::{QueenAutomationOutcome, TaskActivityActor};
        let (service, principal, held, run_id) = persisted_deferral_fixture();
        for active in [true, false] {
            if !active {
                service
                    .store
                    .finish_queen_automation_run(&run_id, QueenAutomationOutcome::NoAction, 102)
                    .unwrap();
            }
            let snapshot = service.queen_queue_snapshot(principal).unwrap();
            assert!(snapshot.review_focus.is_empty());
            assert_eq!(snapshot.queen_tasks.len(), 1);
            assert_eq!(snapshot.by_owner[0], (NextMoveOwner::Queen, 1));
        }
        let fresh = service
            .store
            .create_task("New actionable review", "/workspace/demo")
            .unwrap();
        assert_eq!(
            service
                .queen_queue_snapshot(principal)
                .unwrap()
                .review_focus,
            vec![fresh.id]
        );
        service.store.request_queen_automation_run(200).unwrap();
        let next_run = service.store.claim_queen_automation(200).unwrap().unwrap();
        assert_eq!(
            service
                .reserve_queen_review_focus(principal, &next_run.run_id, next_run.session_id)
                .unwrap(),
            vec![fresh.id],
            "an authenticated unchanged deferral stays out of a new reservation",
        );
        let worker = service
            .store
            .create_worker(
                "Fixture",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/fixture",
                false,
                1,
            )
            .unwrap();
        assert!(
            service
                .reserve_queen_review_focus(
                    AgentPrincipal::from(&worker),
                    &next_run.run_id,
                    next_run.session_id,
                )
                .is_err()
        );
        service
            .store
            .append_task_correction(
                held.id,
                "The interview scope has changed",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        let snapshot = service.queen_queue_snapshot(principal).unwrap();
        assert!(snapshot.review_focus.contains(&held.id));
        assert!(snapshot.review_focus.contains(&fresh.id));
        assert_eq!(
            service
                .reserve_queen_review_focus(principal, &next_run.run_id, next_run.session_id)
                .unwrap(),
            vec![fresh.id],
            "a delivery retry keeps its identity even when the full review changes",
        );
        assert_eq!(
            service.store.get_task(held.id).unwrap().state,
            TaskState::Blocked
        );
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
