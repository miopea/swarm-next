//! Content-minimizing provider hook boundary. Parsing is not authentication.
use serde::Deserialize;
use swarm_domain::{ProviderConversationId, ProviderSessionStartKind};

pub const MAX_PROVIDER_LIFECYCLE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderSessionStartObservation {
    pub conversation: ProviderConversationId,
    pub kind: ProviderSessionStartKind,
}

#[derive(Deserialize)]
struct ClaudeSessionStart<'a> {
    #[serde(borrow)]
    hook_event_name: &'a str,
    #[serde(borrow)]
    session_id: &'a str,
    #[serde(borrow)]
    source: &'a str,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
    // Provider metadata is intentionally not retained. This includes transcript
    // paths, cwd, model, title, cost and any future extension fields.
}

/// Projects one bounded Claude `SessionStart` payload onto provider-neutral facts.
/// The caller must independently authenticate and bind the originating process.
/// Malformed input returns no observation and never includes payloads in errors.
#[must_use]
pub fn read_claude_session_start(input: &[u8]) -> Option<ProviderSessionStartObservation> {
    if input.len() > MAX_PROVIDER_LIFECYCLE_BYTES {
        return None;
    }
    let parsed: ClaudeSessionStart<'_> = serde_json::from_slice(input).ok()?;
    if parsed.hook_event_name != "SessionStart"
        || parsed.agent_id.is_some()
        || parsed.session_id.len() != 36
        || parsed.session_id == "00000000-0000-0000-0000-000000000000"
    {
        return None;
    }
    let conversation = parsed.session_id.parse().ok()?;
    let kind = match parsed.source {
        "startup" => ProviderSessionStartKind::New,
        "resume" => ProviderSessionStartKind::Resumed,
        "clear" => ProviderSessionStartKind::Reset,
        "compact" => ProviderSessionStartKind::Compacted,
        "fork" => ProviderSessionStartKind::Forked,
        _ => ProviderSessionStartKind::Unknown,
    };
    Some(ProviderSessionStartObservation { conversation, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(source: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "00000000-0000-0000-0000-000000000001",
            "source": source,
            "transcript_path": "/private/transcript",
            "cwd": "/private/workspace",
            "session_title": "private title",
        }))
        .unwrap()
    }

    #[test]
    fn lifecycle_kinds_remain_distinct_without_provider_content() {
        for (source, expected) in [
            ("startup", ProviderSessionStartKind::New),
            ("resume", ProviderSessionStartKind::Resumed),
            ("clear", ProviderSessionStartKind::Reset),
            ("compact", ProviderSessionStartKind::Compacted),
            ("fork", ProviderSessionStartKind::Forked),
            ("future-event", ProviderSessionStartKind::Unknown),
        ] {
            let observation = read_claude_session_start(&payload(source)).unwrap();
            assert_eq!(observation.kind, expected);
            assert!(!format!("{observation:?}").contains("private"));
        }
    }

    #[test]
    fn malformed_oversized_and_wrong_event_inputs_produce_no_evidence() {
        assert!(read_claude_session_start(&vec![b' '; MAX_PROVIDER_LIFECYCLE_BYTES + 1]).is_none());
        assert!(read_claude_session_start(b"{not json}").is_none());
        let mut value: serde_json::Value = serde_json::from_slice(&payload("resume")).unwrap();
        for id in [
            "",
            "not-a-conversation",
            "00000000-0000-0000-0000-000000000000",
        ] {
            value["session_id"] = id.into();
            assert!(read_claude_session_start(&serde_json::to_vec(&value).unwrap()).is_none());
        }
        value = serde_json::from_slice(&payload("resume")).unwrap();
        value["agent_id"] = "child-agent".into();
        assert!(read_claude_session_start(&serde_json::to_vec(&value).unwrap()).is_none());
        value = serde_json::from_slice(&payload("resume")).unwrap();
        value["hook_event_name"] = "Stop".into();
        assert!(read_claude_session_start(&serde_json::to_vec(&value).unwrap()).is_none());
        assert!(read_claude_session_start(br#"{"hook_event_name":"SessionStart","session_id":"00000000-0000-0000-0000-000000000001","source":"resume","source":"startup"}"#).is_none());
    }
}
