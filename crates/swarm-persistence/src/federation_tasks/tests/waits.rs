//! Shared work carries its wait (ADR 0119, part 3).

use super::*;
use swarm_domain::{TaskWaitKind, format_rfc3339_seconds};

/// The real clock, which a Blocked note's times are checked against.
fn wall_clock(store: &TaskStore) -> i64 {
    store
        .connection()
        .unwrap()
        .query_row("SELECT unixepoch()", [], |row| row.get(0))
        .unwrap()
}

/// One round of member sync: stage, send every queued command, then read the
/// feed. Returns what was sent.
fn sync(
    keeper: &TaskStore,
    member: &TaskStore,
    credential: &str,
    at: i64,
) -> Vec<FederationTaskCommand> {
    member
        .prepare_local_apiary_task_lifecycle_commands(at)
        .unwrap();
    let mut sent = Vec::new();
    for entry in member
        .list_federation_task_outbox()
        .unwrap()
        .into_iter()
        .filter(|entry| entry.state == FederationTaskOutboxState::Queued)
    {
        let receipt = keeper
            .apply_federation_task_command(credential, &entry.command, at)
            .unwrap();
        assert_eq!(
            receipt.outcome,
            FederationTaskCommandOutcome::Applied,
            "{:?}",
            entry.command
        );
        member
            .apply_federation_task_command_receipt(&receipt, at)
            .unwrap();
        sent.push(entry.command);
    }
    let page = keeper.federation_task_page(credential, 0, at).unwrap();
    member.apply_federation_task_page(&page, at).unwrap();
    sent
}

/// What a Hive that did none of the work sees of `task`.
fn seen_by(
    watcher: &TaskStore,
    keeper: &TaskStore,
    credential: &str,
    task: ApiaryTaskId,
    at: i64,
) -> ApiaryTask {
    let page = keeper.federation_task_page(credential, 0, at).unwrap();
    watcher.apply_federation_task_page(&page, at).unwrap();
    watcher
        .list_visible_apiary_tasks()
        .unwrap()
        .into_iter()
        .find(|held| held.id == task)
        .expect("the other Hive sees the shared task")
}

/// The Keeper's own board, which reads its rows rather than the feed.
fn on_keeper_board(keeper: &TaskStore, task: ApiaryTaskId) -> ApiaryTask {
    keeper
        .list_visible_apiary_tasks()
        .unwrap()
        .into_iter()
        .find(|held| held.id == task)
        .expect("the Keeper lists the shared task")
}

struct SharedWork {
    keeper: TaskStore,
    member: TaskStore,
    member_hive: HiveId,
    credential: String,
    watcher: TaskStore,
    watcher_credential: String,
    task: ApiaryTaskId,
    local: TaskId,
}

/// Shared work a member has claimed and runs Active in one of its workers,
/// with the Keeper in step.
fn shared_work_running(now: i64) -> SharedWork {
    let (keeper, member, acceptance) = joined_member(now);
    let (watcher, watcher_acceptance) = second_member(&keeper, now + 100);
    let credential = acceptance.node_credential.clone();
    let task = keeper
        .create_apiary_task("Shared work", "", TaskPriority::Normal, now + 110)
        .unwrap();
    let page = keeper
        .federation_task_page(&credential, 0, now + 111)
        .unwrap();
    member.apply_federation_task_page(&page, now + 111).unwrap();
    let claim = member
        .queue_federation_task_claim(task.id, now + 112)
        .unwrap();
    let receipt = keeper
        .apply_federation_task_command(&credential, &claim.command, now + 113)
        .unwrap();
    member
        .apply_federation_task_command_receipt(&receipt, now + 113)
        .unwrap();
    let page = keeper
        .federation_task_page(&credential, 0, now + 114)
        .unwrap();
    member.apply_federation_task_page(&page, now + 114).unwrap();
    let worker = member
        .create_worker("Platform", ProviderKind::ClaudeCode, "/workspace", false, 0)
        .unwrap();
    let local = member
        .materialize_local_apiary_task_execution(task.id, worker.id, now + 115)
        .unwrap()
        .local_task_id;
    member.transition_task(local, TaskState::Active).unwrap();
    sync(&keeper, &member, &credential, now + 120);
    assert_eq!(on_keeper_board(&keeper, task.id).state, TaskState::Active);
    let member_hive = member.local_hive_identity().unwrap().hive.id;
    SharedWork {
        keeper,
        member,
        member_hive,
        credential,
        watcher,
        watcher_credential: watcher_acceptance.node_credential,
        task: task.id,
        local,
    }
}

fn outside_block(recheck: i64) -> String {
    format!(
        "Waiting on: Spectrum\nNeeds: X4 back to Available\nRecheck: {}",
        format_rfc3339_seconds(recheck)
    )
}

/// The point of part 3: another Hive sees why shared work is blocked, who
/// must act and when it comes back, and sees the wait end when it does.
#[test]
fn a_blocked_shared_task_carries_its_wait_to_every_hive_and_drops_it_on_return() {
    let now = 900_000;
    let work = shared_work_running(now);
    let recheck = wall_clock(&work.member) + 2 * 86_400;
    work.member
        .transition_task_with_note(work.local, TaskState::Blocked, &outside_block(recheck))
        .unwrap();

    let sent = sync(&work.keeper, &work.member, &work.credential, now + 130);
    assert_eq!(sent.len(), 1);
    let wait = sent[0].wait.clone().expect("the block carries its wait");
    assert_eq!(wait.kind, "outside");
    assert_eq!(wait.party.as_deref(), Some("Spectrum"));
    assert_eq!(wait.needs, "X4 back to Available");
    assert_eq!(wait.resume_at, Some(recheck));
    assert_eq!(wait.hive_id, None, "only an operator wait names a Hive");

    let kept = on_keeper_board(&work.keeper, work.task);
    assert_eq!(kept.state, TaskState::Blocked);
    assert_eq!(kept.wait.as_ref(), Some(&wait), "the Keeper's row holds it");
    let seen = seen_by(
        &work.watcher,
        &work.keeper,
        &work.watcher_credential,
        work.task,
        now + 131,
    );
    assert_eq!(seen.wait, Some(wait));
    assert!(
        sync(&work.keeper, &work.member, &work.credential, now + 132).is_empty(),
        "a wait the Keeper already holds is not sent again"
    );

    // The recheck arrives, and the local task comes back by itself.
    let resumed = work.member.resume_cleared_waits(recheck).unwrap();
    assert_eq!(resumed.resumed, vec![work.local]);
    let sent = sync(&work.keeper, &work.member, &work.credential, now + 140);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].target_state, Some(TaskState::Ready));
    assert_eq!(sent[0].wait, None);
    let seen = seen_by(
        &work.watcher,
        &work.keeper,
        &work.watcher_credential,
        work.task,
        now + 141,
    );
    assert_eq!(seen.state, TaskState::Ready);
    assert_eq!(seen.wait, None, "work that left Blocked is not waiting");
    assert_eq!(on_keeper_board(&work.keeper, work.task).wait, None);
}

/// "RCG Networks is waiting on you" has to name RCG Networks, and a member
/// must not be able to name some other Hive's operator.
#[test]
fn an_operator_wait_names_the_hive_whose_operator_must_act_and_no_other() {
    let now = 900_000;
    let work = shared_work_running(now);
    work.member
        .transition_task_with_note(
            work.local,
            TaskState::Blocked,
            "Waiting on: operator\nNeeds: Approve M2 in the Entra portal",
        )
        .unwrap();
    work.member
        .prepare_local_apiary_task_lifecycle_commands(now + 130)
        .unwrap();
    let mut command = work
        .member
        .list_federation_task_outbox()
        .unwrap()
        .into_iter()
        .find(|entry| entry.state == FederationTaskOutboxState::Queued)
        .expect("the block is staged")
        .command;
    let sent = command.wait.clone().expect("the block carries its wait");
    assert_eq!(sent.kind, "operator_action");
    assert_eq!(sent.hive_id, Some(work.member_hive));

    // Whatever a member writes, the Keeper records the Hive that sent it.
    if let Some(wait) = command.wait.as_mut() {
        wait.hive_id = Some(HiveId::new());
    }
    let receipt = work
        .keeper
        .apply_federation_task_command(&work.credential, &command, now + 131)
        .unwrap();
    assert_eq!(receipt.outcome, FederationTaskCommandOutcome::Applied);
    let kept = on_keeper_board(&work.keeper, work.task)
        .wait
        .expect("the Keeper holds the wait");
    assert_eq!(kept.hive_id, Some(work.member_hive));
    assert_eq!(kept.needs, "Approve M2 in the Entra portal");
}

/// A wait the Keeper cannot show, or one on anything but a block, is refused
/// before it touches the task.
#[test]
fn the_keeper_takes_a_wait_only_with_a_block() {
    let now = 900_000;
    let work = shared_work_running(now);
    let revision = on_keeper_board(&work.keeper, work.task).revision;
    let wait = swarm_domain::ApiaryTaskWait {
        kind: "outside".into(),
        needs: "X4 back".into(),
        party: Some("Spectrum".into()),
        resume_at: Some(now + 86_400),
        hive_id: None,
    };
    let command = |target: TaskState, wait: swarm_domain::ApiaryTaskWait| FederationTaskCommand {
        id: FederationTaskCommandId::new(),
        apiary_id: on_keeper_board(&work.keeper, work.task).apiary_id,
        task_id: work.task,
        expected_revision: revision,
        kind: FederationTaskCommandKind::Transition,
        target_state: Some(target),
        filing: None,
        closure: None,
        wait: Some(wait),
        created_at: now + 130,
    };
    for refused in [
        command(TaskState::Review, wait.clone()),
        command(
            TaskState::Blocked,
            swarm_domain::ApiaryTaskWait {
                needs: "x".repeat(swarm_domain::MAX_WAIT_NEEDS_BYTES + 1),
                ..wait.clone()
            },
        ),
    ] {
        assert!(matches!(
            work.keeper
                .apply_federation_task_command(&work.credential, &refused, now + 131),
            Err(TaskStoreError::InvalidFederationTask)
        ));
    }
    assert_eq!(on_keeper_board(&work.keeper, work.task).revision, revision);
    let receipt = work
        .keeper
        .apply_federation_task_command(
            &work.credential,
            &command(TaskState::Blocked, wait.clone()),
            now + 132,
        )
        .unwrap();
    assert_eq!(receipt.outcome, FederationTaskCommandOutcome::Applied);
    assert_eq!(on_keeper_board(&work.keeper, work.task).wait, Some(wait));
}

/// An outside wait comes back, the worker finds the party has not acted and
/// blocks again with a later recheck, all before the member next syncs. The
/// Keeper still has the task Blocked, so only the wait changed, and it has to
/// reach the other Hives as one.
#[test]
fn a_wait_that_changes_while_blocked_reaches_every_hive() {
    let now = 900_000;
    let work = shared_work_running(now);
    let first = wall_clock(&work.member) + 2 * 86_400;
    work.member
        .transition_task_with_note(work.local, TaskState::Blocked, &outside_block(first))
        .unwrap();
    sync(&work.keeper, &work.member, &work.credential, now + 130);

    work.member.resume_cleared_waits(first).unwrap();
    work.member
        .transition_task(work.local, TaskState::Active)
        .unwrap();
    let later = first + 86_400;
    work.member
        .transition_task_with_note(work.local, TaskState::Blocked, &outside_block(later))
        .unwrap();

    let sent = sync(&work.keeper, &work.member, &work.credential, now + 140);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].target_state, Some(TaskState::Blocked));
    let seen = seen_by(
        &work.watcher,
        &work.keeper,
        &work.watcher_credential,
        work.task,
        now + 141,
    );
    assert_eq!(seen.state, TaskState::Blocked);
    assert_eq!(seen.wait.and_then(|wait| wait.resume_at), Some(later));
    assert!(
        sync(&work.keeper, &work.member, &work.credential, now + 142).is_empty(),
        "once the Keeper holds the new wait nothing more is sent"
    );
}

/// ⚠️ AN OLDER KEEPER REFUSES BLOCKED TO BLOCKED, and a refused command stops
/// this member staging anything more for the task, so its state would freeze
/// at the Keeper for good. A member sends one only when its Keeper said it
/// takes them, and hears that from the next page even with nothing new on it.
#[test]
fn a_keeper_that_does_not_take_wait_updates_is_never_sent_one() {
    let now = 900_000;
    let work = shared_work_running(now);
    let older_keeper = |at: i64| {
        let mut page = work
            .keeper
            .federation_task_page(&work.credential, 0, at)
            .unwrap();
        page.accepts_wait_updates = false;
        work.member.apply_federation_task_page(&page, at).unwrap();
    };
    older_keeper(now + 125);
    work.member
        .transition_task_with_note(
            work.local,
            TaskState::Blocked,
            "Parked while the vendor replies.",
        )
        .unwrap();
    // The block itself goes, as it does to any Keeper.
    work.member
        .prepare_local_apiary_task_lifecycle_commands(now + 130)
        .unwrap();
    for entry in work.member.list_federation_task_outbox().unwrap() {
        if entry.state == FederationTaskOutboxState::Queued {
            let receipt = work
                .keeper
                .apply_federation_task_command(&work.credential, &entry.command, now + 130)
                .unwrap();
            work.member
                .apply_federation_task_command_receipt(&receipt, now + 130)
                .unwrap();
        }
    }
    older_keeper(now + 131);

    // Typing the block afterwards changes only the wait.
    let blocker = work
        .member
        .create_task("Vendor ticket", "/workspace")
        .unwrap();
    work.member
        .add_task_prerequisite(
            work.local,
            blocker.id,
            "the vendor's fix lands first",
            &TaskActivityActor::operator(),
            now + 132,
        )
        .unwrap();
    assert_eq!(
        work.member
            .prepare_local_apiary_task_lifecycle_commands(now + 133)
            .unwrap(),
        0,
        "nothing is sent to a Keeper that would refuse it"
    );

    // The Keeper updates. Its next page carries no new event, only the flag.
    let page = work
        .keeper
        .federation_task_page(&work.credential, 0, now + 134)
        .unwrap();
    assert!(page.accepts_wait_updates);
    work.member
        .apply_federation_task_page(&page, now + 134)
        .unwrap();
    let sent = sync(&work.keeper, &work.member, &work.credential, now + 135);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].wait.as_ref().map(|wait| wait.kind.as_str()),
        Some(TaskWaitKind::Task.as_str())
    );
    assert_eq!(on_keeper_board(&work.keeper, work.task).wait, sent[0].wait);
}

/// A Blocked task lifted to the Apiary keeps saying why, on the Keeper's own
/// board as well as in the feed.
#[test]
fn a_relocated_blocked_task_keeps_its_wait() {
    let now = 1_700_000_000;
    let keeper = TaskStore::in_memory().expect("keeper");
    keeper
        .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
        .expect("apiary");
    let local = keeper
        .create_task("Spectrum outage", "/rcg-platform")
        .unwrap();
    keeper.transition_task(local.id, TaskState::Ready).unwrap();
    keeper.transition_task(local.id, TaskState::Active).unwrap();
    let recheck = wall_clock(&keeper) + 2 * 86_400;
    keeper
        .transition_task_with_note(local.id, TaskState::Blocked, &outside_block(recheck))
        .unwrap();

    let shared = keeper.relocate_local_task_to_apiary(local.id, now).unwrap();
    let wait = shared.wait.clone().expect("the wait moves with the task");
    assert_eq!(wait.kind, "outside");
    assert_eq!(wait.resume_at, Some(recheck));
    let kept = on_keeper_board(&keeper, shared.id);
    assert_eq!(kept.state, TaskState::Blocked);
    assert_eq!(kept.wait, Some(wait));
}
