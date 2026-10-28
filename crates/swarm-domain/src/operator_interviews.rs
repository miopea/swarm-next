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

    #[test]
    fn rendered_snapshots_reject_changed_conditions_and_preserve_plain_legacy_questions() {
        let mut unsupported = serde_json::to_value(question()).unwrap();
        unsupported["unrepresented_condition"] = serde_json::json!("Only after backup");
        assert!(serde_json::from_value::<crate::DecisionQuestion>(unsupported).is_err());
        let mut questions = vec![question()];
        assert!(crate::displayed_decision_questions_match(&questions, None));
        questions[0]
            .option_descriptions
            .insert("Narrow".into(), "No deployment.".into());
        assert!(!crate::displayed_decision_questions_match(&questions, None));
        assert!(crate::displayed_decision_questions_match(
            &questions,
            Some(&questions)
        ));
        assert!(!crate::displayed_decision_questions_match(
            &questions,
            Some(&[])
        ));
        for change in 0..6 {
            let mut stale = questions.clone();
            match change {
                0 => stale[0].option_descriptions.clear(),
                1 => {
                    stale[0]
                        .option_descriptions
                        .insert("Narrow".into(), "Deploy now.".into());
                }
                2 => stale[0].question = "Different?".into(),
                3 => stale[0].header = "Other".into(),
                4 => stale[0].options.reverse(),
                _ => stale[0].multi_select = true,
            }
            assert!(!crate::displayed_decision_questions_match(
                &questions,
                Some(&stale)
            ));
        }
        questions[0].options = vec!["Same".into(), "Same".into()];
        assert!(!crate::displayed_decision_questions_match(
            &questions,
            Some(&questions)
        ));
    }
}

/// Private IPC evidence, not an agent-writable operator receipt. The API must
/// authenticate its engine transport and separately bind an exact decision.
/// Source content must not enter ordinary session summaries or diagnostics.
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct NativeInterviewEvidence {
    /// Set only by the authenticated engine after exact final-result comparison.
    /// Absence on older stored sources means unchecked, never implicit success.
    /// This is provider evidence, not proof of human authorship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_result: Option<NativeInterviewFinalResult>,
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
    /// What the operator wrote beside the option they chose, by question. Kept
    /// apart from the answer, because a decision's answer is compared with its
    /// options. Absent from sources captured before notes were read.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NativeInterviewFinalResult {
    ExactBatch,
    /// The interview was asked and never came back as a completed answer.
    ///
    /// ⚠️ THIS IS NOT AN ANSWER AND MUST NEVER RESOLVE ANYTHING. It carries the
    /// questions that were asked so the operator can be told WHICH item is
    /// affected, and no answers, because none were captured. Only `ExactBatch`
    /// resolves a decision; every other value, including this one, refuses.
    NeverCompleted,
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
        let identified = self.selection_revision > 0
            && self.submit_sequence < u64::MAX
            && !self.tool_use_id.is_empty()
            && self.tool_use_id.len() <= 128
            && self
                .tool_use_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && self.devices.len() <= 4
            && self
                .devices
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.devices.len()
            && valid_native_interview_questions(&self.questions);
        // ⚠️ ASKED AND NEVER ANSWERED CARRIES NO ANSWER, AND MAY CARRY NO TYPING.
        // Requiring both here refused every one of these at intake, so the
        // notice it exists to raise never reached a card.
        if self.final_result == Some(NativeInterviewFinalResult::NeverCompleted) {
            return identified && self.answers.is_empty() && self.notes.is_empty();
        }
        identified
            && self.first_write_sequence > 0
            && self.submit_sequence >= self.first_write_sequence
            && !self.devices.is_empty()
            && self.answers.len() == self.questions.len()
            && self.questions.iter().all(|q| {
                self.answers
                    .get(&q.question)
                    .is_some_and(|a| !a.trim().is_empty() && a.len() <= MAX_OPERATOR_ANSWER_BYTES)
            })
            && self.notes.iter().all(|(question, note)| {
                self.answers.contains_key(question) && bounded(note, MAX_OPERATOR_ANSWER_BYTES)
            })
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;

    fn answered() -> NativeInterviewEvidence {
        NativeInterviewEvidence {
            final_result: Some(NativeInterviewFinalResult::ExactBatch),
            id: OperatorSubmissionId::new(),
            session_id: WorkerSessionId::new(),
            conversation: ProviderConversationId::new(),
            selection_revision: 1,
            tool_use_id: "toolu_fictional".into(),
            devices: vec![PresenceDeviceId::new()],
            first_write_sequence: 3,
            submit_sequence: 4,
            questions: vec![NativeInterviewQuestion {
                header: "Jar".into(),
                question: "Which jar?".into(),
                options: vec![
                    NativeInterviewOption {
                        label: "Amber".into(),
                        description: String::new(),
                    },
                    NativeInterviewOption {
                        label: "Blue".into(),
                        description: String::new(),
                    },
                ],
                multi_select: false,
            }],
            answers: [("Which jar?".into(), "Amber".into())].into(),
            notes: BTreeMap::new(),
        }
    }

    /// The engine builds this when an interview's final batch arrives with no
    /// answer before it: no answers, and no typing if none was recorded.
    #[test]
    fn an_interview_never_answered_is_admitted_without_answers_or_typing() {
        let mut never = answered();
        never.final_result = Some(NativeInterviewFinalResult::NeverCompleted);
        never.answers.clear();
        never.devices.clear();
        never.first_write_sequence = 5;
        never.submit_sequence = 0;
        assert!(never.is_valid());

        let mut claims_an_answer = never.clone();
        claims_an_answer
            .answers
            .insert("Which jar?".into(), "Amber".into());
        assert!(!claims_an_answer.is_valid());

        // An answered source still needs the typing that makes it one.
        let mut untyped = answered();
        untyped.devices.clear();
        assert!(!untyped.is_valid());
    }

    #[test]
    fn notes_belong_to_an_answered_question_and_say_something() {
        let mut noted = answered();
        noted
            .notes
            .insert("Which jar?".into(), "Only the small one".into());
        assert!(noted.is_valid());
        let older: NativeInterviewEvidence =
            serde_json::from_str(&serde_json::to_string(&answered()).unwrap()).unwrap();
        assert!(older.notes.is_empty());

        let mut stray = answered();
        stray.notes.insert("Which lid?".into(), "Red".into());
        assert!(!stray.is_valid());

        let mut blank = answered();
        blank.notes.insert("Which jar?".into(), "  ".into());
        assert!(!blank.is_valid());
    }
}
