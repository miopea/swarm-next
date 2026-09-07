use super::*;
use swarm_domain::{NextMoveOwner, TaskActivityActorKind};

fn decision(store: &TaskStore) -> (TaskId, DecisionRequestId) {
    let queen = store.ensure_queen("/fixture/queen").unwrap();
    let primary = store
        .create_task("Shared fictional question", "/fixture")
        .unwrap();
    let id = DecisionRequestId::new();
    store.connection().unwrap().execute(
        "INSERT INTO decision_requests(id,hive_id,requesting_worker_id,task_id,kind,urgency,title,
         reason,risk,evidence,suggested_action,allowed_actions)
         VALUES (?1,?2,?3,?4,'input','normal','Fictional session gate','Need the same session','','','','[\"Provided a session\"]')",
        params![id.to_string(), primary.hive_id.to_string(), queen.id.to_string(), primary.id.to_string()],
    ).unwrap();
    (primary.id, id)
}

fn blocked(store: &TaskStore) -> TaskId {
    let task = store.create_task("Fictional consumer", "/fixture").unwrap();
    store.transition_task(task.id, TaskState::Ready).unwrap();
    store.transition_task(task.id, TaskState::Blocked).unwrap();
    task.id
}

fn link(store: &TaskStore, task: TaskId, decision: DecisionRequestId) {
    store
        .add_task_decision_link(
            task,
            decision,
            "Same session gate",
            &revision(store, task),
            &TaskActivityActor::operator(),
            100,
        )
        .unwrap();
}

fn revision(store: &TaskStore, task: TaskId) -> String {
    store
        .queen_task_review_evidence(task)
        .unwrap()
        .evidence_revision
}

fn audit_count(store: &TaskStore, task: TaskId) -> i64 {
    store.connection().unwrap().query_row(
        "SELECT count(*) FROM task_activity WHERE task_id=?1 AND note LIKE 'Decision blocker %'",
        [task.to_string()], |row| row.get(0),
    ).unwrap()
}

#[test]
fn shared_decision_updates_all_linked_evidence_without_task_transitions() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let tasks = [blocked(&store), blocked(&store)];
    for task in tasks {
        link(&store, task, gate);
    }
    let before: Vec<_> = tasks
        .iter()
        .map(|id| store.queen_task_review_evidence(*id).unwrap())
        .collect();
    for task in tasks {
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            NextMoveOwner::Operator
        );
        assert!(
            !store
                .blocked_tasks_for_reassessment(200)
                .unwrap()
                .0
                .iter()
                .any(|row| row.id == task)
        );
    }
    // Exercise the real decision resolution service, including its delivery path.
    store
        .resolve_decision_request(gate, "Provided a session", "Fictional answer", "inbox")
        .unwrap();
    for (index, task) in tasks.iter().enumerate() {
        let current = store.get_task(*task).unwrap();
        assert_eq!(current.state, TaskState::Blocked);
        assert_eq!(current.next_move_owner, NextMoveOwner::Queen);
        assert_ne!(
            before[index],
            store.queen_task_review_evidence(*task).unwrap()
        );
    }
    let deliveries: i64 = store
        .connection()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM decision_deliveries WHERE decision_id=?1",
            [gate.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        deliveries, 1,
        "shared blockers must not fan out operator delivery"
    );
}

#[test]
fn shared_decision_second_gate_remains_when_one_is_withdrawn() {
    let store = TaskStore::in_memory().unwrap();
    let (_, first) = decision(&store);
    let (_, second) = decision(&store);
    let task = blocked(&store);
    link(&store, task, first);
    link(&store, task, second);
    let before = store.queen_task_review_evidence(task).unwrap();
    store
        .connection()
        .unwrap()
        .execute(
            "UPDATE decision_requests SET state='withdrawn' WHERE id=?1",
            [first.to_string()],
        )
        .unwrap();
    assert_eq!(
        store.get_task(task).unwrap().next_move_owner,
        NextMoveOwner::Operator
    );
    assert_ne!(before, store.queen_task_review_evidence(task).unwrap());
    assert!(
        store
            .remove_task_decision_link(
                task,
                second,
                "No longer needed",
                &revision(&store, task),
                &TaskActivityActor::operator(),
                101
            )
            .unwrap()
    );
    assert_eq!(
        store.get_task(task).unwrap().next_move_owner,
        NextMoveOwner::Queen
    );
    assert_eq!(
        store.get_decision_request(second).unwrap().state,
        DecisionRequestState::Pending
    );
}

#[test]
fn shared_decision_replay_after_resolution_is_exact_and_does_not_expand_scope() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let task = blocked(&store);
    link(&store, task, gate);
    store
        .resolve_decision_request(gate, "Provided a session", "Fictional answer", "inbox")
        .unwrap();
    link(&store, task, gate);
    assert_eq!(audit_count(&store, task), 1);
    assert!(matches!(
        store.add_task_decision_link(
            task,
            gate,
            "Different reason",
            &revision(&store, task),
            &TaskActivityActor::operator(),
            200
        ),
        Err(TaskStoreError::TaskDecisionLink(
            TaskDecisionLinkError::Conflict
        ))
    ));
    let fresh = blocked(&store);
    assert!(matches!(
        store.add_task_decision_link(
            fresh,
            gate,
            "New scope",
            &revision(&store, fresh),
            &TaskActivityActor::operator(),
            200
        ),
        Err(TaskStoreError::TaskDecisionLink(
            TaskDecisionLinkError::DecisionNotPending
        ))
    ));
}

#[test]
fn shared_decision_primary_membership_cannot_be_duplicated_or_unlinked() {
    let store = TaskStore::in_memory().unwrap();
    let (primary, gate) = decision(&store);
    link(&store, primary, gate);
    assert!(store.task_decision_links(primary).unwrap().is_empty());
    assert!(
        !store
            .remove_task_decision_link(
                primary,
                gate,
                "Not an additional link",
                &revision(&store, primary),
                &TaskActivityActor::operator(),
                100
            )
            .unwrap()
    );
    assert_eq!(
        store.get_decision_request(gate).unwrap().task_id,
        Some(primary)
    );
    assert_eq!(audit_count(&store, primary), 0);
}

#[test]
fn shared_decision_ordinary_worker_cannot_link_or_remove() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let task = blocked(&store);
    let actor = TaskActivityActor {
        kind: TaskActivityActorKind::Worker,
        id: Some(swarm_domain::WorkerId::new().to_string()),
    };
    assert!(matches!(
        store.add_task_decision_link(
            task,
            gate,
            "Not authorized",
            &revision(&store, task),
            &actor,
            100
        ),
        Err(TaskStoreError::TaskDecisionLink(
            TaskDecisionLinkError::Unauthorized
        ))
    ));
    link(&store, task, gate);
    assert!(
        store
            .remove_task_decision_link(
                task,
                gate,
                "Not authorized",
                &revision(&store, task),
                &actor,
                101
            )
            .is_err()
    );
    assert_eq!(store.task_decision_links(task).unwrap().len(), 1);
}

#[test]
fn shared_decision_activity_failure_rolls_back_link() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let task = blocked(&store);
    store
        .connection()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_link_note BEFORE INSERT ON task_activity
         WHEN NEW.note LIKE 'Decision blocker added:%' BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    assert!(
        store
            .add_task_decision_link(
                task,
                gate,
                "Same session gate",
                &revision(&store, task),
                &TaskActivityActor::operator(),
                100
            )
            .is_err()
    );
    assert!(store.task_decision_links(task).unwrap().is_empty());
    store
        .connection()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_link_note")
        .unwrap();
    link(&store, task, gate);
    assert_eq!(audit_count(&store, task), 1);
}

#[test]
fn shared_decision_migration_from_deployed_146_preserves_original_membership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.sqlite3");
    let store = TaskStore::open(&path).unwrap();
    let (primary, gate) = decision(&store);
    let consumer = blocked(&store);
    store
        .connection()
        .unwrap()
        .execute_batch(
            "DROP VIEW task_decision_membership; DROP TABLE task_decision_links;
         DROP INDEX decision_requests_by_task_identity;
         DROP TABLE IF EXISTS hive_support_outbox; PRAGMA user_version=146;",
        )
        .unwrap();
    drop(store);
    let reopened = TaskStore::open(&path).unwrap();
    assert_eq!(
        reopened.get_decision_request(gate).unwrap().task_id,
        Some(primary)
    );
    link(&reopened, consumer, gate);
    drop(reopened);
    let reopened = TaskStore::open(&path).unwrap();
    link(&reopened, consumer, gate);
    assert_eq!(audit_count(&reopened, consumer), 1);
}

#[test]
fn shared_decision_delayed_mutations_cannot_undo_a_later_link_change() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let task = blocked(&store);
    let before_add = revision(&store, task);
    link(&store, task, gate);
    let before_remove = revision(&store, task);
    assert!(
        store
            .remove_task_decision_link(
                task,
                gate,
                "Remove gate",
                &before_remove,
                &TaskActivityActor::operator(),
                101
            )
            .unwrap()
    );
    assert!(
        store
            .add_task_decision_link(
                task,
                gate,
                "Same session gate",
                &before_add,
                &TaskActivityActor::operator(),
                102
            )
            .is_err()
    );
    assert!(store.task_decision_links(task).unwrap().is_empty());
    link(&store, task, gate);
    assert!(
        store
            .remove_task_decision_link(
                task,
                gate,
                "Remove gate",
                &before_remove,
                &TaskActivityActor::operator(),
                103
            )
            .is_err()
    );
    assert_eq!(store.task_decision_links(task).unwrap().len(), 1);
}

#[test]
fn shared_decision_concurrent_last_slot_is_atomic_and_visible_in_the_question() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capacity.sqlite3");
    let store = TaskStore::open(&path).unwrap();
    let (_, gate) = decision(&store);
    for _ in 0..31 {
        let task = blocked(&store);
        link(&store, task, gate);
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let task = blocked(&store);
        let evidence = revision(&store, task);
        let contender = TaskStore::open(&path).unwrap();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            contender.add_task_decision_link(
                task,
                gate,
                "Last shared slot",
                &evidence,
                &TaskActivityActor::operator(),
                200,
            )
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(TaskStoreError::TaskDecisionLink(
                    TaskDecisionLinkError::Capacity
                ))
            ))
            .count(),
        1
    );
    let question = store.get_decision_request(gate).unwrap();
    assert_eq!(question.linked_tasks.len(), 32);
    assert!(
        question
            .linked_tasks
            .iter()
            .all(|link| link.decision_id == gate)
    );
    assert_eq!(store.list_decision_requests().unwrap().len(), 1);
}

#[test]
fn shared_decision_refuses_missing_removed_and_finished_targets() {
    let store = TaskStore::in_memory().unwrap();
    let (_, gate) = decision(&store);
    let actor = TaskActivityActor::operator();
    let task = blocked(&store);
    for (target, question) in [(TaskId::new(), gate), (task, DecisionRequestId::new())] {
        assert!(matches!(
            store.add_task_decision_link(target, question, "Fixture", "unused", &actor, 100),
            Err(TaskStoreError::NotFound)
        ));
    }
    store
        .remove_task_as(task, &actor, "Fictional removed work")
        .unwrap();
    assert!(matches!(
        store.add_task_decision_link(task, gate, "Fixture", "unused", &actor, 100),
        Err(TaskStoreError::NotFound)
    ));
    for terminal in [TaskState::Completed, TaskState::Abandoned] {
        let task = blocked(&store);
        if terminal == TaskState::Completed {
            for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
                store.transition_task(task, state).unwrap();
            }
        }
        store.transition_task(task, terminal).unwrap();
        assert!(matches!(
            store.add_task_decision_link(task, gate, "Fixture", "unused", &actor, 100),
            Err(TaskStoreError::TaskDecisionLink(
                TaskDecisionLinkError::TaskFinished
            ))
        ));
        assert!(store.task_decision_links(task).unwrap().is_empty());
    }
}

#[test]
fn shared_decision_refuses_foreign_task_or_question_even_when_both_match() {
    for (foreign_task, foreign_decision) in [(true, false), (false, true), (true, true)] {
        let store = TaskStore::in_memory().unwrap();
        let (_, gate) = decision(&store);
        let task = blocked(&store);
        let foreign = swarm_domain::HiveId::new().to_string();
        let operator = swarm_domain::OperatorId::new().to_string();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO operators(id,display_name) VALUES (?1,'Fictional operator')",
                    [&operator],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives(id,name,operator_id) VALUES (?1,'Fictional other Hive',?2)",
                    params![foreign, operator],
                )
                .unwrap();
            if foreign_task {
                connection
                    .execute(
                        "UPDATE tasks SET hive_id=?1 WHERE id=?2",
                        params![foreign, task.to_string()],
                    )
                    .unwrap();
            }
            if foreign_decision {
                connection
                    .execute(
                        "UPDATE decision_requests SET hive_id=?1 WHERE id=?2",
                        params![foreign, gate.to_string()],
                    )
                    .unwrap();
            }
        }
        assert!(matches!(
            store.add_task_decision_link(
                task,
                gate,
                "Fixture",
                "unused",
                &TaskActivityActor::operator(),
                100
            ),
            Err(TaskStoreError::NotFound)
        ));
        assert!(store.task_decision_links(task).unwrap().is_empty());
    }
}
