//! Native question/result observations, never operator-authenticated receipts.
use std::collections::BTreeMap;

use serde::Deserialize;
use swarm_domain::{
    MAX_DECISION_QUESTIONS, MAX_OPERATOR_ANSWER_BYTES, ProviderConversationId,
    valid_native_interview_questions,
};
pub use swarm_domain::{NativeInterviewOption, NativeInterviewQuestion};

const MAX_TOOL_USE_ID_BYTES: usize = 128;

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
            // Unescaped separators cannot prove how text was distributed
            // between answers. These cases need a structured final result.
            if self.questions.len() > 1
                && answer
                    .chars()
                    .any(|value| value == '"' || value == '\\' || value.is_control())
            {
                return false;
            }
            clauses.push(format!("\"{question}\"=\"{answer}\""));
        }
        let joined = clauses.join(", ");
        let expected = format!(
            "Your questions have been answered: {joined}. You can now continue with these answers in mind."
        );
        call.response.as_str() == Some(expected.as_str())
    }
}

#[derive(Deserialize)]
struct FinalBatch {
    hook_event_name: String,
    session_id: String,
    tool_calls: Vec<FinalBatchCall>,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
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
    annotations: BTreeMap<String, serde_json::Value>,
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
    let hook: InterviewHook = serde_json::from_slice(input).ok()?;
    if hook.tool_name != "AskUserQuestion"
        || hook.agent_id.is_some()
        || hook.session_id.len() != 36
        || hook.session_id == "00000000-0000-0000-0000-000000000000"
        || hook.tool_use_id.is_empty()
        || hook.tool_use_id.len() > MAX_TOOL_USE_ID_BYTES
        || !hook
            .tool_use_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || !hook.tool_input.answers.is_empty()
        || !hook.tool_input.annotations.is_empty()
        || !valid_native_interview_questions(&hook.tool_input.questions)
    {
        return None;
    }
    let (phase, answers) = match hook.hook_event_name.as_str() {
        "PreToolUse" if hook.tool_response.is_none() => {
            (NativeInterviewPhase::Requested, BTreeMap::new())
        }
        "PostToolUse" => {
            let response = hook.tool_response?;
            if response.questions != hook.tool_input.questions
                || !response.annotations.is_empty()
                || response.answers.len() != response.questions.len()
                || !response.questions.iter().all(|question| {
                    response
                        .answers
                        .get(&question.question)
                        .is_some_and(|answer| {
                            !answer.trim().is_empty() && answer.len() <= MAX_OPERATOR_ANSWER_BYTES
                        })
                })
            {
                return None;
            }
            (NativeInterviewPhase::Completed, response.answers)
        }
        _ => return None,
    };
    Some(NativeInterviewObservation {
        conversation: hook.session_id.parse().ok()?,
        tool_use_id: hook.tool_use_id,
        phase,
        questions: hook.tool_input.questions,
        answers,
    })
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
        let mut event = payload(true);
        event["tool_input"]["answers"] = json!({"Which fictional fruit?":"Pear"});
        assert!(read(&event).is_none());
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
        assert!(read_claude_interview(bytes).is_none());

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
