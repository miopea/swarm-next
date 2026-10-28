//! Native question/result observations, never operator-authenticated receipts.
use std::collections::BTreeMap;

use serde::Deserialize;
use swarm_domain::{
    MAX_DECISION_QUESTIONS, MAX_OPERATOR_ANSWER_BYTES, ProviderConversationId,
    valid_native_interview_questions,
};
pub use swarm_domain::{NativeInterviewOption, NativeInterviewQuestion};

const MAX_TOOL_USE_ID_BYTES: usize = 128;

/// What Claude Code records as the answer when the operator wrote notes and
/// chose no option. The notes are then the answer.
const NOTES_ONLY: &str = "(notes only)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeInterviewPhase {
    Requested,
    Completed,
}

/// A successful native tool result establishes neither human authorship nor a
/// Swarm decision identity. In particular, other hooks can answer the tool.
/// Transport must authenticate the provider incarnation; the engine must prove
/// input provenance and the application must bind the exact pending decision.
#[derive(Clone, Eq, PartialEq)]
pub struct NativeInterviewObservation {
    pub conversation: ProviderConversationId,
    pub tool_use_id: String,
    pub phase: NativeInterviewPhase,
    questions: Vec<NativeInterviewQuestion>,
    answers: BTreeMap<String, String>,
    /// Notes the operator wrote, by question, as Claude Code reported them.
    notes: BTreeMap<String, String>,
}

impl NativeInterviewObservation {
    #[must_use]
    pub fn questions(&self) -> &[NativeInterviewQuestion] {
        &self.questions
    }

    #[must_use]
    pub fn answers(&self) -> &BTreeMap<String, String> {
        &self.answers
    }

    /// The answer to record for each question: the option chosen, or the
    /// operator's notes where they chose none.
    #[must_use]
    pub fn recorded_answers(&self) -> BTreeMap<String, String> {
        self.answers
            .iter()
            .map(|(question, answer)| {
                let recorded = match self.notes.get(question) {
                    Some(notes) if answer == NOTES_ONLY => notes,
                    _ => answer,
                };
                (question.clone(), recorded.clone())
            })
            .collect()
    }

    /// Notes written beside a chosen option. Notes that stand in for an answer
    /// are recorded as the answer instead, so they are not repeated here.
    #[must_use]
    pub fn recorded_notes(&self) -> BTreeMap<String, String> {
        self.notes
            .iter()
            .filter(|(question, _)| {
                self.answers
                    .get(*question)
                    .is_some_and(|answer| answer != NOTES_ONLY)
            })
            .map(|(question, notes)| (question.clone(), notes.clone()))
            .collect()
    }

    /// Compares provider invocation and the complete supported question shape,
    /// including option descriptions. This deliberately grants no authority.
    #[must_use]
    pub fn completes(&self, presented: &Self) -> bool {
        self.phase == NativeInterviewPhase::Completed
            && presented.phase == NativeInterviewPhase::Requested
            && self.conversation == presented.conversation
            && self.tool_use_id == presented.tool_use_id
            && self.questions == presented.questions
    }

    /// Compare already observed answers with the provider's
    /// final batch result. This is not human provenance or a decision receipt.
    /// A batch alone loses evidence of programmatic input, so it cannot create
    /// a completed observation. Ambiguous multi-answer text is unsupported.
    #[must_use]
    pub fn matches_final_batch(&self, bytes: &[u8]) -> bool {
        if self.phase != NativeInterviewPhase::Completed
            || self.questions.is_empty()
            || bytes.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES
        {
            return false;
        }
        let Ok(batch) = serde_json::from_slice::<FinalBatch>(bytes) else {
            return false;
        };
        if batch.hook_event_name != "PostToolBatch"
            || batch.agent_id.is_some()
            || batch.session_id.parse::<ProviderConversationId>().ok() != Some(self.conversation)
            || batch.tool_calls.len() > swarm_domain::MAX_NATIVE_INTERVIEW_BATCH
        {
            return false;
        }
        let mut matching = batch
            .tool_calls
            .iter()
            .filter(|call| call.invocation == self.tool_use_id);
        let Some(call) = matching.next() else {
            return false;
        };
        if matching.next().is_some() || call.name != "AskUserQuestion" {
            return false;
        }
        let Ok(input) = serde_json::from_value::<InterviewInput>(call.input.clone()) else {
            return false;
        };
        if input.questions != self.questions
            || !input.answers.is_empty()
            || !input.annotations.is_empty()
        {
            return false;
        }
        let mut clauses = Vec::with_capacity(self.questions.len());
        for item in &self.questions {
            let question = &item.question;
            let Some(answer) = self.answers.get(question) else {
                return false;
            };
            let notes = self.notes.get(question);
            // Unescaped separators cannot prove how text was distributed
            // between answers. These cases need a structured final result.
            if self.questions.len() > 1
                && std::iter::once(answer).chain(notes).any(|text| {
                    text.chars()
                        .any(|value| value == '"' || value == '\\' || value.is_control())
                })
            {
                return false;
            }
            // A quote can move the line between an answer and its notes:
            // `"Blue" notes: x"` is Blue noted `x"`, or `Blue" notes: x` alone.
            // Words with more than one reading confirm neither.
            if answer != NOTES_ONLY {
                let body = notes.map_or_else(
                    || format!("{answer}\""),
                    |notes| format!("{answer}\" notes: {notes}"),
                );
                if usize::from(body.ends_with('"')) + body.matches("\" notes: ").count() > 1 {
                    return false;
                }
            }
            clauses.push(match (answer == NOTES_ONLY, notes) {
                (true, Some(notes)) => {
                    format!("\"{question}\"=(no option selected) notes: {notes}")
                }
                (true, None) => return false,
                (false, Some(notes)) => format!("\"{question}\"=\"{answer}\" notes: {notes}"),
                (false, None) => format!("\"{question}\"=\"{answer}\""),
            });
        }
        let joined = clauses.join(", ");
        let Some(response) = call.response.as_str() else {
            return false;
        };
        ANSWERED_WORDINGS
            .iter()
            .any(|(lead, tail)| response == format!("{lead}{joined}{tail}"))
    }
}

/// Claude Code's two wordings of an answered interview, as the text before the
/// answers and the text after them. Each is still matched exactly.
///
/// Only the first was accepted, and the second is not rare: RCG Networks'
/// transcripts carry it from 2.1.257 to 2.1.311, interleaved with the first in
/// the same session. Every answer it carried was refused at finalize, which
/// records nothing, so the item stayed up with nothing to say. On 2026-10-27
/// that was an approval the operator had answered in the terminal.
const ANSWERED_WORDINGS: [(&str, &str); 2] = [
    (
        "Your questions have been answered: ",
        ". You can now continue with these answers in mind.",
    ),
    (
        "The user answered: ",
        ". Read the answers carefully — they may request clarification, changes, or that you not proceed — and follow what they actually say.",
    ),
];

#[derive(Deserialize)]
struct FinalBatch {
    hook_event_name: String,
    session_id: String,
    tool_calls: Vec<FinalBatchCall>,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
}

pub(crate) fn final_batch_conversation(bytes: &[u8]) -> Option<ProviderConversationId> {
    if bytes.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES {
        return None;
    }
    let batch: FinalBatch = serde_json::from_slice(bytes).ok()?;
    if batch.hook_event_name != "PostToolBatch"
        || batch.agent_id.is_some()
        || batch.tool_calls.len() > swarm_domain::MAX_NATIVE_INTERVIEW_BATCH
    {
        return None;
    }
    batch.session_id.parse().ok()
}

/// Whether a final batch is the completion of one specific invocation.
///
/// ⚠️ NARROW ON PURPOSE. `PostToolBatch` carries no tool matcher — a batch is not
/// one tool — so it fires after EVERY tool call and almost none of them are
/// interviews. This is the only thing that distinguishes "the interview we were
/// waiting for came back" from ordinary traffic, so it matches the
/// conversation AND the exact invocation, not merely the shape.
pub(crate) fn final_batch_answers_invocation(
    bytes: &[u8],
    conversation: ProviderConversationId,
    invocation: &str,
) -> bool {
    if bytes.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES {
        return false;
    }
    let Ok(batch) = serde_json::from_slice::<FinalBatch>(bytes) else {
        return false;
    };
    batch.hook_event_name == "PostToolBatch"
        && batch.agent_id.is_none()
        && batch.tool_calls.len() <= swarm_domain::MAX_NATIVE_INTERVIEW_BATCH
        && batch.session_id.parse::<ProviderConversationId>().ok() == Some(conversation)
        && batch
            .tool_calls
            .iter()
            .any(|call| call.invocation == invocation && call.name == "AskUserQuestion")
}

/// Whether a payload that could not be read as an interview is nonetheless the
/// answer to this invocation: a main-session `PostToolUse` for `AskUserQuestion`
/// naming it. Anything else is other traffic — a subagent's batch reaches the
/// same hook — and must not be taken for a refused answer.
pub(crate) fn completion_names_invocation(
    bytes: &[u8],
    conversation: ProviderConversationId,
    invocation: &str,
) -> bool {
    #[derive(Deserialize)]
    struct Named {
        hook_event_name: String,
        session_id: String,
        tool_name: String,
        tool_use_id: String,
        #[serde(default)]
        agent_id: Option<serde::de::IgnoredAny>,
    }
    if bytes.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES {
        return false;
    }
    serde_json::from_slice::<Named>(bytes).is_ok_and(|named| {
        named.hook_event_name == "PostToolUse"
            && named.agent_id.is_none()
            && named.tool_name == "AskUserQuestion"
            && named.tool_use_id == invocation
            && named.session_id.parse::<ProviderConversationId>().ok() == Some(conversation)
    })
}

#[derive(Deserialize)]
struct FinalBatchCall {
    #[serde(rename = "tool_use_id")]
    invocation: String,
    #[serde(rename = "tool_name")]
    name: String,
    #[serde(rename = "tool_input")]
    input: serde_json::Value,
    #[serde(rename = "tool_response")]
    response: serde_json::Value,
}

impl std::fmt::Debug for NativeInterviewObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeInterviewObservation")
            .field("conversation", &self.conversation)
            .field("phase", &self.phase)
            .field("question_count", &self.questions.len())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InterviewInput {
    questions: Vec<NativeInterviewQuestion>,
    #[serde(default, deserialize_with = "unique_answers")]
    answers: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, InterviewAnnotation>,
}

/// What the operator added to one answer. Only notes are read: a preview
/// annotation belongs to an option preview, which questions here cannot carry,
/// and anything newer fails closed rather than disappearing from the record.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InterviewAnnotation {
    notes: String,
}

fn unique_answers<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Answers;
    impl<'de> serde::de::Visitor<'de> for Answers {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded map of unique question answers")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut answers = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if answers.len() == MAX_DECISION_QUESTIONS || answers.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "ambiguous or oversized answer map",
                    ));
                }
            }
            Ok(answers)
        }
    }
    deserializer.deserialize_map(Answers)
}

#[derive(Deserialize)]
struct InterviewHook {
    hook_event_name: String,
    session_id: String,
    tool_use_id: String,
    tool_name: String,
    tool_input: InterviewInput,
    #[serde(default)]
    tool_response: Option<InterviewInput>,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
}

/// Main-session `AskUserQuestion` only. Unsupported/partial/oversized evidence
/// remains unavailable; it is never converted into a guessed answer.
#[must_use]
pub fn read_claude_interview(input: &[u8]) -> Option<NativeInterviewObservation> {
    if input.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES {
        return None;
    }
    let hook: InterviewHook = match serde_json::from_slice::<InterviewHook>(input) {
        Ok(hook) => hook,
        Err(error) => {
            tracing::warn!(%error, "interview payload rejected: not parseable as a hook envelope");
            return None;
        }
    };
    // ⚠️ NAMES THE FIELD, because "not a readable Claude interview" is nine
    // different bugs wearing one message. This rejects a payload the provider
    // just sent, so the interesting case is always DRIFT — a field the provider
    // changed shape on between versions — and a reader who cannot see which
    // field has to diff the whole envelope against a fixture to guess.
    let rejection = if hook.tool_name != "AskUserQuestion" {
        Some("tool_name is not AskUserQuestion")
    } else if hook.agent_id.is_some() {
        Some("agent_id present, so this is a subagent rather than the main session")
    } else if hook.session_id.len() != 36 {
        Some("session_id is not 36 characters")
    } else if hook.session_id == "00000000-0000-0000-0000-000000000000" {
        Some("session_id is the nil uuid")
    } else if hook.tool_use_id.is_empty() {
        Some("tool_use_id is empty")
    } else if hook.tool_use_id.len() > MAX_TOOL_USE_ID_BYTES {
        Some("tool_use_id is too long")
    } else if !hook
        .tool_use_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Some("tool_use_id contains characters outside [A-Za-z0-9_]")
    } else if hook.hook_event_name == "PreToolUse" && !hook.tool_input.answers.is_empty() {
        // ⚠️ THE PROGRAMMATIC MARKER, AND IT ONLY MEANS ANYTHING BEFORE THE
        // HUMAN ANSWERS. At PreToolUse nothing has been answered yet, so answers
        // present in the INPUT can only have been supplied by the caller — the
        // forged-consent shape that claude-2.1.267-programmatic-interview.json
        // records, where a programmatic invocation pre-filled "Amber".
        //
        // At PostToolUse it means the opposite. Claude Code 2.1.270 echoes the
        // answers the operator just gave back into tool_input, so refusing them
        // there rejected EVERY genuine interview. Measured 2026-09-12: that
        // rejection also calls invalidate_pending, destroying the observation
        // PreToolUse had already stored, so one refusal took out all three hooks
        // and native_operator_interviews sat at 0 for the life of the feature.
        //
        // Provenance is not weakened by this, because it never rested here.
        // NativeInterviewCapture::complete refuses without a pending entry, and
        // only a clean Requested observation creates one; it then additionally
        // requires device writes, a submit sequence and matching write
        // sequences — evidence a person typed. A programmatic completion with
        // no prior clean request still produces nothing.
        Some("tool_input.answers is not empty on a request, which marks a programmatic pre-fill")
    } else if hook.hook_event_name == "PreToolUse" && !hook.tool_input.annotations.is_empty() {
        // Same timing argument: annotations are collected from the person at
        // answer time, so before the answer they can only be caller-supplied.
        Some("tool_input.annotations is not empty on a request")
    } else if !valid_native_interview_questions(&hook.tool_input.questions) {
        Some("tool_input.questions failed validation")
    } else {
        None
    };
    if let Some(reason) = rejection {
        tracing::warn!(
            reason,
            event = %hook.hook_event_name,
            tool = %hook.tool_name,
            questions = hook.tool_input.questions.len(),
            "interview payload rejected"
        );
        return None;
    }
    let (phase, answers, notes) = match hook.hook_event_name.as_str() {
        "PreToolUse" if hook.tool_response.is_none() => (
            NativeInterviewPhase::Requested,
            BTreeMap::new(),
            BTreeMap::new(),
        ),
        "PostToolUse" => {
            let response = hook.tool_response?;
            let notes = response
                .annotations
                .into_iter()
                .map(|(question, annotation)| (question, annotation.notes))
                .collect::<BTreeMap<_, _>>();
            if response.questions != hook.tool_input.questions
                || response.answers.len() != response.questions.len()
                || !response.questions.iter().all(|question| {
                    response
                        .answers
                        .get(&question.question)
                        .is_some_and(|answer| {
                            !answer.trim().is_empty() && answer.len() <= MAX_OPERATOR_ANSWER_BYTES
                        })
                        && answered_with_notes(
                            question,
                            &response.answers[&question.question],
                            notes.get(&question.question),
                        )
                })
                || !notes
                    .keys()
                    .all(|question| response.answers.contains_key(question))
            {
                return None;
            }
            (NativeInterviewPhase::Completed, response.answers, notes)
        }
        _ => return None,
    };
    Some(NativeInterviewObservation {
        conversation: hook.session_id.parse().ok()?,
        tool_use_id: hook.tool_use_id,
        phase,
        questions: hook.tool_input.questions,
        answers,
        notes,
    })
}

/// Whether an answer and its notes are a shape Swarm can record without
/// guessing. Notes must say something. "(notes only)" is Claude's word for
/// notes with no option chosen, so it needs notes, and it cannot be read that
/// way when an option carries that very label.
fn answered_with_notes(
    question: &NativeInterviewQuestion,
    answer: &str,
    notes: Option<&String>,
) -> bool {
    if notes.is_some_and(|notes| notes.trim().is_empty() || notes.len() > MAX_OPERATOR_ANSWER_BYTES)
    {
        return false;
    }
    answer != NOTES_ONLY
        || (notes.is_some()
            && !question
                .options
                .iter()
                .any(|option| option.label == NOTES_ONLY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn payload(completed: bool) -> Value {
        let questions = json!([{
            "question":"Which fictional fruit?", "header":"Orchard",
            "options":[{"label":"Apple","description":"Keep the red fruit."},
                {"label":"Pear","description":"Keep the green fruit."}], "multiSelect":false
        }, {
            "question":"Which fictional jar?", "header":"Honey",
            "options":[{"label":"Amber"},{"label":"Blue"}], "multiSelect":false
        }]);
        let mut value = json!({"hook_event_name":if completed {"PostToolUse"} else {"PreToolUse"},
            "session_id":"00000000-0000-0000-0000-000000000001", "tool_use_id":"toolu_example",
            "tool_name":"AskUserQuestion", "tool_input":{"questions":questions},
            "cwd":"/private/workspace", "transcript_path":"/private/transcript"});
        if completed {
            value["tool_response"] = json!({"questions":questions,
                "answers":{"Which fictional fruit?":"Pear", "Which fictional jar?":"  Something else 🐝\nKeep this exact. "},
                "annotations":{}});
        }
        value
    }

    fn read(value: &Value) -> Option<NativeInterviewObservation> {
        read_claude_interview(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn final_batch_compares_results_but_cannot_create_operator_evidence() {
        let bytes = include_bytes!("../fixtures/claude-2.1.267-final-interview-batch.json");
        let batch: Value = serde_json::from_slice(bytes).unwrap();
        assert!(read_claude_interview(bytes).is_none());
        // Synthetic observations isolate the comparison; the captured batch
        // came from programmatic hooks and is never human acceptance evidence.
        let mut event = json!({
            "hook_event_name":"PostToolUse", "session_id":batch["session_id"],
            "tool_use_id":batch["tool_calls"][0]["tool_use_id"], "tool_name":"AskUserQuestion",
            "tool_input":batch["tool_calls"][0]["tool_input"],
            "tool_response":{"questions":batch["tool_calls"][0]["tool_input"]["questions"],
                "answers":{"Which fictional jar?":"Amber"}}
        });
        assert!(!read(&event).unwrap().matches_final_batch(bytes));
        event["tool_response"]["answers"]["Which fictional jar?"] = json!("Blue");
        let completed = read(&event).unwrap();
        assert!(completed.matches_final_batch(bytes));
        for change in 0..7 {
            let mut changed = batch.clone();
            match change {
                0 => changed["session_id"] = json!("00000000-0000-0000-0000-000000000001"),
                1 => changed["tool_calls"][0]["tool_use_id"] = json!("toolu_other"),
                2 => {
                    changed["tool_calls"][0]["tool_input"]["questions"][0]["options"][0]["description"] =
                        json!("Changed scope");
                }
                3 => {
                    let duplicate = changed["tool_calls"][0].clone();
                    changed["tool_calls"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                4 => changed["agent_id"] = json!("child"),
                5 => {
                    changed["tool_calls"][0]["tool_input"]["answers"] =
                        json!({"Which fictional jar?":"Blue"});
                }
                _ => changed["tool_calls"][0]["tool_response"] = json!("Blue"),
            }
            assert!(!completed.matches_final_batch(&serde_json::to_vec(&changed).unwrap()));
        }
        assert!(
            !completed.matches_final_batch(&vec![b' '; crate::MAX_PROVIDER_LIFECYCLE_BYTES + 1])
        );
        event["hook_event_name"] = json!("PreToolUse");
        event.as_object_mut().unwrap().remove("tool_response");
        assert!(!read(&event).unwrap().matches_final_batch(bytes));
    }

    /// The second wording completes an interview as the first does, and is
    /// matched just as exactly: a different answer, the other wording's tail,
    /// or a cut-off tail is still not a match.
    ///
    /// ABLATION: drop the second entry of `ANSWERED_WORDINGS` and the first
    /// assertion fails; match on the lead alone and the loop fails.
    #[test]
    fn both_wordings_of_an_answered_interview_complete_it_and_nothing_looser_does() {
        let bytes = include_bytes!("../fixtures/claude-2.1.267-final-interview-batch.json");
        let batch: Value = serde_json::from_slice(bytes).unwrap();
        let completed = read(&json!({
            "hook_event_name":"PostToolUse", "session_id":batch["session_id"],
            "tool_use_id":batch["tool_calls"][0]["tool_use_id"], "tool_name":"AskUserQuestion",
            "tool_input":batch["tool_calls"][0]["tool_input"],
            "tool_response":{"questions":batch["tool_calls"][0]["tool_input"]["questions"],
                "answers":{"Which fictional jar?":"Blue"}}
        }))
        .unwrap();
        let worded = |response: &str| {
            let mut changed = batch.clone();
            changed["tool_calls"][0]["tool_response"] = json!(response);
            serde_json::to_vec(&changed).unwrap()
        };
        let careful = ". Read the answers carefully — they may request clarification, changes, or that you not proceed — and follow what they actually say.";

        assert!(completed.matches_final_batch(&worded(&format!(
            "The user answered: \"Which fictional jar?\"=\"Blue\"{careful}"
        ))));
        for response in [
            format!("The user answered: \"Which fictional jar?\"=\"Amber\"{careful}"),
            "The user answered: \"Which fictional jar?\"=\"Blue\". You can now continue with these answers in mind.".to_owned(),
            "The user answered: \"Which fictional jar?\"=\"Blue\".".to_owned(),
            format!("Your questions have been answered: \"Which fictional jar?\"=\"Blue\"{careful}"),
        ] {
            assert!(
                !completed.matches_final_batch(&worded(&response)),
                "{response}"
            );
        }
    }

    #[test]
    fn a_quote_that_moves_the_line_between_answer_and_notes_confirms_neither() {
        let bytes = include_bytes!("../fixtures/claude-2.1.267-final-interview-batch.json");
        let batch: Value = serde_json::from_slice(bytes).unwrap();
        let answered = |answer: &str, annotations: Value| {
            read(&json!({
                "hook_event_name":"PostToolUse", "session_id":batch["session_id"],
                "tool_use_id":batch["tool_calls"][0]["tool_use_id"], "tool_name":"AskUserQuestion",
                "tool_input":batch["tool_calls"][0]["tool_input"],
                "tool_response":{"questions":batch["tool_calls"][0]["tool_input"]["questions"],
                    "answers":{"Which fictional jar?":answer}, "annotations":annotations}
            }))
            .unwrap()
        };
        let worded = |clause: &str| {
            let mut changed = batch.clone();
            changed["tool_calls"][0]["tool_response"] = json!(format!(
                "The user answered: \"Which fictional jar?\"={clause}. Read the answers carefully — they may request clarification, changes, or that you not proceed — and follow what they actually say."
            ));
            serde_json::to_vec(&changed).unwrap()
        };

        // Blue noted `the "small" one"`, or the answer `Blue" notes: the "small" one`:
        // the same words, so neither reading is taken.
        let both = r#""Blue" notes: the "small" one""#;
        for observed in [
            answered(
                "Blue",
                json!({"Which fictional jar?":{"notes":"the \"small\" one\""}}),
            ),
            answered("Blue\" notes: the \"small\" one", json!({})),
        ] {
            assert!(!observed.matches_final_batch(&worded(both)));
        }

        // A quote that leaves one reading is kept.
        let inner = answered(
            "Blue",
            json!({"Which fictional jar?":{"notes":"the \"small\" one"}}),
        );
        assert!(inner.matches_final_batch(&worded(r#""Blue" notes: the "small" one"#)));
    }

    /// Notes beside an answer, in the shapes Claude Code wrote into this Hive's
    /// transcripts (2.1.250 to 2.1.302, read 2026-10-27): after the chosen option, or in its
    /// place. The answer recorded is still the option; the notes travel apart.
    #[test]
    fn notes_beside_an_answer_complete_it_and_are_kept_apart_from_it() {
        let bytes = include_bytes!("../fixtures/claude-2.1.267-final-interview-batch.json");
        let batch: Value = serde_json::from_slice(bytes).unwrap();
        let answered = |answer: &str, annotations: Value| {
            read(&json!({
                "hook_event_name":"PostToolUse", "session_id":batch["session_id"],
                "tool_use_id":batch["tool_calls"][0]["tool_use_id"], "tool_name":"AskUserQuestion",
                "tool_input":batch["tool_calls"][0]["tool_input"],
                "tool_response":{"questions":batch["tool_calls"][0]["tool_input"]["questions"],
                    "answers":{"Which fictional jar?":answer}, "annotations":annotations}
            }))
        };
        let worded = |response: String| {
            let mut changed = batch.clone();
            changed["tool_calls"][0]["tool_response"] = json!(response);
            serde_json::to_vec(&changed).unwrap()
        };
        let careful = ". Read the answers carefully — they may request clarification, changes, or that you not proceed — and follow what they actually say.";

        let noted = answered(
            "Blue",
            json!({"Which fictional jar?":{"notes":"only the small one, after lunch"}}),
        )
        .unwrap();
        assert_eq!(noted.recorded_answers()["Which fictional jar?"], "Blue");
        assert_eq!(
            noted.recorded_notes()["Which fictional jar?"],
            "only the small one, after lunch"
        );
        assert!(noted.matches_final_batch(&worded(format!(
            "The user answered: \"Which fictional jar?\"=\"Blue\" notes: only the small one, after lunch{careful}"
        ))));
        for response in [
            format!("The user answered: \"Which fictional jar?\"=\"Blue\"{careful}"),
            format!(
                "The user answered: \"Which fictional jar?\"=\"Blue\" notes: only the small one{careful}"
            ),
            format!(
                "The user answered: \"Which fictional jar?\"=(no option selected) notes: only the small one, after lunch{careful}"
            ),
        ] {
            assert!(
                !noted.matches_final_batch(&worded(response.clone())),
                "{response}"
            );
        }

        // Notes and no option: the notes are the answer, and are not repeated.
        let notes_only = answered(
            "(notes only)",
            json!({"Which fictional jar?":{"notes":"neither, leave the shelf alone"}}),
        )
        .unwrap();
        assert_eq!(
            notes_only.recorded_answers()["Which fictional jar?"],
            "neither, leave the shelf alone"
        );
        assert!(notes_only.recorded_notes().is_empty());
        assert!(notes_only.matches_final_batch(&worded(format!(
            "The user answered: \"Which fictional jar?\"=(no option selected) notes: neither, leave the shelf alone{careful}"
        ))));

        // What cannot be read without guessing is refused.
        assert!(answered("(notes only)", json!({})).is_none());
        assert!(answered("Blue", json!({"Which fictional jar?":{"notes":"  "}})).is_none());
        assert!(answered("Blue", json!({"Which fictional lid?":{"notes":"red"}})).is_none());
        assert!(
            answered(
                "Blue",
                json!({"Which fictional jar?":{"preview":"Blue jar"}})
            )
            .is_none()
        );
    }

    /// Several answers with notes match only while no note could carry a
    /// separator, the same rule answers already follow.
    #[test]
    fn notes_across_several_questions_match_only_when_unambiguous() {
        let mut event = payload(true);
        event["tool_response"]["answers"] =
            json!({"Which fictional fruit?":"Pear", "Which fictional jar?":"(notes only)"});
        event["tool_response"]["annotations"] = json!({
            "Which fictional fruit?":{"notes":"ripe ones"},
            "Which fictional jar?":{"notes":"go ahead and create, we need an IT vault"}});
        let completed = read(&event).unwrap();
        let input = event["tool_input"].clone();
        let batch = |response: &str| {
            serde_json::to_vec(&json!({"hook_event_name":"PostToolBatch",
                "session_id":"00000000-0000-0000-0000-000000000001",
                "tool_calls":[{"tool_name":"AskUserQuestion", "tool_use_id":"toolu_example",
                    "tool_input":input, "tool_response":response}]}))
            .unwrap()
        };
        assert!(completed.matches_final_batch(&batch(
            "Your questions have been answered: \"Which fictional fruit?\"=\"Pear\" notes: ripe ones, \"Which fictional jar?\"=(no option selected) notes: go ahead and create, we need an IT vault. You can now continue with these answers in mind."
        )));

        event["tool_response"]["annotations"]["Which fictional fruit?"]["notes"] =
            json!("ripe ones\", \"Which fictional jar?\"=\"Amber");
        let quoted = read(&event).unwrap();
        assert!(!quoted.matches_final_batch(&batch(
            "Your questions have been answered: \"Which fictional fruit?\"=\"Pear\" notes: ripe ones\", \"Which fictional jar?\"=\"Amber, \"Which fictional jar?\"=(no option selected) notes: go ahead and create, we need an IT vault. You can now continue with these answers in mind."
        )));
    }

    #[test]
    fn three_question_final_batch_preserves_order_and_refuses_ambiguous_text() {
        let bytes = include_bytes!("../fixtures/claude-2.1.267-three-question-final-batch.json");
        let batch: Value = serde_json::from_slice(bytes).unwrap();
        let questions = batch["tool_calls"][0]["tool_input"]["questions"].clone();
        let mut event = json!({
            "hook_event_name":"PostToolUse", "session_id":batch["session_id"],
            "tool_use_id":batch["tool_calls"][0]["tool_use_id"], "tool_name":"AskUserQuestion",
            "tool_input":{"questions":questions},
            "tool_response":{"questions":questions, "answers":{
                "Which fictional jar?":"Blue", "Which fictional shelf?":"Blue",
                "Which fictional tag?":"Blue"}}
        });
        // Synthetic completion isolates comparison, not human authorship.
        assert!(read(&event).unwrap().matches_final_batch(bytes));
        event["tool_response"]["answers"]["Which fictional shelf?"] = json!("Amber");
        assert!(!read(&event).unwrap().matches_final_batch(bytes));
        event["tool_response"]["answers"]["Which fictional shelf?"] = json!("Blue");
        let mut reordered = batch.clone();
        reordered["tool_calls"][0]["tool_input"]["questions"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(
            !read(&event)
                .unwrap()
                .matches_final_batch(&serde_json::to_vec(&reordered).unwrap())
        );
        for answer in [
            "Blue\", \"Which fictional shelf?\"=\"Amber",
            "Blue\nAmber",
            "Blue\\Amber",
        ] {
            event["tool_response"]["answers"]["Which fictional jar?"] = json!(answer);
            let mut ambiguous = batch.clone();
            ambiguous["tool_calls"][0]["tool_response"] = json!(format!(
                "Your questions have been answered: \"Which fictional jar?\"=\"{answer}\", \"Which fictional shelf?\"=\"Blue\", \"Which fictional tag?\"=\"Blue\". You can now continue with these answers in mind."
            ));
            assert!(
                !read(&event)
                    .unwrap()
                    .matches_final_batch(&serde_json::to_vec(&ambiguous).unwrap())
            );
        }
        assert!(read_claude_interview(bytes).is_none());
    }

    #[test]
    fn exact_native_interview_preserves_free_text_and_redacts_debug() {
        let begin = read(&payload(false)).unwrap();
        let end = read(&payload(true)).unwrap();
        assert!(end.completes(&begin));
        assert_eq!(
            end.answers()["Which fictional jar?"],
            "  Something else 🐝\nKeep this exact. "
        );
        assert_eq!(
            end.questions()[0].options[1].description,
            "Keep the green fruit."
        );
        let debug = format!("{end:?}");
        for private in [
            "Pear",
            "fruit",
            "private",
            "Something else",
            "toolu_example",
        ] {
            assert!(!debug.contains(private));
        }
    }

    #[test]
    fn unrelated_failed_child_and_programmatic_answers_are_not_observations() {
        for (key, value) in [
            ("hook_event_name", json!("PostToolUseFailure")),
            ("tool_name", json!("Bash")),
            ("agent_id", json!("child")),
            ("session_id", json!("00000000-0000-0000-0000-000000000000")),
            ("tool_use_id", json!("private\ntext")),
        ] {
            let mut event = payload(true);
            event[key] = value;
            assert!(read(&event).is_none());
        }
        // ⚠️ MOVED TO THE REQUEST PHASE, WHERE THE MARKER STILL MEANS SOMETHING.
        // On a completion, Claude Code 2.1.270 echoes the operator's answers
        // into tool_input, so refusing them there rejected every genuine
        // interview. Before the answer exists, they can only be caller-supplied.
        let mut event = payload(false);
        event["tool_input"]["answers"] = json!({"Which fictional fruit?":"Pear"});
        assert!(
            read(&event).is_none(),
            "a request carrying answers is a programmatic pre-fill"
        );
        let mut event = payload(true);
        event["tool_input"]["answers"] = json!({"Which fictional fruit?":"Pear"});
        assert!(
            read(&event).is_some(),
            "a completion carrying answers is the 2.1.270 human shape"
        );
        assert!(read_claude_interview(b"not json").is_none());
    }

    #[test]
    fn installed_provider_programmatic_callback_cannot_become_operator_evidence() {
        // Actual reduced callback from the disposable Claude 2.1.267 PTY probe.
        // PreToolUse supplied Amber; PostToolUse rewrote the model result to Blue.
        // This observer still saw Amber. Neither result was a human answer.
        let bytes = include_bytes!("../fixtures/claude-2.1.267-programmatic-interview.json");
        let callback: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            callback["tool_input"]["answers"]["Which fictional jar?"],
            "Amber"
        );
        assert_eq!(
            callback["tool_response"]["answers"]["Which fictional jar?"],
            "Amber"
        );
        // ⚠️ THE PROXY MOVED; THE GUARANTEE DID NOT. This used to assert that
        // read_claude_interview refuses the completion. It no longer can: a
        // programmatic completion and a genuine 2.1.270 one are byte-identical
        // in shape — both carry the answers in tool_input AND tool_response —
        // so the payload layer cannot tell them apart and never really could.
        //
        // What actually stops forged consent is the CAPTURE: complete() refuses
        // without a pending entry, and only a clean Requested observation makes
        // one. So assert that instead, which is what this test is named for and
        // is strictly stronger than the shape check it replaces.
        let completion = read_claude_interview(bytes).expect("2.1.270 shape now reads");
        assert_eq!(completion.phase, NativeInterviewPhase::Completed);
        let mut capture = crate::native_interview_capture::NativeInterviewCapture::default();
        let session = swarm_domain::WorkerSessionId::new();
        assert!(
            !capture.observe_at_revision(session, 1, completion),
            "a completion with no clean request behind it must not become evidence"
        );
        assert!(
            capture.retained().is_empty(),
            "no operator evidence may exist from a programmatic callback"
        );

        // Reconstruct only the observed pre-hook invocation; it is a request,
        // not an answer. Removing the programmatic marker from a completion
        // would discard provenance rather than prove operator authorship.
        let mut requested = callback;
        requested["hook_event_name"] = json!("PreToolUse");
        requested.as_object_mut().unwrap().remove("tool_response");
        requested["tool_input"]
            .as_object_mut()
            .unwrap()
            .remove("answers");
        let observation = read(&requested).unwrap();
        assert_eq!(observation.phase, NativeInterviewPhase::Requested);
        assert!(observation.answers().is_empty());
    }

    #[test]
    fn missing_extra_or_ambiguous_answers_do_not_complete_an_interview() {
        let mut event = payload(true);
        event["tool_response"]["answers"]
            .as_object_mut()
            .unwrap()
            .remove("Which fictional jar?");
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["tool_response"]["answers"]["unknown"] = json!("Apple");
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["tool_input"]["questions"][1]["question"] = json!("Which fictional fruit?");
        assert!(read(&event).is_none());
        let serialized = serde_json::to_string(&payload(true)).unwrap().replace(
            "\"Which fictional fruit?\":\"Pear\"",
            "\"Which fictional fruit?\":\"Apple\",\"Which fictional fruit?\":\"Pear\"",
        );
        assert!(read_claude_interview(serialized.as_bytes()).is_none());
    }

    #[test]
    fn changed_context_cannot_match_an_earlier_presentation() {
        let begin = read(&payload(false)).unwrap();
        for (key, value) in [
            ("tool_use_id", json!("toolu_other")),
            ("session_id", json!("00000000-0000-0000-0000-000000000002")),
        ] {
            let mut event = payload(true);
            event[key] = value;
            assert!(!read(&event).unwrap().completes(&begin));
        }
        let mut event = payload(true);
        event["tool_input"]["questions"][0]["options"][1]["description"] =
            json!("Also deploy everything.");
        assert!(read(&event).is_none());
        event["tool_response"]["questions"] = event["tool_input"]["questions"].clone();
        assert!(!read(&event).unwrap().completes(&begin));
    }

    #[test]
    fn payload_text_and_unrepresented_question_features_fail_closed() {
        let mut event = payload(true);
        event["tool_response"]["answers"]["Which fictional fruit?"] =
            json!("x".repeat(MAX_OPERATOR_ANSWER_BYTES + 1));
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["tool_input"]["questions"][0]["options"][0]["preview"] = json!("Different meaning");
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["irrelevant"] = json!("x".repeat(crate::MAX_PROVIDER_LIFECYCLE_BYTES));
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["tool_response"]["annotations"] =
            json!({"Which fictional fruit?":"Only after review"});
        assert!(read(&event).is_none());
        let mut event = payload(true);
        event["tool_response"]["unknown_answer_context"] = json!("Only after review");
        assert!(read(&event).is_none());
    }

    #[test]
    fn all_three_answers_and_multiselect_text_survive_without_guessing_options() {
        let mut event = payload(true);
        let mut third = event["tool_input"]["questions"][1].clone();
        third["question"] = json!("Which fictional flowers?");
        third["header"] = json!("Meadow");
        third["multiSelect"] = json!(true);
        event["tool_input"]["questions"]
            .as_array_mut()
            .unwrap()
            .push(third);
        event["tool_response"]["questions"] = event["tool_input"]["questions"].clone();
        event["tool_response"]["answers"]["Which fictional flowers?"] = json!("Clover, Lavender");
        let completed = read(&event).unwrap();
        assert_eq!(completed.answers().len(), 3);
        assert_eq!(
            completed.answers()["Which fictional flowers?"],
            "Clover, Lavender"
        );
        assert!(completed.questions()[2].multi_select);
        event["tool_response"]["answers"]["Which fictional flowers?"] = json!(" \n\t");
        assert!(read(&event).is_none());
    }
}
