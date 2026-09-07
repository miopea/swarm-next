//! Review coverage is distinct from task completion and queue clearance.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{DecisionRequestId, NextMoveOwner, TaskId, TaskState};

pub const MAX_QUEEN_REVIEW_OBLIGATIONS: usize = 256;
const MAX_EVIDENCE_REVISION_BYTES: usize = 128;

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
#[must_use]
pub fn queen_review_assessment_status(
    kind: QueenReviewDispositionKind,
    recorded_run: &str,
    current_run: Option<&str>,
    revision_matches: bool,
) -> QueenReviewAssessmentStatus {
    if !revision_matches {
        QueenReviewAssessmentStatus::EvidenceChanged
    } else if kind == QueenReviewDispositionKind::InsufficientEvidence {
        QueenReviewAssessmentStatus::InsufficientEvidence
    } else if let Some(current_run) = current_run {
        if kind == QueenReviewDispositionKind::OperatorDeferral || recorded_run == current_run {
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

    #[must_use]
    pub fn can_record(&self, state: TaskState, owner: NextMoveOwner) -> bool {
        self.can_cover(state, owner)
            || (self.kind == QueenReviewDispositionKind::InsufficientEvidence
                && owner == NextMoveOwner::Queen
                && !matches!(state, TaskState::Completed | TaskState::Abandoned))
    }

    #[must_use]
    pub fn can_cover(&self, state: TaskState, owner: NextMoveOwner) -> bool {
        owner == NextMoveOwner::Queen
            && match self.kind {
                QueenReviewDispositionKind::InsufficientEvidence => false,
                QueenReviewDispositionKind::ExternalCondition => {
                    matches!(state, TaskState::Blocked | TaskState::Review)
                }
                QueenReviewDispositionKind::OperatorDeferral => matches!(
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

    #[test]
    fn missing_evidence_is_never_a_verified_wait_in_any_run() {
        for run in [None, Some("original"), Some("later")] {
            assert_eq!(
                queen_review_assessment_status(
                    QueenReviewDispositionKind::InsufficientEvidence,
                    "original",
                    run,
                    true
                ),
                QueenReviewAssessmentStatus::InsufficientEvidence
            );
            assert_eq!(
                queen_review_assessment_status(
                    QueenReviewDispositionKind::InsufficientEvidence,
                    "original",
                    run,
                    false
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
