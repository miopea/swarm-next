//! Review coverage is distinct from task completion and queue clearance.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{DecisionRequestId, NextMoveOwner, TaskId, TaskState};

pub const MAX_QUEEN_REVIEW_OBLIGATIONS: usize = 256;
pub const QUEEN_REVIEW_FOCUS_SIZE: usize = 3;
const MAX_EVIDENCE_REVISION_BYTES: usize = 128;

/// Rotate attention through a bounded candidate set in stable identity order.
/// Check timestamps and refreshed judgments must not move the cursor's peers.
/// This is not
/// coverage, execution priority, or permission to alter a task's gate.
///
/// # Errors
/// Rejects partial-sized overflow and duplicate identities rather than claiming
/// a fair traversal of an ambiguous candidate list.
pub fn next_queen_review_focus(
    candidates: &[TaskId],
    after: Option<TaskId>,
) -> Result<Vec<TaskId>, &'static str> {
    if candidates.len() > MAX_QUEEN_REVIEW_OBLIGATIONS {
        return Err("review focus candidate capacity exceeded");
    }
    let unique = candidates.iter().copied().collect::<HashSet<_>>();
    if unique.len() != candidates.len() {
        return Err("review focus candidates contain duplicate tasks");
    }
    let mut candidates = candidates.to_vec();
    candidates.sort_by_key(ToString::to_string);
    let start = after.map_or(0, |id| {
        let cursor = id.to_string();
        candidates.partition_point(|candidate| candidate.to_string() <= cursor)
    });
    Ok(candidates
        .iter()
        .cycle()
        .skip(start)
        .take(QUEEN_REVIEW_FOCUS_SIZE.min(candidates.len()))
        .copied()
        .collect())
}

#[cfg(test)]
mod focus_rotation_tests {
    use super::*;

    #[test]
    fn recurring_fresh_checks_cannot_monopolize_a_stable_backlog() {
        // Six recurring external checks precede nine unchanged investigations.
        // Taking the first three each run never reaches any investigation.
        let mut candidates = (0..15).map(|_| TaskId::new()).collect::<Vec<_>>();
        candidates.sort_by_key(ToString::to_string);
        let mut cursor = None;
        let mut seen = HashSet::new();
        for _ in 0..5 {
            let focus = next_queen_review_focus(&candidates, cursor).unwrap();
            assert_eq!(focus.len(), QUEEN_REVIEW_FOCUS_SIZE);
            cursor = focus.last().copied();
            seen.extend(focus);
        }
        assert_eq!(seen.len(), candidates.len());
        assert_eq!(
            next_queen_review_focus(&candidates, cursor).unwrap(),
            candidates[..3]
        );
    }

    #[test]
    fn refreshed_check_order_cannot_skip_older_investigations() {
        let mut candidates = (0..15).map(|_| TaskId::new()).collect::<Vec<_>>();
        let mut cursor = None;
        let mut seen = HashSet::new();
        for _ in 0..5 {
            candidates.rotate_left(2);
            candidates.reverse();
            let focus = next_queen_review_focus(&candidates, cursor).unwrap();
            cursor = focus.last().copied();
            for task in focus {
                assert!(
                    seen.insert(task),
                    "repeated before reaching every candidate"
                );
            }
        }
        assert_eq!(seen.len(), candidates.len());
    }

    #[test]
    fn covered_batch_tail_leaving_candidates_does_not_restart_the_backlog() {
        let mut candidates = (0..12).map(|_| TaskId::new()).collect::<Vec<_>>();
        candidates.sort_by_key(ToString::to_string);
        let original = candidates.clone();
        let mut cursor = None;
        let mut visited = Vec::new();
        for _ in 0..4 {
            let focus = next_queen_review_focus(&candidates, cursor).unwrap();
            cursor = focus.last().copied();
            visited.extend(focus);
            // A checked operator deferral is no longer an uncovered candidate;
            // recurring external checks earlier in the batch remain eligible.
            candidates.retain(|id| Some(*id) != cursor);
        }
        assert_eq!(visited, original);
        assert_eq!(
            next_queen_review_focus(&candidates, cursor).unwrap(),
            original[..2]
                .iter()
                .chain(original[3..4].iter())
                .copied()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn shrinking_empty_and_removed_cursor_sets_do_not_repeat_an_identity() {
        let mut candidates = [TaskId::new(), TaskId::new()];
        candidates.sort_by_key(ToString::to_string);
        assert_eq!(
            next_queen_review_focus(&candidates, Some(candidates[0])).unwrap(),
            vec![candidates[1], candidates[0]]
        );
        assert_eq!(
            next_queen_review_focus(&candidates, Some(TaskId::new())).unwrap(),
            candidates
        );
        assert!(
            next_queen_review_focus(&[], Some(candidates[0]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ambiguous_or_oversized_snapshots_cannot_claim_fairness() {
        let id = TaskId::new();
        assert!(next_queen_review_focus(&[id, id], None).is_err());
        let overflow = (0..=MAX_QUEEN_REVIEW_OBLIGATIONS)
            .map(|_| TaskId::new())
            .collect::<Vec<_>>();
        assert!(next_queen_review_focus(&overflow, None).is_err());
    }
}

/// Evidence revisions are supplied by the authoritative application snapshot.
/// They must include dependency/decision changes, not just a task timestamp.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueenReviewObligation {
    pub task_id: TaskId,
    pub evidence_revision: String,
}

/// Task facts and their evidence identity from one consistent read snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct QueenTaskReviewEvidence {
    pub task: crate::Task,
    pub obligation: QueenReviewObligation,
    pub current_run_id: Option<String>,
    /// A saved judgment with an explicit reuse verdict, not a new assessment.
    pub previous_assessment: Option<QueenReviewAssessmentEvidence>,
}

/// Current saved judgments for the queue, separate from task lifecycle/ownership.
/// Missing or truncated evidence must never be interpreted as an all-clear.
#[derive(Clone, Debug, Serialize)]
pub struct QueenReviewQueueSnapshot {
    pub items: Vec<QueenTaskReviewEvidence>,
    pub truncated: bool,
    pub checked_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenReviewAssessmentStatus {
    InsufficientEvidence,
    CoveredForCurrentRun,
    FreshExternalCheckRequired,
    EvidenceChanged,
    NoActiveReview,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueenReviewAssessmentEvidence {
    pub assessment: QueenReviewDispositionInput,
    pub recorded_at: i64,
    pub status: QueenReviewAssessmentStatus,
}

/// Reusing a receipt never means resuming or completing its task.
///
/// A receipt whose kind no longer covers the task's current state is never
/// reported covered (ADR 0135), so this view agrees with run coverage: an
/// external condition saved on Review before the rule asks for a fresh check,
/// and recording one is then refused with the move to make.
#[must_use]
pub fn queen_review_assessment_status(
    kind: QueenReviewDispositionKind,
    recorded_run: &str,
    current_run: Option<&str>,
    revision_matches: bool,
    state: TaskState,
    owner: NextMoveOwner,
) -> QueenReviewAssessmentStatus {
    if !revision_matches {
        QueenReviewAssessmentStatus::EvidenceChanged
    } else if kind == QueenReviewDispositionKind::InsufficientEvidence {
        QueenReviewAssessmentStatus::InsufficientEvidence
    } else if let Some(current_run) = current_run {
        if kind.covers(state, owner)
            && (kind == QueenReviewDispositionKind::OperatorDeferral || recorded_run == current_run)
        {
            QueenReviewAssessmentStatus::CoveredForCurrentRun
        } else {
            QueenReviewAssessmentStatus::FreshExternalCheckRequired
        }
    } else {
        QueenReviewAssessmentStatus::NoActiveReview
    }
}

/// Only receipts validated by the disposition command enter this evaluation.
/// This type is deliberately not a deserializable agent claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedQueenReviewReceipt {
    pub task_id: TaskId,
    pub evidence_revision: String,
}

/// An accepted assessment may explicitly leave its obligation uncovered.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RecordedQueenReviewAssessment {
    pub task_id: TaskId,
    pub evidence_revision: String,
    pub covers_wait: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum QueenReviewCoverage {
    Covered { waiting_obligations: usize },
    Missing { task_ids: Vec<TaskId> },
    Unavailable,
}

/// What a Queen run left unaccounted for, named so an incomplete finish says
/// what is left (ADR 0135).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueenRunUncovered {
    /// Up to [`MAX_NAMED_UNCOVERED`] named items.
    pub work: Vec<UncoveredQueenWork>,
    /// How many items were uncovered in all.
    pub total: usize,
    /// Coverage could not be read in full, so the list may be short.
    pub unavailable: bool,
}

/// How many uncovered items a finish reply names, so it stays readable.
pub const MAX_NAMED_UNCOVERED: usize = 16;

impl QueenRunUncovered {
    /// Names what a finished run left, from coverage read after it finished.
    ///
    /// ⚠️ THE RUN IS ALREADY RECORDED when this is read, so nothing here may
    /// fail. A coverage that could not be read (`None`), or a task that has
    /// gone since, marks the list `unavailable` instead. An error here would
    /// turn a finish that succeeded into one Queen is told never to repeat.
    pub fn from_coverage(
        coverage: impl IntoIterator<Item = (Option<QueenReviewCoverage>, UncoveredObligation)>,
        mut describe: impl FnMut(TaskId) -> Option<(TaskState, String)>,
    ) -> Self {
        let mut uncovered = Self {
            work: Vec::new(),
            total: 0,
            unavailable: false,
        };
        for (coverage, obligation) in coverage {
            match coverage {
                Some(QueenReviewCoverage::Covered { .. }) => {}
                None | Some(QueenReviewCoverage::Unavailable) => uncovered.unavailable = true,
                Some(QueenReviewCoverage::Missing { task_ids }) => {
                    uncovered.total += task_ids.len();
                    for task_id in task_ids {
                        if uncovered.work.len() == MAX_NAMED_UNCOVERED {
                            break;
                        }
                        match describe(task_id) {
                            Some((state, title)) => uncovered.work.push(UncoveredQueenWork {
                                task_id,
                                state,
                                title,
                                obligation,
                                next: moves_off_queens_list(obligation, state),
                            }),
                            None => uncovered.unavailable = true,
                        }
                    }
                }
            }
        }
        uncovered
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UncoveredQueenWork {
    pub task_id: TaskId,
    pub state: TaskState,
    pub title: String,
    pub obligation: UncoveredObligation,
    /// The moves that take a Draft or Review item off Queen's list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<&'static str>,
}

/// What takes an uncovered item off Queen's list, for the two states where she
/// kept choosing something that does not (ADR 0135).
///
/// ⚠️ MEASURED 2026-11-01, after the first ADR 0135 fix: every run for three
/// hours finished incomplete on the same three items. Two were Review tasks
/// whose returned review the worker had answered, which hands the next move
/// back to Queen. She answered with an ordinary message ("it stays with you")
/// and an `insufficient_evidence` record each run, and neither moves anything.
/// The list named the tasks but not the way out.
///
/// Other states and recovery items carry no hint here, because why they are
/// Queen's depends on facts this list does not hold.
#[must_use]
pub const fn moves_off_queens_list(
    obligation: UncoveredObligation,
    state: TaskState,
) -> Option<&'static str> {
    match (obligation, state) {
        (UncoveredObligation::Review, TaskState::Review) => Some(REVIEW_LEAVES_YOUR_LIST),
        (UncoveredObligation::Review, TaskState::Draft) => Some(DRAFT_LEAVES_YOUR_LIST),
        _ => None,
    }
}

const REVIEW_LEAVES_YOUR_LIST: &str = "It stays yours until one of these happens. Approved and only waiting to ship: move it to awaiting_release. Finished and verified: completed. The worker still owes work or an answer: swarm_return_reviewed_work, which works again after they have answered an earlier return. Waits on another task: add that task as a prerequisite. Needs the operator's judgment: swarm_request_decision. A message or an insufficient_evidence record leaves it yours.";

const DRAFT_LEAVES_YOUR_LIST: &str = "It stays yours until one of these happens. Make it ready and assign it if nobody holds it, or abandoned if it should not be done. Waits on another task: add that task as a prerequisite. Waits on the operator: ask once with swarm_request_decision, then act on the answer or cite it as an operator_deferral. An insufficient_evidence record leaves it yours.";

/// Said when Queen messages the worker on a Review task that is still hers
/// (ADR 0135). A message never changes who owes the next move.
pub const A_MESSAGE_HANDS_NOTHING_BACK: &str = "This task is in Review and its next move is still yours: a message hands nothing back. If the worker owes more work or an answer, return it with swarm_return_reviewed_work, which works again after they have answered an earlier return. If it is approved and only waits to ship, move it to awaiting_release.";

/// Which duty a run did not account for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UncoveredObligation {
    /// A Queen-owned task with no move and no assessment that covers it.
    Review,
    /// A worker whose stalled work Queen has not assessed.
    Recovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueenReviewDispositionKind {
    ExternalCondition,
    OperatorDeferral,
    InsufficientEvidence,
}

/// A proposed assessment, not proof of operator authority or task completion.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueenReviewDispositionInput {
    pub task_id: TaskId,
    pub run_id: String,
    pub expected_revision: String,
    pub kind: QueenReviewDispositionKind,
    pub condition: String,
    pub evidence: String,
    pub source: String,
    pub operator_activity_sequence: Option<i64>,
    pub operator_decision_id: Option<DecisionRequestId>,
}

impl QueenReviewDispositionInput {
    /// Validate shape only; persistence verifies source authority and revision.
    ///
    /// # Errors
    /// Refuses missing/broad evidence and unsupported claims of human authority.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.run_id.parse::<uuid::Uuid>().is_err()
            || self.expected_revision.len() != 64
            || !self
                .expected_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("supply the current run ID and exact task review evidence revision");
        }
        for (value, maximum) in [
            (&self.condition, 1000),
            (&self.evidence, 2000),
            (&self.source, 1000),
        ] {
            if value.trim().is_empty() || value.len() > maximum {
                return Err("condition, checked evidence and source must be nonempty and bounded");
            }
        }
        if self
            .operator_activity_sequence
            .is_some_and(|sequence| sequence <= 0)
        {
            return Err("operator activity sequence must identify an existing source event");
        }
        let sources = usize::from(self.operator_activity_sequence.is_some())
            + usize::from(self.operator_decision_id.is_some());
        match self.kind {
            QueenReviewDispositionKind::OperatorDeferral if sources != 1 => Err(
                "an operator deferral requires exactly one authenticated task-linked operator source",
            ),
            QueenReviewDispositionKind::ExternalCondition
            | QueenReviewDispositionKind::InsufficientEvidence
                if sources != 0 =>
            {
                Err("a non-operator assessment cannot assert operator authority")
            }
            _ => Ok(()),
        }
    }

    /// Why this assessment cannot be recorded on a task in `state`, or `None`
    /// when it can.
    #[must_use]
    pub fn refusal(&self, state: TaskState, owner: NextMoveOwner) -> Option<&'static str> {
        let recordable = self.can_cover(state, owner)
            || (self.kind == QueenReviewDispositionKind::InsufficientEvidence
                && owner == NextMoveOwner::Queen
                && !matches!(state, TaskState::Completed | TaskState::Abandoned));
        if recordable {
            None
        } else if self.kind == QueenReviewDispositionKind::ExternalCondition
            && state == TaskState::Review
        {
            Some(REVIEW_IS_NOT_A_WAIT)
        } else {
            Some(
                "this is not Queen-owned waiting work; route Ready work or use the existing dependency, decision and lifecycle commands",
            )
        }
    }

    #[must_use]
    pub fn can_cover(&self, state: TaskState, owner: NextMoveOwner) -> bool {
        self.kind.covers(state, owner)
    }
}

/// ⚠️ REVIEW IS NOT A PLACE TO WAIT (ADR 0135). An external condition used to
/// cover a Review task, so approved work waiting only to merge and ship sat in
/// Review behind a fresh "waiting on deploy" note every run. Each note
/// satisfied coverage, so nothing ever pushed the work on. Measured 2026-11-01:
/// one worker had 18 approved tasks, re-noted up to 19 times each. Awaiting release
/// is where finished, unshipped work goes, and it closes itself.
pub const REVIEW_IS_NOT_A_WAIT: &str = "Review is not a place to wait, so an external condition is refused on a Review task (ADR 0135). If the work is approved and only waits to be merged or shipped, move it to awaiting_release with swarm_transition_task: it needs no evidence and completes itself when the deployment is recorded. If something is missing, or the work waits on an outside party or a date, hand it back with swarm_return_reviewed_work so its worker blocks it with a named wait. If it waits on another task, add that task as a prerequisite. If the operator must judge, ask with swarm_request_decision.";

impl QueenReviewDispositionKind {
    /// Whether a saved assessment of this kind accounts for a task that is in
    /// `state` now. Coverage asks this of every saved receipt, so a receipt
    /// recorded under an older rule stops covering a state that no longer
    /// accepts it.
    #[must_use]
    pub fn covers(self, state: TaskState, owner: NextMoveOwner) -> bool {
        owner == NextMoveOwner::Queen
            && match self {
                Self::InsufficientEvidence => false,
                Self::ExternalCondition => state == TaskState::Blocked,
                Self::OperatorDeferral => matches!(
                    state,
                    TaskState::Draft | TaskState::Blocked | TaskState::Review
                ),
            }
    }
}

/// Check current, already-verified receipts without changing any task state.
/// Partial snapshots and ambiguous receipts never certify coverage.
#[must_use]
pub fn queen_review_coverage(
    obligations: &[QueenReviewObligation],
    receipts: &[VerifiedQueenReviewReceipt],
    snapshot_complete: bool,
) -> QueenReviewCoverage {
    if !snapshot_complete
        || obligations.len() > MAX_QUEEN_REVIEW_OBLIGATIONS
        || receipts.len() > MAX_QUEEN_REVIEW_OBLIGATIONS
    {
        return QueenReviewCoverage::Unavailable;
    }
    let mut expected = HashSet::new();
    for obligation in obligations {
        if obligation.evidence_revision.trim().is_empty()
            || obligation.evidence_revision.len() > MAX_EVIDENCE_REVISION_BYTES
            || !expected.insert(obligation.task_id)
        {
            return QueenReviewCoverage::Unavailable;
        }
    }
    let mut observed = HashMap::new();
    for receipt in receipts {
        if receipt.evidence_revision.trim().is_empty()
            || receipt.evidence_revision.len() > MAX_EVIDENCE_REVISION_BYTES
            || observed
                .insert(receipt.task_id, &receipt.evidence_revision)
                .is_some()
        {
            return QueenReviewCoverage::Unavailable;
        }
    }
    let missing: Vec<_> = obligations
        .iter()
        .filter(|obligation| {
            observed.get(&obligation.task_id).copied() != Some(&obligation.evidence_revision)
        })
        .map(|obligation| obligation.task_id)
        .collect();
    if missing.is_empty() {
        QueenReviewCoverage::Covered {
            waiting_obligations: obligations.len(),
        }
    } else {
        QueenReviewCoverage::Missing { task_ids: missing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assessment(kind: QueenReviewDispositionKind) -> QueenReviewDispositionInput {
        QueenReviewDispositionInput {
            task_id: TaskId::new(),
            run_id: uuid::Uuid::nil().to_string(),
            expected_revision: "0".repeat(64),
            kind,
            condition: "A fictional vendor window".into(),
            evidence: "Read the fictional status page".into(),
            source: "Fictional status page".into(),
            operator_activity_sequence: None,
            operator_decision_id: None,
        }
    }

    /// Measured 2026-11-01: approved work sat in Review behind a fresh "waiting
    /// on deploy" note every run, and each note satisfied coverage (ADR 0135).
    #[test]
    fn review_is_not_a_place_to_wait_on_an_outside_condition() {
        let outside = assessment(QueenReviewDispositionKind::ExternalCondition);
        assert!(!outside.can_cover(TaskState::Review, NextMoveOwner::Queen));
        let refusal = outside
            .refusal(TaskState::Review, NextMoveOwner::Queen)
            .expect("an outside wait is refused on Review");
        for exit in [
            "awaiting_release",
            "swarm_return_reviewed_work",
            "prerequisite",
            "swarm_request_decision",
        ] {
            assert!(
                refusal.contains(exit),
                "the refusal names {exit}: {refusal}"
            );
        }

        // Blocked is where an outside wait belongs, and it still covers there.
        assert!(outside.can_cover(TaskState::Blocked, NextMoveOwner::Queen));
        assert_eq!(
            outside.refusal(TaskState::Blocked, NextMoveOwner::Queen),
            None
        );

        // An operator's word still parks Review, and an unknown answer is still
        // recordable there without covering anything.
        let deferral = QueenReviewDispositionKind::OperatorDeferral;
        assert!(deferral.covers(TaskState::Review, NextMoveOwner::Queen));
        let unknown = assessment(QueenReviewDispositionKind::InsufficientEvidence);
        assert_eq!(
            unknown.refusal(TaskState::Review, NextMoveOwner::Queen),
            None
        );
        assert!(!unknown.can_cover(TaskState::Review, NextMoveOwner::Queen));

        // Work that is not Queen's keeps the general refusal.
        let general = outside
            .refusal(TaskState::Active, NextMoveOwner::Worker)
            .expect("active work is not Queen-owned waiting work");
        assert!(
            general.contains("not Queen-owned waiting work"),
            "{general}"
        );
    }

    /// The finish is recorded before this list is built, so a failed read or
    /// a task removed since must say `unavailable`, never fail (ADR 0135).
    #[test]
    fn an_incomplete_finish_names_what_is_left_without_failing() {
        let gone = TaskId::new();
        let named: Vec<TaskId> = (0..MAX_NAMED_UNCOVERED + 3)
            .map(|_| TaskId::new())
            .collect();
        let uncovered = QueenRunUncovered::from_coverage(
            [
                (
                    Some(QueenReviewCoverage::Missing {
                        task_ids: std::iter::once(gone).chain(named.clone()).collect(),
                    }),
                    UncoveredObligation::Review,
                ),
                (
                    Some(QueenReviewCoverage::Covered {
                        waiting_obligations: 0,
                    }),
                    UncoveredObligation::Recovery,
                ),
            ],
            |task_id| (task_id != gone).then(|| (TaskState::Draft, "Fixture draft".to_owned())),
        );
        assert_eq!(
            uncovered.total,
            MAX_NAMED_UNCOVERED + 4,
            "every item counts"
        );
        assert_eq!(
            uncovered.work.len(),
            MAX_NAMED_UNCOVERED,
            "the named list is bounded"
        );
        assert_eq!(
            uncovered.work[0].task_id, named[0],
            "a task gone since is skipped"
        );
        assert!(uncovered.unavailable, "a task gone since says so");

        let unread =
            QueenRunUncovered::from_coverage([(None, UncoveredObligation::Recovery)], |_| None);
        assert_eq!(
            (unread.total, unread.unavailable),
            (0, true),
            "a coverage that could not be read says so"
        );

        let covered = QueenRunUncovered::from_coverage(
            [(
                Some(QueenReviewCoverage::Covered {
                    waiting_obligations: 0,
                }),
                UncoveredObligation::Review,
            )],
            |_| None,
        );
        assert_eq!((covered.total, covered.unavailable), (0, false));
    }

    /// Queen read three uncovered names every run for three hours and moved
    /// none of them: the list said what was left, not how it leaves (ADR 0135).
    #[test]
    fn each_uncovered_draft_or_review_says_how_it_leaves_queens_list() {
        let review = TaskId::new();
        let draft = TaskId::new();
        let blocked = TaskId::new();
        let stalled = TaskId::new();
        let uncovered = QueenRunUncovered::from_coverage(
            [
                (
                    Some(QueenReviewCoverage::Missing {
                        task_ids: vec![review, draft, blocked],
                    }),
                    UncoveredObligation::Review,
                ),
                (
                    Some(QueenReviewCoverage::Missing {
                        task_ids: vec![stalled],
                    }),
                    UncoveredObligation::Recovery,
                ),
            ],
            |task_id| {
                let state = if task_id == review {
                    TaskState::Review
                } else if task_id == draft {
                    TaskState::Draft
                } else if task_id == blocked {
                    TaskState::Blocked
                } else {
                    TaskState::Review
                };
                Some((state, "Fixture".to_owned()))
            },
        );
        let next = |id: TaskId| {
            uncovered
                .work
                .iter()
                .find(|item| item.task_id == id)
                .expect("named")
                .next
        };

        let review_moves = next(review).expect("a Review item names its exits");
        for exit in [
            "awaiting_release",
            "swarm_return_reviewed_work",
            "works again after they have answered",
            "prerequisite",
            "swarm_request_decision",
            "A message or an insufficient_evidence record leaves it yours",
        ] {
            assert!(review_moves.contains(exit), "{exit}: {review_moves}");
        }

        let draft_moves = next(draft).expect("a Draft item names its exits");
        // Ready alone leaves an unassigned task Queen's to route.
        for exit in [
            "ready",
            "assign it if nobody holds it",
            "abandoned",
            "prerequisite",
            "swarm_request_decision",
            "operator_deferral",
        ] {
            assert!(draft_moves.contains(exit), "{exit}: {draft_moves}");
        }
        assert!(
            !draft_moves.contains("blocked"),
            "a draft cannot be blocked, so the hint must not offer it: {draft_moves}"
        );

        // Why other items are Queen's depends on facts this list does not
        // hold, so they carry no hint rather than a wrong one.
        assert_eq!(next(blocked), None);
        assert_eq!(next(stalled), None, "a recovery item is not a review exit");
    }

    #[test]
    fn missing_evidence_is_never_a_verified_wait_in_any_run() {
        for run in [None, Some("original"), Some("later")] {
            assert_eq!(
                queen_review_assessment_status(
                    QueenReviewDispositionKind::InsufficientEvidence,
                    "original",
                    run,
                    true,
                    TaskState::Blocked,
                    NextMoveOwner::Queen
                ),
                QueenReviewAssessmentStatus::InsufficientEvidence
            );
            assert_eq!(
                queen_review_assessment_status(
                    QueenReviewDispositionKind::InsufficientEvidence,
                    "original",
                    run,
                    false,
                    TaskState::Blocked,
                    NextMoveOwner::Queen
                ),
                QueenReviewAssessmentStatus::EvidenceChanged
            );
        }
    }

    #[test]
    fn an_unchanged_valid_wait_is_covered_not_declared_finished() {
        let task_id = TaskId::new();
        let obligations = [QueenReviewObligation {
            task_id,
            evidence_revision: "verified-window-v1".into(),
        }];
        let receipts = [VerifiedQueenReviewReceipt {
            task_id,
            evidence_revision: "verified-window-v1".into(),
        }];
        assert_eq!(
            queen_review_coverage(&obligations, &receipts, true),
            QueenReviewCoverage::Covered {
                waiting_obligations: 1
            }
        );
    }

    #[test]
    fn changed_dependency_and_missing_receipts_leave_exact_obligations() {
        let task_id = TaskId::new();
        let obligations = [QueenReviewObligation {
            task_id,
            evidence_revision: "dependency-now-completed".into(),
        }];
        let receipts = [VerifiedQueenReviewReceipt {
            task_id,
            evidence_revision: "dependency-not-completed".into(),
        }];
        for supplied in [&receipts[..], &[]] {
            assert_eq!(
                queen_review_coverage(&obligations, supplied, true),
                QueenReviewCoverage::Missing {
                    task_ids: vec![task_id]
                }
            );
        }
    }

    #[test]
    fn incomplete_empty_or_duplicate_evidence_is_not_success() {
        assert_eq!(
            queen_review_coverage(&[], &[], false),
            QueenReviewCoverage::Unavailable
        );
        let task_id = TaskId::new();
        let obligation = QueenReviewObligation {
            task_id,
            evidence_revision: "v1".into(),
        };
        let receipt = VerifiedQueenReviewReceipt {
            task_id,
            evidence_revision: "v1".into(),
        };
        assert_eq!(
            queen_review_coverage(&[obligation.clone(), obligation.clone()], &[], true),
            QueenReviewCoverage::Unavailable
        );
        assert_eq!(
            queen_review_coverage(&[obligation], &[receipt.clone(), receipt], true),
            QueenReviewCoverage::Unavailable
        );
        assert_eq!(
            queen_review_coverage(
                &[QueenReviewObligation {
                    task_id,
                    evidence_revision: String::new()
                }],
                &[],
                true
            ),
            QueenReviewCoverage::Unavailable
        );
    }

    #[test]
    fn overflow_never_silently_drops_obligations() {
        let obligations: Vec<_> = (0..=MAX_QUEEN_REVIEW_OBLIGATIONS)
            .map(|_| QueenReviewObligation {
                task_id: TaskId::new(),
                evidence_revision: "v1".into(),
            })
            .collect();
        assert_eq!(
            queen_review_coverage(&obligations, &[], true),
            QueenReviewCoverage::Unavailable
        );
    }

    #[test]
    fn full_capacity_is_supported_but_receipt_overflow_is_not() {
        let obligations: Vec<_> = (0..MAX_QUEEN_REVIEW_OBLIGATIONS)
            .map(|_| QueenReviewObligation {
                task_id: TaskId::new(),
                evidence_revision: "v1".into(),
            })
            .collect();
        let mut receipts: Vec<_> = obligations
            .iter()
            .map(|item| VerifiedQueenReviewReceipt {
                task_id: item.task_id,
                evidence_revision: item.evidence_revision.clone(),
            })
            .collect();
        assert_eq!(
            queen_review_coverage(&obligations, &receipts, true),
            QueenReviewCoverage::Covered {
                waiting_obligations: MAX_QUEEN_REVIEW_OBLIGATIONS,
            }
        );
        receipts.push(VerifiedQueenReviewReceipt {
            task_id: TaskId::new(),
            evidence_revision: "v1".into(),
        });
        assert_eq!(
            queen_review_coverage(&obligations, &receipts, true),
            QueenReviewCoverage::Unavailable
        );
    }

    #[test]
    fn a_receipt_for_another_task_does_not_cover_the_current_obligation() {
        let task_id = TaskId::new();
        let obligations = [QueenReviewObligation {
            task_id,
            evidence_revision: "v1".into(),
        }];
        let receipts = [VerifiedQueenReviewReceipt {
            task_id: TaskId::new(),
            evidence_revision: "v1".into(),
        }];
        assert_eq!(
            queen_review_coverage(&obligations, &receipts, true),
            QueenReviewCoverage::Missing {
                task_ids: vec![task_id]
            }
        );
    }

    #[test]
    fn invalid_revisions_and_partial_snapshots_never_certify_a_wait() {
        let task_id = TaskId::new();
        for revision in [" ".to_string(), "x".repeat(MAX_EVIDENCE_REVISION_BYTES + 1)] {
            let obligations = [QueenReviewObligation {
                task_id,
                evidence_revision: revision.clone(),
            }];
            let receipts = [VerifiedQueenReviewReceipt {
                task_id,
                evidence_revision: revision,
            }];
            assert_eq!(
                queen_review_coverage(&obligations, &receipts, true),
                QueenReviewCoverage::Unavailable
            );
        }
        let obligations = [QueenReviewObligation {
            task_id,
            evidence_revision: "v1".into(),
        }];
        let receipts = [VerifiedQueenReviewReceipt {
            task_id,
            evidence_revision: "v1".into(),
        }];
        assert_eq!(
            queen_review_coverage(&obligations, &receipts, false),
            QueenReviewCoverage::Unavailable
        );
        assert_eq!(
            queen_review_coverage(&[], &[], true),
            QueenReviewCoverage::Covered {
                waiting_obligations: 0
            }
        );
    }
}
