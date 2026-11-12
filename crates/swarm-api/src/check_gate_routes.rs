//! The check gate's routes (ADR 0115): this Hive's level, and CI as last read
//! for one task.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_application::ApplicationError;
use swarm_domain::{
    CheckGateLevel, CiObservation, IndependentCheck, IndependentCheckState, MustNotTouchReport,
    TaskCommitReport, independent_check_state,
};
use swarm_persistence::TaskStoreError;

use super::{
    ApiError, AppState, authorize, parse_task_id, task_service, task_store, task_store_error,
    unix_timestamp,
};

/// The check gate in effect, and the level this Hive's Apiary sets, if any:
/// that one is locked here and only its Keeper can change it (ADR 0113).
#[derive(Serialize)]
struct CheckGateView {
    level: CheckGateLevel,
    set_by_apiary: Option<CheckGateLevel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetCheckGateRequest {
    level: CheckGateLevel,
}

pub(super) async fn check_gate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .check_gate()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CheckGateView {
            level: setting.level,
            set_by_apiary: setting.set_by_apiary,
        }),
    )
        .into_response())
}

pub(super) async fn set_check_gate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetCheckGateRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let setting = task_store(&state)?
        .set_check_gate_level(request.level, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CheckGateView {
            level: setting.level,
            set_by_apiary: setting.set_by_apiary,
        }),
    )
        .into_response())
}

/// What the check gate knows about one task: the level, how many commits the
/// task recorded (none means the gate never holds it), and CI as last read.
/// Beside it, the worker's independent check of those commits (ADR 0171),
/// which is shown and never gates anything; null when the task built no code.
#[derive(Serialize)]
struct TaskCiView {
    level: CheckGateLevel,
    recorded_commits: usize,
    observation: Option<CiObservation>,
    independent_check: Option<CardCheck>,
    /// The task's must-not-touch patterns against its commits (ADR 0172),
    /// also shown and never gating; null when the task has none.
    must_not_touch: Option<CardScope>,
}

/// The task's must-not-touch report as the card reads it (ADR 0172).
#[derive(Serialize)]
#[serde(untagged)]
enum CardScope {
    Read(MustNotTouchReport),
    /// The patterns could not be read. Said beside CI and the check, and
    /// never read as no limits, which would hide a crossing.
    Unreadable {
        state: &'static str,
        why: String,
    },
}

fn card_scope(read: Result<Option<MustNotTouchReport>, ApplicationError>) -> Option<CardScope> {
    match read {
        Ok(report) => report.map(CardScope::Read),
        Err(error) => Some(CardScope::Unreadable {
            state: "unreadable",
            why: error.to_string(),
        }),
    }
}

/// The independent check as the card reads it.
#[derive(Serialize)]
#[serde(untagged)]
enum CardCheck {
    Read(IndependentCheckState),
    /// The check could not be read. Said beside CI, so a damaged check row
    /// neither takes the CI row with it nor reads as no check at all.
    Unreadable {
        state: &'static str,
        why: String,
    },
}

fn card_check(
    report: Option<&TaskCommitReport>,
    latest: &Result<Option<IndependentCheck>, TaskStoreError>,
) -> Option<CardCheck> {
    match latest {
        Ok(latest) => independent_check_state(report, latest.as_ref()).map(CardCheck::Read),
        Err(error) => Some(CardCheck::Unreadable {
            state: "unreadable",
            why: error.to_string(),
        }),
    }
}

pub(super) async fn task_ci(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let task_id = parse_task_id(&task_id)?;
    let store = task_store(&state)?;
    let level = store
        .check_gate()
        .map_err(|error| task_store_error(&error))?
        .level;
    let report = store
        .task_commit_report(task_id)
        .map_err(|error| task_store_error(&error))?;
    let observation = store
        .ci_observation(task_id)
        .map_err(|error| task_store_error(&error))?;
    let latest_check = store.latest_independent_check(task_id);
    let must_not_touch = task_service(&state)?.must_not_touch_report(task_id);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(TaskCiView {
            level,
            recorded_commits: report.as_ref().map_or(0, |report| report.commits.len()),
            observation,
            independent_check: card_check(report.as_ref(), &latest_check),
            must_not_touch: card_scope(must_not_touch),
        }),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use swarm_application::ApplicationError;
    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, IndependentCheck, IndependentCheckVerdict,
        MustNotTouchReport, TaskCommit, TaskCommitReport, TaskId,
    };
    use swarm_persistence::TaskStoreError;

    use super::{card_check, card_scope};

    /// A check that cannot be read is said to be unreadable on the card,
    /// rather than failing the CI beside it or reading as no check at all.
    #[test]
    fn a_check_that_cannot_be_read_reads_as_unreadable_and_nothing_else_changes() {
        let report = TaskCommitReport {
            task_id: TaskId::new(),
            workspace: "/work".to_owned(),
            repository_state: CommitRepositoryState::Read,
            reported_at: 1,
            commits: vec![TaskCommit {
                sha: "aaaaaaaa".to_owned(),
                verdict: CommitVerdict::Present,
                subject: "feat: a".to_owned(),
                changed_paths: vec!["src/lib.rs".to_owned()],
                found_in: None,
            }],
        };
        let damaged: Result<Option<IndependentCheck>, TaskStoreError> = Err(
            TaskStoreError::IntegrityFailure("unknown independent check verdict maybe".to_owned()),
        );

        let unreadable = json!(card_check(Some(&report), &damaged));
        assert_eq!(unreadable["state"], "unreadable", "{unreadable}");
        assert!(
            unreadable["why"]
                .as_str()
                .is_some_and(|why| why.contains("unknown independent check verdict")),
            "{unreadable}"
        );

        assert_eq!(
            json!(card_check(Some(&report), &Ok(None))),
            json!({ "state": "not_checked" })
        );
        let check = IndependentCheck {
            commits: "aaaaaaaa".to_owned(),
            head: "aaaaaaaa".to_owned(),
            verdict: IndependentCheckVerdict::Pass,
            ran: "cargo test".to_owned(),
            findings: String::new(),
            recorded_at: 2,
        };
        assert_eq!(
            json!(card_check(Some(&report), &Ok(Some(check))))["state"],
            "checked"
        );
        assert_eq!(
            json!(card_check(None, &Ok(None))),
            json!(null),
            "no code, no row"
        );
    }

    /// Must-not-touch patterns that cannot be read are said to be unreadable
    /// on the card, never shown as no limits, and the CI and the check beside
    /// them are still read (ADR 0172).
    #[test]
    fn a_scope_that_cannot_be_read_reads_as_unreadable_and_never_as_no_limits() {
        let damaged: Result<Option<MustNotTouchReport>, ApplicationError> =
            Err(ApplicationError::Store(TaskStoreError::IntegrityFailure(
                "must-not-touch patterns are not a list".to_owned(),
            )));
        let unreadable = json!(card_scope(damaged));
        assert_eq!(unreadable["state"], "unreadable", "{unreadable}");
        assert!(
            unreadable["why"]
                .as_str()
                .is_some_and(|why| why.contains("must-not-touch patterns are not a list")),
            "{unreadable}"
        );

        assert_eq!(
            json!(card_scope(Ok(None))),
            json!(null),
            "no patterns, no row"
        );
        let report = MustNotTouchReport {
            patterns: vec!["migrations/**".to_owned()],
            commits_compared: 1,
            commits_not_compared: 0,
            crossings: Vec::new(),
            more_commits: 0,
        };
        assert_eq!(
            json!(card_scope(Ok(Some(report.clone())))),
            json!(report),
            "a report reads as it always has"
        );
    }
}
