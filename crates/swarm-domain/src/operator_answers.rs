//! Exact answer correlation, not authentication or semantic approval inference.
//! The application must establish first-party origin before using these rules.

use crate::{DecisionQuestion, DecisionRequestId, WorkerId, WorkerSessionId};

/// A complete question snapshot prevents a reused header from matching changed
/// options or wording. Session identity prevents a replacement worker receiving
/// an earlier session's answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorAnswerTarget {
    pub decision_id: DecisionRequestId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub question: DecisionQuestion,
}

/// Evidence describes provider consumption, not just a successful PTY write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorAnswerConsumption {
    Unconfirmed,
    Rejected,
    Confirmed,
}

/// Caller-authenticated evidence; deliberately not deserializable from MCP/HTTP.
/// Exact text is private and Debug-redacted so diagnostics cannot log answers.
#[derive(Clone, Eq, PartialEq)]
pub struct OperatorAnswerEvidence {
    target: OperatorAnswerTarget,
    text: String,
    consumption: OperatorAnswerConsumption,
}

impl std::fmt::Debug for OperatorAnswerEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorAnswerEvidence")
            .field("decision_id", &self.target.decision_id)
            .field("consumption", &self.consumption)
            .finish_non_exhaustive()
    }
}

pub const MAX_OPERATOR_ANSWER_BYTES: usize = 16 * 1024;

impl OperatorAnswerEvidence {
    /// Constructs bounded evidence after the application verifies its origin.
    /// Returns None for empty or oversized answers; never trims or truncates.
    #[must_use]
    pub fn new(
        target: OperatorAnswerTarget,
        text: String,
        consumption: OperatorAnswerConsumption,
    ) -> Option<Self> {
        if text.trim().is_empty() || text.len() > MAX_OPERATOR_ANSWER_BYTES {
            return None;
        }
        Some(Self {
            target,
            text,
            consumption,
        })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// An existing answer is compared exactly, never semantically. A result of
    /// `ResolveConsumed` must be committed with its evidence in one transaction,
    /// without another answer delivery to the originating worker.
    #[must_use]
    pub fn correlate(
        &self,
        current: &OperatorAnswerTarget,
        existing_answer: Option<&str>,
    ) -> OperatorAnswerCorrelation {
        if &self.target != current {
            return OperatorAnswerCorrelation::DifferentQuestionOrSession;
        }
        if self.consumption != OperatorAnswerConsumption::Confirmed {
            return OperatorAnswerCorrelation::NotConfirmed;
        }
        match existing_answer {
            None => OperatorAnswerCorrelation::ResolveConsumed,
            Some(answer) if answer == self.text => OperatorAnswerCorrelation::AlreadyResolved,
            Some(_) => OperatorAnswerCorrelation::ConflictingAnswer,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorAnswerCorrelation {
    ResolveConsumed,
    AlreadyResolved,
    ConflictingAnswer,
    DifferentQuestionOrSession,
    NotConfirmed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> OperatorAnswerTarget {
        OperatorAnswerTarget {
            decision_id: DecisionRequestId::new(),
            worker_id: WorkerId::new(),
            session_id: WorkerSessionId::new(),
            question: DecisionQuestion {
                header: "Scope".into(),
                question: "Which scope?".into(),
                options: vec!["Narrow".into(), "Broad".into()],
                multi_select: false,
            },
        }
    }

    #[test]
    fn exact_answer_resolves_once_and_never_overwrites() {
        let target = target();
        let evidence = OperatorAnswerEvidence::new(
            target.clone(),
            " Narrow ".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        assert_eq!(evidence.text(), " Narrow ");
        assert_eq!(
            evidence.correlate(&target, None),
            OperatorAnswerCorrelation::ResolveConsumed
        );
        assert_eq!(
            evidence.correlate(&target, Some(" Narrow ")),
            OperatorAnswerCorrelation::AlreadyResolved
        );
        assert_eq!(
            evidence.correlate(&target, Some("Narrow")),
            OperatorAnswerCorrelation::ConflictingAnswer
        );
    }

    #[test]
    fn changed_identity_or_question_cannot_consume_an_answer() {
        let target = target();
        let evidence = OperatorAnswerEvidence::new(
            target.clone(),
            "Narrow".into(),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        let mut variants = vec![target.clone(); 7];
        variants[0].decision_id = DecisionRequestId::new();
        variants[1].worker_id = WorkerId::new();
        variants[2].session_id = WorkerSessionId::new();
        variants[3].question.header = "Other".into();
        variants[4].question.question = "Different?".into();
        variants[5].question.options.reverse();
        variants[6].question.multi_select = true;
        for changed in variants {
            assert_eq!(
                evidence.correlate(&changed, None),
                OperatorAnswerCorrelation::DifferentQuestionOrSession
            );
        }
    }

    #[test]
    fn unconfirmed_or_rejected_input_is_not_resolution_even_with_matching_text() {
        let target = target();
        for state in [
            OperatorAnswerConsumption::Unconfirmed,
            OperatorAnswerConsumption::Rejected,
        ] {
            let evidence =
                OperatorAnswerEvidence::new(target.clone(), "Narrow".into(), state).unwrap();
            assert_eq!(
                evidence.correlate(&target, None),
                OperatorAnswerCorrelation::NotConfirmed
            );
            assert_eq!(
                evidence.correlate(&target, Some("Narrow")),
                OperatorAnswerCorrelation::NotConfirmed
            );
        }
    }

    #[test]
    fn answers_are_byte_bounded_and_diagnostics_do_not_leak_content() {
        let target = target();
        for text in [
            String::new(),
            " \n\t".into(),
            "é".repeat(MAX_OPERATOR_ANSWER_BYTES / 2 + 1),
        ] {
            assert!(
                OperatorAnswerEvidence::new(
                    target.clone(),
                    text,
                    OperatorAnswerConsumption::Confirmed
                )
                .is_none()
            );
        }
        let evidence = OperatorAnswerEvidence::new(
            target.clone(),
            "s".repeat(MAX_OPERATOR_ANSWER_BYTES),
            OperatorAnswerConsumption::Confirmed,
        )
        .unwrap();
        assert_eq!(evidence.text().len(), MAX_OPERATOR_ANSWER_BYTES);
        let debug = format!("{evidence:?}");
        assert!(!debug.contains("ssss"));
        assert!(!debug.contains(&target.question.question));
    }
}
