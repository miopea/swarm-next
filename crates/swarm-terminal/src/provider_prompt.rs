//! A provider prompt hook supplies text, not human provenance or consumption.
use std::borrow::Cow;

use serde::Deserialize;
use swarm_domain::{MAX_OPERATOR_ANSWER_BYTES, ProviderConversationId};

/// Not an operator receipt. Other hooks may still reject this submission, and
/// Swarm automation also submits prompts through the same provider interface.
/// No Serialize/Deserialize implementation: transport must preserve this trust
/// distinction explicitly rather than accepting an agent's asserted observation.
#[derive(Clone, Eq, PartialEq)]
pub struct ProviderPromptObservation {
    pub conversation: ProviderConversationId,
    text: String,
}

impl ProviderPromptObservation {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Debug for ProviderPromptObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderPromptObservation")
            .field("conversation", &self.conversation)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct ClaudePrompt<'a> {
    #[serde(borrow)]
    hook_event_name: &'a str,
    #[serde(borrow)]
    session_id: &'a str,
    #[serde(borrow)]
    prompt: Cow<'a, str>,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
}

/// Parses a bounded main-session prompt observation without transcript/path data.
/// Malformed, oversized, child-agent and unrelated events return no observation.
/// The caller must authenticate the process and separately establish origin and
/// consumption before constructing any operator evidence or resolving a decision.
#[must_use]
pub fn read_claude_prompt_submission(input: &[u8]) -> Option<ProviderPromptObservation> {
    if input.len() > crate::MAX_PROVIDER_LIFECYCLE_BYTES {
        return None;
    }
    let parsed: ClaudePrompt<'_> = serde_json::from_slice(input).ok()?;
    if parsed.hook_event_name != "UserPromptSubmit"
        || parsed.agent_id.is_some()
        || parsed.session_id.len() != 36
        || parsed.session_id == "00000000-0000-0000-0000-000000000000"
        || parsed.prompt.trim().is_empty()
        || parsed.prompt.len() > MAX_OPERATOR_ANSWER_BYTES
    {
        return None;
    }
    Some(ProviderPromptObservation {
        conversation: parsed.session_id.parse().ok()?,
        text: parsed.prompt.into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(text: &str) -> serde_json::Value {
        serde_json::json!({"hook_event_name":"UserPromptSubmit", "session_id":"00000000-0000-0000-0000-000000000001", "prompt":text, "transcript_path":"/private/transcript", "cwd":"/private/workspace"})
    }

    fn read(value: &serde_json::Value) -> Option<ProviderPromptObservation> {
        read_claude_prompt_submission(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn preserves_exact_multiline_unicode_and_quoted_material_without_debug_leaks() {
        let text = "  Keep scope narrow.\nQuoted: \"deploy everything\" 🐝\t";
        let observation = read(&payload(text)).unwrap();
        assert_eq!(observation.text(), text);
        let debug = format!("{observation:?}");
        assert!(!debug.contains("deploy"));
        assert!(!debug.contains("private"));
    }

    #[test]
    fn unrelated_events_children_invalid_ids_and_bad_shapes_are_rejected() {
        for (field, replacement) in [
            ("hook_event_name", serde_json::json!("PostToolUse")),
            ("agent_id", serde_json::json!("child")),
            (
                "session_id",
                serde_json::json!("00000000-0000-0000-0000-000000000000"),
            ),
            ("session_id", serde_json::json!("short")),
            ("prompt", serde_json::json!(["text"])),
            ("prompt", serde_json::json!(" \n\t")),
        ] {
            let mut value = payload("answer");
            value[field] = replacement;
            assert!(read(&value).is_none());
        }
        assert!(read_claude_prompt_submission(b"not json").is_none());
    }

    #[test]
    fn both_decoded_text_and_transport_have_hard_bounds() {
        assert!(read(&payload(&"x".repeat(MAX_OPERATOR_ANSWER_BYTES))).is_some());
        assert!(read(&payload(&"é".repeat(MAX_OPERATOR_ANSWER_BYTES / 2 + 1))).is_none());
        let mut value = payload("short");
        value["irrelevant"] = "x".repeat(crate::MAX_PROVIDER_LIFECYCLE_BYTES).into();
        assert!(read(&value).is_none());
    }
}
