use super::{
    ApiaryId, ApiaryTask, ApiaryTaskId, ApiaryTaskSource, ApiaryTaskWait, FederationNodeId,
    FederationTaskPage, HiveId, TaskPriority, TaskState, reports_task_waits,
};
use crate::{TaskWaitKind, TaskWaitView};

fn blocked_task(wait: Option<ApiaryTaskWait>) -> ApiaryTask {
    ApiaryTask {
        id: ApiaryTaskId::new(),
        apiary_id: ApiaryId::new(),
        source: ApiaryTaskSource::Swarm,
        title: "Shared work".into(),
        description: String::new(),
        priority: TaskPriority::Normal,
        state: TaskState::Blocked,
        home_node_id: Some(FederationNodeId::new()),
        home_hive_id: Some(HiveId::new()),
        revision: 3,
        closure_evidence: None,
        wait,
        created_at: 1,
        updated_at: 2,
    }
}

fn local(kind: TaskWaitKind) -> TaskWaitView {
    TaskWaitView {
        kind,
        needs: "Spectrum restores the circuit".into(),
        party: Some("Spectrum".into()),
        resume_at: Some(1_800_000_000),
        done_at: None,
        cleared: false,
    }
}

/// A Hive on an older build wrote snapshots with no wait, and a later one may
/// send a kind this build has never heard of. Neither can fail the page.
#[test]
fn a_snapshot_without_a_wait_or_with_an_unknown_kind_still_reads() {
    let mut old = serde_json::to_value(blocked_task(None)).expect("encode");
    assert!(
        old.get("wait").is_none(),
        "no wait is written as no field, so an older Hive's snapshot is unchanged"
    );
    let read: ApiaryTask = serde_json::from_value(old.clone()).expect("older snapshot reads");
    assert_eq!(read.wait, None);

    old["wait"] = serde_json::json!({ "kind": "vendor_ticket", "needs": "Ticket 42 closes" });
    let read: ApiaryTask = serde_json::from_value(old).expect("unknown kind reads");
    let wait = read.wait.expect("the wait is kept");
    assert_eq!(wait.kind, "vendor_ticket");
    assert!(
        wait.is_well_formed(),
        "a Keeper stores a kind it does not know"
    );
}

/// Only an operator wait names a Hive, and only the Hive that sent it.
#[test]
fn an_operator_wait_names_the_hive_that_sent_it_and_no_other_wait_names_one() {
    let hive = HiveId::new();
    let operator = ApiaryTaskWait::from_local(&local(TaskWaitKind::OperatorAction), hive);
    assert_eq!(operator.kind, "operator_action");
    assert_eq!(operator.hive_id, Some(hive));

    let outside = ApiaryTaskWait::from_local(&local(TaskWaitKind::Outside), hive);
    assert_eq!(outside.hive_id, None);
    assert_eq!(outside.party.as_deref(), Some("Spectrum"));
    assert_eq!(outside.resume_at, Some(1_800_000_000));
    assert_eq!(outside.needs, "Spectrum restores the circuit");

    let claimed_elsewhere = ApiaryTaskWait {
        hive_id: Some(HiveId::new()),
        ..operator.clone()
    };
    assert_eq!(
        claimed_elsewhere.sent_by(hive),
        operator,
        "the sender is who it names, whatever the member wrote"
    );
}

/// A local wait is bounded in characters and a Keeper's in bytes. What a
/// member sends must always be stored, or its sync stops on a retry loop.
#[test]
fn a_long_local_wait_is_cut_to_what_a_keeper_stores() {
    let long = TaskWaitView {
        needs: "é".repeat(1000),
        party: Some("ü".repeat(120)),
        ..local(TaskWaitKind::Outside)
    };
    let wait = ApiaryTaskWait::from_local(&long, HiveId::new());
    assert!(wait.is_well_formed(), "{} bytes", wait.needs.len());
    assert_eq!(wait.needs.len(), crate::MAX_WAIT_NEEDS_BYTES);
    assert!(wait.needs.chars().all(|c| c == 'é'));
    assert_eq!(
        wait.party.as_deref().map(str::len),
        Some(crate::MAX_WAIT_PARTY_BYTES)
    );
}

#[test]
fn a_keeper_refuses_a_wait_it_could_not_show() {
    let wait = ApiaryTaskWait::from_local(&local(TaskWaitKind::Outside), HiveId::new());
    assert!(wait.is_well_formed());
    for bad in [
        ApiaryTaskWait {
            kind: String::new(),
            ..wait.clone()
        },
        ApiaryTaskWait {
            kind: "Outside; DROP".into(),
            ..wait.clone()
        },
        ApiaryTaskWait {
            kind: "x".repeat(33),
            ..wait.clone()
        },
        ApiaryTaskWait {
            needs: "x".repeat(crate::MAX_WAIT_NEEDS_BYTES + 1),
            ..wait.clone()
        },
        ApiaryTaskWait {
            party: Some("x".repeat(crate::MAX_WAIT_PARTY_BYTES + 1)),
            ..wait.clone()
        },
        ApiaryTaskWait {
            resume_at: Some(-1),
            ..wait.clone()
        },
    ] {
        assert!(!bad.is_well_formed(), "{bad:?} must be refused");
    }
}

#[test]
fn a_member_says_why_from_the_release_that_carries_waits() {
    assert!(!reports_task_waits("1.17.4"));
    assert!(!reports_task_waits("1.17.3"));
    assert!(reports_task_waits("1.17.5"));
    assert!(reports_task_waits("1.18.0"));
    assert!(!reports_task_waits("not a version"));
}

/// An older Keeper's page has no flag, and that must read as "do not send a
/// wait update", the only answer that cannot freeze a member's task.
#[test]
fn a_page_from_an_older_keeper_does_not_accept_wait_updates() {
    let page = FederationTaskPage {
        schema_version: 1,
        protocol_version: 1,
        apiary_id: ApiaryId::new(),
        member_node_id: FederationNodeId::new(),
        events: Vec::new(),
        next_cursor: 0,
        has_more: false,
        generated_at: 0,
        accepts_wait_updates: false,
    };
    let encoded = serde_json::to_value(&page).expect("encode");
    assert!(encoded.get("accepts_wait_updates").is_none());
    let read: FederationTaskPage = serde_json::from_value(encoded).expect("decode");
    assert!(!read.accepts_wait_updates);

    let newer = FederationTaskPage {
        accepts_wait_updates: true,
        ..page
    };
    let read: FederationTaskPage =
        serde_json::from_value(serde_json::to_value(&newer).expect("encode")).expect("decode");
    assert!(read.accepts_wait_updates);
}
