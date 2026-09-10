//! Private captured source; authentication and decision settlement are separate.
use crate::{
    MAX_DECISION_QUESTION_HEADER_BYTES, MAX_DECISION_QUESTION_OPTION_BYTES,
    MAX_DECISION_QUESTION_OPTIONS, MAX_DECISION_QUESTION_TEXT_BYTES, MAX_DECISION_QUESTIONS,
    MAX_OPERATOR_ANSWER_BYTES, OperatorSubmissionId, PresenceDeviceId, ProviderConversationId,
    WorkerSessionId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_OPTION_DESCRIPTION_BYTES: usize = crate::MAX_DECISION_OPTION_DESCRIPTION_BYTES;
pub const MAX_NATIVE_INTERVIEW_SOURCE_BYTES: usize = 128 * 1024;
pub const MAX_NATIVE_INTERVIEW_BATCH: usize = 32;

// Reject unrepresented question/option fields: previews and future provider
// behavior must not silently disappear from an exact-question comparison.
#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NativeInterviewOption {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NativeInterviewQuestion {
    pub question: String,
    pub header: String,
    pub options: Vec<NativeInterviewOption>,
    #[serde(default, rename = "multiSelect")]
    pub multi_select: bool,
}

impl NativeInterviewQuestion {
    /// Lossless question-shape conversion, not decision identity or provenance.
    /// Consumers must still bind the full decision ID and authenticated source.
    #[must_use]
    pub fn from_decision(question: &crate::DecisionQuestion) -> Option<Self> {
        if !crate::valid_decision_questions(std::slice::from_ref(question)) {
            return None;
        }
        Some(Self {
            question: question.question.clone(),
            header: question.header.clone(),
            options: question
                .options
                .iter()
                .map(|label| NativeInterviewOption {
                    label: label.clone(),
                    description: question
                        .option_descriptions
                        .get(label)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
            multi_select: question.multi_select,
        })
    }
}

#[cfg(test)]
mod description_tests {
    use super::*;

    fn question() -> crate::DecisionQuestion {
        serde_json::from_str(
            r#"{"header":"Scope","question":"Which scope?","options":["Narrow","Broad"]}"#,
        )
        .unwrap()
    }

    #[test]
    fn old_questions_remain_readable_and_description_conversion_is_lossless() {
        let mut question = question();
        assert!(question.option_descriptions.is_empty());
        assert!(
            serde_json::to_value(&question)
                .unwrap()
                .get("option_descriptions")
                .is_none()
        );
        question
            .option_descriptions
            .insert("Narrow".into(), " Only this repo.\nNo deployment. ".into());
        let native = NativeInterviewQuestion::from_decision(&question).unwrap();
        assert_eq!(
            native.options[0].description,
            " Only this repo.\nNo deployment. "
        );
        assert!(native.options[1].description.is_empty());
        let roundtrip = serde_json::from_str(&serde_json::to_string(&question).unwrap()).unwrap();
        assert_eq!(question, roundtrip);
    }

    #[test]
    fn descriptions_cannot_change_or_disappear_in_an_exact_match() {
        let mut question = question();
        let bare = NativeInterviewQuestion::from_decision(&question).unwrap();
        question
            .option_descriptions
            .insert("Narrow".into(), "Only after approval".into());
        assert!(bare != NativeInterviewQuestion::from_decision(&question).unwrap());
        question
            .option_descriptions
            .insert("Unknown".into(), "Condition".into());
        assert!(NativeInterviewQuestion::from_decision(&question).is_none());
        question.option_descriptions.remove("Unknown");
        question.option_descriptions.insert(
            "Narrow".into(),
            "é".repeat(MAX_OPTION_DESCRIPTION_BYTES / 2 + 1),
        );
        assert!(NativeInterviewQuestion::from_decision(&question).is_none());
    }
}

/// Private IPC evidence, not an agent-writable operator receipt. The API must
/// authenticate its engine transport and separately bind an exact decision.
/// Source content must not enter ordinary session summaries or diagnostics.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct NativeInterviewEvidence {
    pub id: OperatorSubmissionId,
    pub session_id: WorkerSessionId,
    pub conversation: ProviderConversationId,
    pub selection_revision: u64,
    pub tool_use_id: String,
    pub devices: Vec<PresenceDeviceId>,
    pub first_write_sequence: u64,
    pub submit_sequence: u64,
    pub questions: Vec<NativeInterviewQuestion>,
    pub answers: BTreeMap<String, String>,
}

impl std::fmt::Debug for NativeInterviewEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeInterviewEvidence")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

#[must_use]
pub fn valid_native_interview_questions(questions: &[NativeInterviewQuestion]) -> bool {
    !questions.is_empty()
        && questions.len() <= MAX_DECISION_QUESTIONS
        && questions.iter().enumerate().all(|(index, question)| {
            bounded(&question.header, MAX_DECISION_QUESTION_HEADER_BYTES)
                && bounded(&question.question, MAX_DECISION_QUESTION_TEXT_BYTES)
                && (2..=MAX_DECISION_QUESTION_OPTIONS).contains(&question.options.len())
                && !questions[..index].iter().any(|previous| {
                    previous.header == question.header || previous.question == question.question
                })
                && question.options.iter().enumerate().all(|(index, option)| {
                    bounded(&option.label, MAX_DECISION_QUESTION_OPTION_BYTES)
                        && option.description.len() <= MAX_OPTION_DESCRIPTION_BYTES
                        && !question.options[..index]
                            .iter()
                            .any(|previous| previous.label == option.label)
                })
        })
}

fn bounded(text: &str, maximum: usize) -> bool {
    !text.trim().is_empty() && text.len() <= maximum
}

impl NativeInterviewEvidence {
    /// Structural integrity is not proof of human authorship or permission.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.selection_revision > 0
            && self.first_write_sequence > 0
            && self.submit_sequence >= self.first_write_sequence
            && self.submit_sequence < u64::MAX
            && !self.tool_use_id.is_empty()
            && self.tool_use_id.len() <= 128
            && self
                .tool_use_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && (1..=4).contains(&self.devices.len())
            && self
                .devices
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.devices.len()
            && valid_native_interview_questions(&self.questions)
            && self.answers.len() == self.questions.len()
            && self.questions.iter().all(|q| {
                self.answers
                    .get(&q.question)
                    .is_some_and(|a| !a.trim().is_empty() && a.len() <= MAX_OPERATOR_ANSWER_BYTES)
            })
    }
}
