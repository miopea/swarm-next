use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::{QueenAutonomyLevel, QueenAutonomyPolicy};
use swarm_persistence::AUTOMATIC_WAKE_BATCH_LIMIT;

use super::{ApiError, AppState, authorize, task_store, task_store_error, unix_timestamp};

#[derive(Debug, Deserialize)]
pub(super) struct SetQueenAutonomyPolicyRequest {
    at_hive: QueenAutonomyLevel,
    away: QueenAutonomyLevel,
    night_watch: QueenAutonomyLevel,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetQueenAutomationRequest {
    enabled: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CoordinatorStatusResponse {
    completed_actions: usize,
    queen_calls_avoided: usize,
    uncertain_actions: usize,
    queued_actions: usize,
    stale_attention_actions: usize,
    worker_exit_attention_actions: usize,
    unstarted_attention_actions: usize,
    last_action_at: Option<i64>,
    automatic_start_admission: super::runtime::CoordinatorStartAdmission,
    automatic_start_batch_limit: usize,
    /// What the coordinator wanted to do and could not, once it has been true
    /// long enough to be worth saying. Nothing here is a fault in the
    /// coordinator: declining to type into a terminal with an unanswered
    /// prompt is correct, and saying nothing about it for a day is not.
    held: Vec<HeldDeliveryResponse>,
    /// Blocks old enough that the OPERATOR should hear about them directly,
    /// not only Queen.
    ///
    /// Twelve hours, which the operator chose over the recommended twenty-four
    /// (decision 01a0418f) — read as a preference to hear sooner, so nothing
    /// here batches or coalesces to restore a longer effective delay.
    ///
    /// Carried on the coordinator payload the control room already polls
    /// rather than on a surface of its own, so it lands in the Decision Inbox
    /// beside the other attention cards — the place workers already ask instead
    /// of interrupting a terminal.
    blocked_escalations: Vec<BlockedEscalationResponse>,
    unsettled_review: Vec<UnsettledReviewResponse>,
    /// Briefings queued and not moving, and what each is waiting on.
    ///
    /// A held delivery was attempted and refused. A briefing the dispatcher
    /// never claims is never attempted, so nothing recorded it: thirteen sat
    /// six hours with attempts at zero while the board showed work assigned and
    /// apparently ignored.
    held_briefings: Vec<swarm_persistence::HeldTaskDispatch>,
}

/// One thing the coordinator is holding, and for how long.
#[derive(Debug, Serialize)]
pub(super) struct HeldDeliveryResponse {
    /// Which kind of hold this is. The two are not the same situation: one is
    /// work waiting for a prompt to be answered, the other is work that was
    /// never started and will not be retried.
    kind: String,
    subject: String,
    worker_name: Option<String>,
    reason: String,
    first_observed_at: i64,
    last_observed_at: i64,
    observations: i64,
}

/// A stranded prompt is silent for a grace period first.
///
/// A prompt answered in ten seconds is the system working, and turning that
/// into an item would teach the operator to ignore the queue. Two minutes is
/// long enough that nobody is coming.
const HELD_DELIVERY_GRACE_SECONDS: i64 = 120;

/// Briefings queued and not moving, with what each is waiting on.
///
/// Distinct from a held delivery: those were attempted and refused, so they
/// reach the refusal ledger. A briefing the dispatcher never claims is never
/// attempted, so nothing recorded it and the board showed work assigned and
/// apparently ignored.
fn held_briefings(
    state: &Arc<AppState>,
) -> Result<Vec<swarm_persistence::HeldTaskDispatch>, ApiError> {
    crate::task_store(state)?
        .held_task_dispatches(crate::unix_timestamp())
        .map_err(|error| task_store_error(&error))
}

/// One block the operator should know about, with what it is waiting on.
#[derive(Debug, Serialize)]
pub(super) struct BlockedEscalationResponse {
    task_id: String,
    title: String,
    worker_name: String,
    workspace: String,
    blocked_for_seconds: i64,
}

/// One piece of finished work that nothing has settled, and why.
///
/// THE TYPE NAMES ITS SUBJECT, because the last count in this area did not. The
/// figure this whole design began from — "49 of 355 completed tasks carry
/// nothing anyone verified" — was really 31, because the query counted
/// unapproved exemption claims without excluding tasks that ALSO held a
/// deployment. A number was produced about something adjacent to the claim, and
/// then relayed to the operator as the sharpest evidence in the complaint.
///
/// So: this is REVIEWED WORK NOTHING HAS SETTLED. Not "unverified tasks", which
/// could mean four different populations. Work in review, not removed, carrying
/// neither a deployment nor an approved exemption — the exact set
/// `reviewed_work_awaiting_judgment` returns, and it is reused rather than
/// re-expressed so the number and the list cannot drift apart.
#[derive(Debug, Serialize)]
pub(super) struct UnsettledReviewResponse {
    task_id: String,
    title: String,
    workspace: String,
    /// Whose work this is.
    ///
    /// The operator's first question about waiting work is whose it is, and
    /// eleven rows shipped in v1.1.0 unable to answer it: "no clear which
    /// worker". Every row on this Hive has an assigned worker, so the fallback
    /// is for a task whose worker was archived, not for a normal case.
    worker_name: String,
    /// Which of the three states this is, as a stable label rather than prose.
    ///
    /// The sentence in `reason` is unchanged and still correct; it just cannot
    /// be said eleven times. Seven of eleven rows carried a byte-identical
    /// forty-eight character sentence, so the majority of the card's ink was
    /// three strings repeated and the titles — the only part that differed —
    /// competed with them for the same line. The UI shows this as a short chip
    /// and says the sentence once.
    kind: &'static str,
    /// Why a person is needed, derived from what the task recorded.
    reason: &'static str,
    /// When the work was filed, which is the only field carrying its age.
    ///
    /// NOT `updated_at`, which the list used to be ordered by. On this Hive
    /// eleven unsettled rows have eleven distinct `created_at` values and TWO
    /// distinct `updated_at` values — a bulk pass touched ten of them in the
    /// same second — so ordering by it is not merely uninformative, it is
    /// very nearly constant and the resulting order is arbitrary.
    created_at: i64,
}

/// Reviewed work the deterministic passes could not settle.
///
/// Everything the coordinator CAN settle is already gone by the time this runs:
/// work carrying a deployment closes itself, and so does work whose recorded
/// commits show there was nothing to deploy. What is left genuinely needs a
/// person, which is what earns it a place on Needs you.
fn unsettled_review(state: &Arc<AppState>) -> Result<Vec<UnsettledReviewResponse>, ApiError> {
    let store = crate::task_store(state)?;
    let waiting = store
        .reviewed_work_awaiting_judgment()
        .map_err(|error| task_store_error(&error))?;
    // One lookup per DISTINCT worker, not per row. Three of this Hive's eleven
    // rows belong to the same worker.
    let mut names: HashMap<swarm_domain::WorkerId, String> = HashMap::new();
    let mut rows: Vec<UnsettledReviewResponse> = waiting
        .into_iter()
        .filter_map(|task_id| {
            let task = store.get_task(task_id).ok()?;
            let report = store.task_commit_report(task_id).ok()?;
            let claimed = matches!(
                store.completion_evidence(task_id).ok()?,
                swarm_persistence::CompletionEvidence::ExemptionClaimed
            );
            // ORDER MATTERS AND IS DELIBERATE. A claim nobody approved is the
            // most specific thing true of a task, so it is said first; the
            // commit settlement is what is left to say when there is no claim.
            //
            // The label and the sentence are chosen together, in one place, so
            // a row cannot be chipped as one state and explained as another.
            let (kind, reason) = if claimed {
                (
                    "claim_unapproved",
                    "a claim that nothing was deployed, which nobody has approved",
                )
            } else {
                match swarm_domain::commit_settlement(report.as_ref()) {
                    swarm_domain::CommitSettlement::BuiltCode => (
                        "code_no_deployment",
                        "it recorded commits that touch code, and no deployment",
                    ),
                    swarm_domain::CommitSettlement::Unknown => (
                        "nothing_reported",
                        "nobody reported what this work produced",
                    ),
                    // Settleable, so the sweep will take it on its next pass.
                    // Present here only in the seconds between the two.
                    swarm_domain::CommitSettlement::NothingBuilt
                    | swarm_domain::CommitSettlement::DocumentationOnly => {
                        ("settling", "waiting for the coordinator to settle it")
                    }
                }
            };
            let worker_name = task.assigned_worker_id.map_or_else(
                || UNASSIGNED_WORKER_NAME.to_owned(),
                |worker_id| {
                    names
                        .entry(worker_id)
                        .or_insert_with(|| {
                            store.get_worker_profile(worker_id).map_or_else(
                                |_| UNASSIGNED_WORKER_NAME.to_owned(),
                                |profile| profile.name,
                            )
                        })
                        .clone()
                },
            );
            Some(UnsettledReviewResponse {
                task_id: task_id.to_string(),
                title: task.title,
                workspace: task.workspace,
                worker_name,
                kind,
                reason,
                created_at: task.created_at,
            })
        })
        .collect();
    // Sorted here so the list arrives in the order it is read: worker first,
    // because that is the question asked of it, then oldest first inside a
    // worker, because age is the only thing that says which has been waiting.
    rows.sort_by(|left, right| {
        left.worker_name
            .cmp(&right.worker_name)
            .then(left.created_at.cmp(&right.created_at))
    });
    Ok(rows)
}

/// What a row says when its worker cannot be named.
///
/// Reached when a task has no assigned worker, or when the worker it names has
/// since been archived — `get_worker_profile` filters those out. Neither is the
/// normal case: all eleven rows on this Hive resolve to a live worker.
const UNASSIGNED_WORKER_NAME: &str = "Unassigned";

/// Blocks past the operator's twelve-hour threshold.
///
/// The threshold lives here rather than in the query so the number the operator
/// chose is stated once, next to the reason it is that number.
fn blocked_escalations(state: &Arc<AppState>) -> Result<Vec<BlockedEscalationResponse>, ApiError> {
    let store = crate::task_store(state)?;
    let candidates = store
        .operator_block_escalation_candidates(
            crate::unix_timestamp(),
            OPERATOR_BLOCK_ESCALATION_SECONDS,
        )
        .map_err(|error| task_store_error(&error))?;
    Ok(candidates
        .into_iter()
        .filter_map(|candidate| {
            let task = store.get_task(candidate.task_id).ok()?;
            let worker = store.get_worker_profile(candidate.worker_id).ok()?;
            Some(BlockedEscalationResponse {
                task_id: candidate.task_id.to_string(),
                title: task.title,
                worker_name: worker.name,
                workspace: task.workspace,
                blocked_for_seconds: candidate.age_seconds,
            })
        })
        .collect())
}

/// Twelve hours. The operator chose it over Queen's recommended twenty-four
/// (decision 01a0418f, "Reach me after 12 hours"), which is a preference to
/// hear sooner rather than a rounding of the same answer.
const OPERATOR_BLOCK_ESCALATION_SECONDS: i64 = 12 * 60 * 60;

fn held_deliveries(state: &Arc<AppState>) -> Result<Vec<HeldDeliveryResponse>, ApiError> {
    let refusals = crate::task_store(state)?
        .standing_coordinator_refusals(crate::unix_timestamp(), HELD_DELIVERY_GRACE_SECONDS)
        .map_err(|error| task_store_error(&error))?;
    let mut held: Vec<_> = refusals
        .into_iter()
        .map(|refusal| HeldDeliveryResponse {
            kind: refusal.kind,
            subject: refusal.subject,
            worker_name: refusal.worker_name,
            reason: refusal.reason,
            first_observed_at: refusal.first_observed_at,
            last_observed_at: refusal.last_observed_at,
            observations: refusal.observations,
        })
        .collect();
    let messages = crate::task_store(state)?
        .task_message_attention()
        .map_err(|error| task_store_error(&error))?;
    held.extend(messages.items.into_iter().map(|message| HeldDeliveryResponse {
        kind: "task_message_reconciliation".into(),
        subject: message.message_id.clone(),
        worker_name: Some("Queen".into()),
        reason: format!("{}: {} delivery. Queen must inspect task {} and message {} before resolving or explicitly retrying. No automatic replay.{}",
            message.task_title, message.state, message.task_id, message.message_id,
            if message.superseded { " This review request is superseded and cannot be retried." } else { "" }),
        first_observed_at: message.updated_at,
        last_observed_at: message.updated_at,
        observations: 1,
    }));
    Ok(held)
}

pub(super) async fn queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = task_store(&state)?
        .queen_autonomy_policy()
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

pub(super) async fn set_queen_autonomy_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetQueenAutonomyPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let policy = task_store(&state)?
        .set_queen_autonomy_policy(
            QueenAutonomyPolicy {
                at_hive: request.at_hive,
                away: request.away,
                night_watch: request.night_watch,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(policy)).into_response())
}

pub(super) async fn queen_automation_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

pub(super) async fn coordinator_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let status = state
        .coordinator_status()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CoordinatorStatusResponse {
            completed_actions: status.completed_actions,
            queen_calls_avoided: status.queen_calls_avoided,
            uncertain_actions: status.uncertain_actions,
            queued_actions: status.queued_actions,
            stale_attention_actions: status.stale_attention_actions,
            worker_exit_attention_actions: status.worker_exit_attention_actions,
            unstarted_attention_actions: status.unstarted_attention_actions,
            last_action_at: status.last_action_at,
            automatic_start_admission: state.coordinator_start_admission(),
            automatic_start_batch_limit: usize::from(AUTOMATIC_WAKE_BATCH_LIMIT),
            held: held_deliveries(&state)?,
            held_briefings: held_briefings(&state)?,
            blocked_escalations: blocked_escalations(&state)?,
            unsettled_review: unsettled_review(&state)?,
        }),
    )
        .into_response())
}

pub(super) async fn set_queen_automation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetQueenAutomationRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .set_queen_automation_enabled(request.enabled, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}

pub(super) async fn run_queen_automation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    task_store(&state)?
        .request_queen_automation_run(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    state.deliver_coordination().await;
    let status = task_store(&state)?
        .queen_automation_status(unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(status)).into_response())
}
