//! Claude's own report of each turn, from the hooks Swarm installs (ADR 0123).
//!
//! The screen says what a terminal LOOKS like. These say what Claude DID: a
//! prompt was taken, the turn ended, the turn died on an API error, a question
//! or permission dialog is waiting, auto mode refused a tool. Parsing is not
//! authentication; the host accepts an observation only from the live
//! provider's capability.
use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use swarm_domain::ProviderConversationId;

/// The largest hook input the turn helper reads.
///
/// Larger than the lifecycle limit on purpose. A Stop carries Claude's whole
/// final message and a `UserPromptSubmit` the whole prompt, and losing the
/// event because a worker wrote a long answer would leave the turn open. The
/// helper forwards only the compact form this module writes, never the input.
pub const MAX_PROVIDER_TURN_INPUT_BYTES: usize = 8 * 1024 * 1024;

const MAX_DENIED_TOOL_CHARS: usize = 64;
const MAX_DENIAL_REASON_CHARS: usize = 160;
const MAX_TOOL_CALL_ID_CHARS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTurnPhase {
    /// A prompt was taken and no `Stop` or `StopFailure` has followed it.
    Working,
    /// The latest turn finished normally.
    Ended,
    /// The latest turn ended on an API error.
    Failed,
}

/// Why a turn ended on an API error, grouped by what would fix it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTurnFailure {
    RateLimit,
    Overloaded,
    Authentication,
    Billing,
    Server,
    Other,
}

/// What Claude is waiting on someone for, mid-turn or after it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTurnWait {
    /// An `AskUserQuestion` menu or an MCP elicitation dialog.
    Question,
    /// A permission dialog that has waited about six seconds.
    Permission,
    /// A usage limit reset and Claude did not carry on by itself.
    UsageLimit,
}

/// What a finished turn left running, by the kind Claude names.
///
/// Claude's `monitor` entries are not counted. Measured on 2.1.310 on
/// 2026-10-24: a Monitor tool watch is listed as a `shell`, and the `monitor`
/// entries belong to an MCP server. Live workers carried one to three of them
/// for minutes with no process behind them and nothing on Claude's own
/// footer, so counting them kept resting workers Buzzing. An engine from
/// before this sends a `monitors` count, which is ignored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderTurnBackground {
    pub shells: u16,
    pub workflows: u16,
    /// Subagents, teammates, cloud sessions, MCP tasks and kinds this build
    /// does not know by name.
    pub others: u16,
}

impl ProviderTurnBackground {
    #[must_use]
    pub const fn is_running(self) -> bool {
        self.shells > 0 || self.workflows > 0 || self.others > 0
    }
}

/// A tool call auto mode refused. Never the call's input, which can carry
/// command lines and file contents.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderDenial {
    pub tool: String,
    pub reason: String,
    /// The refused call's id, so the batch it belongs to is not read as Claude
    /// moving on. Kept by the host and never sent on: nothing else needs it.
    #[serde(skip)]
    pub call: Option<String>,
}

/// The turn state the host keeps per provider session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderTurn {
    /// Counts every accepted observation, so a reader can tell a new report
    /// from the one it already saw.
    pub sequence: u64,
    pub phase: ProviderTurnPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ProviderTurnFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<ProviderTurnWait>,
    /// From the latest Stop. Absent before one, and when Claude could not
    /// reach its task registry, so the screen is read instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<ProviderTurnBackground>,
    /// The latest refusal in the current turn, kept until Claude runs another
    /// tool after it or the next turn starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial: Option<ProviderDenial>,
}

impl Default for ProviderTurn {
    fn default() -> Self {
        Self {
            sequence: 0,
            phase: ProviderTurnPhase::Ended,
            failure: None,
            waiting: None,
            background: None,
            denial: None,
        }
    }
}

/// One hook, reduced to what it means for the turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderTurnEvent {
    /// `UserPromptSubmit`: a prompt was taken, typed or started by Claude itself.
    Submitted,
    /// `Stop`.
    Ended {
        background: Option<ProviderTurnBackground>,
    },
    /// `StopFailure`.
    Failed(ProviderTurnFailure),
    Waiting(ProviderTurnWait),
    /// An `AskUserQuestion` was answered.
    Resolved,
    /// `PostToolBatch`: every call in a batch resolved, named by id.
    BatchResolved {
        calls: Vec<String>,
    },
    /// `PermissionDenied`.
    Denied(ProviderDenial),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderTurnObservation {
    pub conversation: ProviderConversationId,
    pub event: ProviderTurnEvent,
    /// The hook also feeds native interview capture, which must still see it.
    pub interview: bool,
}

impl ProviderTurn {
    /// Applies one accepted observation.
    pub fn apply(&mut self, event: &ProviderTurnEvent) {
        self.sequence = self.sequence.saturating_add(1);
        match event {
            ProviderTurnEvent::Submitted => {
                // A message typed mid-turn is reported mid-turn, under the
                // running prompt, and must not wipe what that turn recorded.
                if self.phase != ProviderTurnPhase::Working {
                    self.failure = None;
                    self.waiting = None;
                    self.denial = None;
                }
                self.phase = ProviderTurnPhase::Working;
            }
            ProviderTurnEvent::Ended { background } => {
                self.phase = ProviderTurnPhase::Ended;
                self.failure = None;
                self.waiting = None;
                self.background = *background;
            }
            ProviderTurnEvent::Failed(failure) => {
                self.phase = ProviderTurnPhase::Failed;
                self.failure = Some(*failure);
                self.waiting = None;
            }
            ProviderTurnEvent::Waiting(wait) => self.waiting = Some(*wait),
            ProviderTurnEvent::Resolved => self.resolve_wait(),
            ProviderTurnEvent::BatchResolved { calls } => {
                self.resolve_wait();
                // Measured on 2.1.310: the refused call's own batch resolves
                // right after the refusal and names it. A batch without it
                // means Claude ran something else, so it has moved on. One it
                // stopped on stays through the Stop until the next prompt.
                if self
                    .denial
                    .as_ref()
                    .and_then(|denial| denial.call.as_ref())
                    .is_some_and(|call| !calls.contains(call))
                {
                    self.denial = None;
                }
            }
            ProviderTurnEvent::Denied(denial) => self.denial = Some(denial.clone()),
        }
    }

    fn resolve_wait(&mut self) {
        if matches!(
            self.waiting,
            Some(ProviderTurnWait::Question | ProviderTurnWait::Permission)
        ) {
            self.waiting = None;
        }
    }
}

#[derive(Deserialize)]
struct ClaudeHook<'a> {
    #[serde(borrow)]
    hook_event_name: Cow<'a, str>,
    #[serde(borrow)]
    session_id: Cow<'a, str>,
    #[serde(default)]
    agent_id: Option<serde::de::IgnoredAny>,
    #[serde(default, borrow)]
    tool_name: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    tool_use_id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    tool_calls: Option<Vec<ClaudeToolCall<'a>>>,
    #[serde(default, borrow)]
    notification_type: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    error: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    reason: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    background_tasks: Option<Vec<ClaudeBackgroundTask<'a>>>,
    // Prompts, messages, tool inputs and responses, paths and every future
    // field are skipped unread.
}

#[derive(Deserialize)]
struct ClaudeBackgroundTask<'a> {
    #[serde(borrow, rename = "type")]
    kind: Cow<'a, str>,
}

/// One call in a `PostToolBatch`. Its input and response are skipped unread.
#[derive(Deserialize)]
struct ClaudeToolCall<'a> {
    #[serde(default, borrow)]
    tool_use_id: Option<Cow<'a, str>>,
}

/// Reads one Claude hook payload for what it says about the turn.
///
/// Returns nothing for hooks that say nothing about it, for subagent hooks, and
/// for malformed input. Accepts both Claude's own payload and the compact form
/// [`compact_turn_payload`] writes from it.
#[must_use]
pub fn read_claude_turn_event(input: &[u8]) -> Option<ProviderTurnObservation> {
    if input.len() > MAX_PROVIDER_TURN_INPUT_BYTES {
        return None;
    }
    let hook: ClaudeHook<'_> = serde_json::from_slice(input).ok()?;
    if hook.agent_id.is_some() || hook.session_id.len() != 36 {
        return None;
    }
    let conversation = hook.session_id.parse().ok()?;
    let tool = hook.tool_name.as_deref();
    let (event, interview) = match hook.hook_event_name.as_ref() {
        "UserPromptSubmit" => (ProviderTurnEvent::Submitted, false),
        "Stop" => (
            ProviderTurnEvent::Ended {
                background: hook.background_tasks.as_deref().map(background),
            },
            false,
        ),
        "StopFailure" => (
            ProviderTurnEvent::Failed(failure(hook.error.as_deref().unwrap_or_default())),
            false,
        ),
        "Notification" => (
            ProviderTurnEvent::Waiting(match hook.notification_type.as_deref()? {
                "permission_prompt" => ProviderTurnWait::Permission,
                "elicitation_dialog" => ProviderTurnWait::Question,
                "quota_auto_resume_stale" | "quota_auto_resume_disabled" => {
                    ProviderTurnWait::UsageLimit
                }
                _ => return None,
            }),
            false,
        ),
        "PermissionDenied" => (
            ProviderTurnEvent::Denied(ProviderDenial {
                tool: bounded(tool?, MAX_DENIED_TOOL_CHARS),
                reason: bounded(
                    hook.reason.as_deref().unwrap_or_default(),
                    MAX_DENIAL_REASON_CHARS,
                ),
                call: hook.tool_use_id.as_deref().map(call_id),
            }),
            false,
        ),
        "PreToolUse" if tool == Some("AskUserQuestion") => {
            (ProviderTurnEvent::Waiting(ProviderTurnWait::Question), true)
        }
        "PostToolUse" if tool == Some("AskUserQuestion") => (ProviderTurnEvent::Resolved, true),
        "PostToolBatch" => (
            ProviderTurnEvent::BatchResolved {
                calls: hook
                    .tool_calls
                    .iter()
                    .flatten()
                    .filter_map(|call| call.tool_use_id.as_deref().map(call_id))
                    .collect(),
            },
            true,
        ),
        _ => return None,
    };
    Some(ProviderTurnObservation {
        conversation,
        event,
        interview,
    })
}

/// The smallest payload that reads back as the same observation.
///
/// The helper reads Claude's full input and forwards this instead, so a long
/// prompt or answer never meets the host's request limit and no prompt or
/// message text leaves the helper.
#[must_use]
pub fn compact_turn_payload(observation: &ProviderTurnObservation) -> Option<String> {
    let session_id = observation.conversation.to_string();
    let payload = match &observation.event {
        ProviderTurnEvent::Submitted => {
            serde_json::json!({"hook_event_name": "UserPromptSubmit", "session_id": session_id})
        }
        ProviderTurnEvent::Ended { background } => {
            let mut payload =
                serde_json::json!({"hook_event_name": "Stop", "session_id": session_id});
            if let Some(background) = background {
                let tasks = [
                    ("shell", background.shells),
                    ("workflow", background.workflows),
                    ("other", background.others),
                ]
                .into_iter()
                .flat_map(|(kind, count)| {
                    std::iter::repeat_n(serde_json::json!({"type": kind}), usize::from(count))
                })
                .collect::<Vec<_>>();
                payload["background_tasks"] = tasks.into();
            }
            payload
        }
        ProviderTurnEvent::Failed(failure) => serde_json::json!({
            "hook_event_name": "StopFailure",
            "session_id": session_id,
            "error": failure_name(*failure),
        }),
        ProviderTurnEvent::Waiting(wait) => serde_json::json!({
            "hook_event_name": "Notification",
            "session_id": session_id,
            "notification_type": match wait {
                ProviderTurnWait::Permission => "permission_prompt",
                ProviderTurnWait::Question => "elicitation_dialog",
                ProviderTurnWait::UsageLimit => "quota_auto_resume_disabled",
            },
        }),
        ProviderTurnEvent::Denied(denial) => serde_json::json!({
            "hook_event_name": "PermissionDenied",
            "session_id": session_id,
            "tool_name": denial.tool,
            "tool_use_id": denial.call,
            "reason": denial.reason,
        }),
        // These reach the host through the interview hook, unchanged.
        ProviderTurnEvent::Resolved | ProviderTurnEvent::BatchResolved { .. } => return None,
    };
    Some(payload.to_string())
}

fn background(tasks: &[ClaudeBackgroundTask<'_>]) -> ProviderTurnBackground {
    let mut counted = ProviderTurnBackground::default();
    // Every entry is in flight: Claude lists only tasks it is still running.
    for task in tasks {
        let slot = match task.kind.as_ref() {
            "shell" => &mut counted.shells,
            // Not work the operator can see; see `ProviderTurnBackground`.
            "monitor" => continue,
            "workflow" => &mut counted.workflows,
            _ => &mut counted.others,
        };
        *slot = slot.saturating_add(1);
    }
    counted
}

fn failure(error: &str) -> ProviderTurnFailure {
    match error {
        "rate_limit" => ProviderTurnFailure::RateLimit,
        "overloaded" => ProviderTurnFailure::Overloaded,
        "authentication_failed" | "oauth_org_not_allowed" | "cloud_credential_error" => {
            ProviderTurnFailure::Authentication
        }
        "billing_error" | "account_on_hold" => ProviderTurnFailure::Billing,
        "server_error" => ProviderTurnFailure::Server,
        _ => ProviderTurnFailure::Other,
    }
}

const fn failure_name(failure: ProviderTurnFailure) -> &'static str {
    match failure {
        ProviderTurnFailure::RateLimit => "rate_limit",
        ProviderTurnFailure::Overloaded => "overloaded",
        ProviderTurnFailure::Authentication => "authentication_failed",
        ProviderTurnFailure::Billing => "billing_error",
        ProviderTurnFailure::Server => "server_error",
        ProviderTurnFailure::Other => "unknown",
    }
}

/// Bounded the same way on both sides, so a refusal and its batch still match.
fn call_id(id: &str) -> String {
    bounded(id, MAX_TOOL_CALL_ID_CHARS)
}

fn bounded(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONVERSATION: &str = "00000000-0000-0000-0000-000000000001";

    fn read(value: &serde_json::Value) -> Option<ProviderTurnObservation> {
        read_claude_turn_event(&serde_json::to_vec(value).unwrap())
    }

    fn hook(name: &str) -> serde_json::Value {
        serde_json::json!({
            "hook_event_name": name,
            "session_id": CONVERSATION,
            "transcript_path": "/private/transcript.jsonl",
            "cwd": "/private/workspace",
            "prompt_id": "550e8400-e29b-41d4-a716-446655440000",
            "permission_mode": "auto",
        })
    }

    fn event(value: &serde_json::Value) -> Option<ProviderTurnEvent> {
        read(value).map(|observation| observation.event)
    }

    /// Shapes measured from Claude Code 2.1.310 on 2026-10-23, not the docs.
    #[test]
    fn each_turn_hook_reads_as_what_it_means() {
        let mut submit = hook("UserPromptSubmit");
        submit["prompt"] = "private prompt text".into();
        assert_eq!(event(&submit), Some(ProviderTurnEvent::Submitted));

        let mut stop = hook("Stop");
        stop["last_assistant_message"] = "private answer".into();
        stop["stop_hook_active"] = false.into();
        stop["session_crons"] = serde_json::json!([]);
        stop["background_tasks"] = serde_json::json!([
            {"id": "bt3oi8uod", "type": "shell", "status": "running", "description": "Sleep", "command": "sleep 25"},
            {"id": "b0shd59sm", "type": "shell", "status": "running", "description": "Sleep", "command": "sleep 12"},
            {"id": "m1", "type": "monitor", "status": "running", "server": "s", "tool": "t"},
            {"id": "w1", "type": "workflow", "status": "running", "name": "review"},
            {"id": "a1", "type": "subagent", "status": "running", "agent_type": "Explore"},
            {"id": "c1", "type": "cloud session", "status": "running"},
        ]);
        assert_eq!(
            event(&stop),
            Some(ProviderTurnEvent::Ended {
                background: Some(ProviderTurnBackground {
                    shells: 2,
                    workflows: 1,
                    others: 2,
                }),
            })
        );
        stop["background_tasks"] = serde_json::json!([]);
        assert_eq!(
            event(&stop),
            Some(ProviderTurnEvent::Ended {
                background: Some(ProviderTurnBackground::default()),
            })
        );
        // Absent when Claude cannot reach its task registry: unknown, not none.
        stop.as_object_mut().unwrap().remove("background_tasks");
        assert_eq!(
            event(&stop),
            Some(ProviderTurnEvent::Ended { background: None })
        );

        for (error, expected) in [
            ("rate_limit", ProviderTurnFailure::RateLimit),
            ("overloaded", ProviderTurnFailure::Overloaded),
            ("authentication_failed", ProviderTurnFailure::Authentication),
            ("oauth_org_not_allowed", ProviderTurnFailure::Authentication),
            (
                "cloud_credential_error",
                ProviderTurnFailure::Authentication,
            ),
            ("billing_error", ProviderTurnFailure::Billing),
            ("account_on_hold", ProviderTurnFailure::Billing),
            ("server_error", ProviderTurnFailure::Server),
            ("max_output_tokens", ProviderTurnFailure::Other),
            ("a_future_error", ProviderTurnFailure::Other),
        ] {
            let mut failed = hook("StopFailure");
            failed["error"] = error.into();
            failed["last_assistant_message"] = "API Error: private".into();
            assert_eq!(
                event(&failed),
                Some(ProviderTurnEvent::Failed(expected)),
                "{error}"
            );
        }
    }

    #[test]
    fn only_notifications_that_mean_waiting_are_read() {
        for (kind, expected) in [
            ("permission_prompt", Some(ProviderTurnWait::Permission)),
            ("elicitation_dialog", Some(ProviderTurnWait::Question)),
            (
                "quota_auto_resume_stale",
                Some(ProviderTurnWait::UsageLimit),
            ),
            (
                "quota_auto_resume_disabled",
                Some(ProviderTurnWait::UsageLimit),
            ),
            ("quota_auto_resume_fired", None),
            ("idle_prompt", None),
            ("agent_needs_input", None),
            ("auth_success", None),
        ] {
            let mut notification = hook("Notification");
            notification["notification_type"] = kind.into();
            notification["message"] = "private".into();
            assert_eq!(
                event(&notification),
                expected.map(ProviderTurnEvent::Waiting),
                "{kind}"
            );
        }
    }

    #[test]
    fn a_denial_keeps_the_tool_and_reason_and_never_the_input() {
        let mut denied = hook("PermissionDenied");
        denied["tool_name"] = "Bash".into();
        denied["tool_input"] = serde_json::json!({"command": "rm -rf /private/secret"});
        denied["tool_use_id"] = "toolu_1".into();
        denied["reason"] = "[Irreversible Local Destruction]".into();
        let observation = read(&denied).unwrap();
        assert_eq!(
            observation.event,
            ProviderTurnEvent::Denied(ProviderDenial {
                tool: "Bash".into(),
                reason: "[Irreversible Local Destruction]".into(),
                call: Some("toolu_1".into()),
            })
        );
        assert!(!format!("{observation:?}").contains("secret"));

        denied["tool_name"] = "x".repeat(500).into();
        denied["reason"] = format!("line\nbreak{}", "y".repeat(500)).into();
        let ProviderTurnEvent::Denied(bounded) = read(&denied).unwrap().event else {
            panic!("expected a denial");
        };
        assert_eq!(bounded.tool.chars().count(), MAX_DENIED_TOOL_CHARS);
        assert_eq!(bounded.reason.chars().count(), MAX_DENIAL_REASON_CHARS);
        assert!(!bounded.reason.contains('\n'));
    }

    #[test]
    fn questions_are_read_from_the_interview_hooks_and_still_go_to_capture() {
        let mut asked = hook("PreToolUse");
        asked["tool_name"] = "AskUserQuestion".into();
        let observation = read(&asked).unwrap();
        assert_eq!(
            observation.event,
            ProviderTurnEvent::Waiting(ProviderTurnWait::Question)
        );
        assert!(observation.interview);

        asked["hook_event_name"] = "PostToolUse".into();
        assert_eq!(read(&asked).unwrap().event, ProviderTurnEvent::Resolved);
        let batch = hook("PostToolBatch");
        assert_eq!(
            read(&batch).unwrap().event,
            ProviderTurnEvent::BatchResolved { calls: Vec::new() }
        );
        assert!(read(&batch).unwrap().interview);

        // Any other tool says nothing about the turn.
        asked["hook_event_name"] = "PreToolUse".into();
        asked["tool_name"] = "Bash".into();
        assert_eq!(read(&asked), None);
    }

    #[test]
    fn subagent_malformed_oversized_and_unrelated_hooks_produce_nothing() {
        let mut child = hook("Stop");
        child["agent_id"] = "a1".into();
        assert_eq!(read(&child), None);
        for name in [
            "SubagentStop",
            "SessionStart",
            "SessionEnd",
            "PreCompact",
            "PermissionRequest",
        ] {
            assert_eq!(read(&hook(name)), None, "{name}");
        }
        let mut wrong = hook("Stop");
        wrong["session_id"] = "not-a-conversation".into();
        assert_eq!(read(&wrong), None);
        assert_eq!(read_claude_turn_event(b"{not json"), None);
        assert_eq!(
            read_claude_turn_event(&vec![b' '; MAX_PROVIDER_TURN_INPUT_BYTES + 1]),
            None
        );
    }

    #[test]
    fn the_compact_payload_reads_back_as_the_same_observation_without_content() {
        let mut stop = hook("Stop");
        stop["last_assistant_message"] = "private answer".repeat(10_000).into();
        stop["background_tasks"] =
            serde_json::json!([{"type": "shell"}, {"type": "subagent"}, {"type": "workflow"}]);
        let mut denied = hook("PermissionDenied");
        denied["tool_name"] = "Bash".into();
        denied["tool_use_id"] = "toolu_01Q8hrpWNB".into();
        denied["reason"] = "[Data Exfiltration]".into();
        let mut failed = hook("StopFailure");
        failed["error"] = "overloaded".into();
        let mut waiting = hook("Notification");
        waiting["notification_type"] = "quota_auto_resume_stale".into();
        let mut submit = hook("UserPromptSubmit");
        submit["prompt"] = "private prompt".into();
        for original in [stop, denied, failed, waiting, submit] {
            let observation = read(&original).unwrap();
            let compact = compact_turn_payload(&observation).unwrap();
            assert!(compact.len() < 1024, "{compact}");
            assert!(!compact.contains("private"));
            assert_eq!(
                read_claude_turn_event(compact.as_bytes()),
                Some(observation)
            );
        }
        let mut stop = hook("Stop");
        stop.as_object_mut().unwrap().remove("background_tasks");
        let observation = read(&stop).unwrap();
        let compact = compact_turn_payload(&observation).unwrap();
        assert_eq!(
            read_claude_turn_event(compact.as_bytes()),
            Some(observation)
        );
    }

    /// Measured on live workers on 2026-10-24: Swarm Next listed one MCP
    /// `monitor` and Platform three, with neither a process nor a footer
    /// count behind them. A Monitor tool watch is listed as a shell.
    #[test]
    fn an_mcp_monitor_is_not_work_and_an_older_engine_s_count_is_ignored() {
        let mut stop = hook("Stop");
        stop["background_tasks"] = serde_json::json!([
            {"id": "m1", "type": "monitor", "status": "running", "server": "s", "tool": "t"},
            {"id": "m2", "type": "monitor", "status": "running", "server": "s", "tool": "u"},
        ]);
        let Some(ProviderTurnEvent::Ended {
            background: Some(left),
        }) = event(&stop)
        else {
            panic!("a Stop with a task list ends the turn with that list");
        };
        assert!(!left.is_running());

        stop["background_tasks"] = serde_json::json!([
            {"id": "bwthnd08i", "type": "shell", "status": "running",
             "description": "tick after 25 seconds", "command": "sleep 25; echo tick"},
        ]);
        assert_eq!(
            event(&stop),
            Some(ProviderTurnEvent::Ended {
                background: Some(ProviderTurnBackground {
                    shells: 1,
                    ..ProviderTurnBackground::default()
                }),
            })
        );

        // The engine running before this release reports them as a count.
        let older: ProviderTurn = serde_json::from_value(serde_json::json!({
            "sequence": 32,
            "phase": "ended",
            "background": {"shells": 0, "monitors": 3, "workflows": 0, "others": 0},
        }))
        .unwrap();
        assert!(!older.background.unwrap().is_running());
    }

    #[test]
    fn a_turn_opens_on_a_prompt_and_closes_on_its_end() {
        let mut turn = ProviderTurn::default();
        turn.apply(&ProviderTurnEvent::Submitted);
        assert_eq!(turn.phase, ProviderTurnPhase::Working);
        turn.apply(&ProviderTurnEvent::Waiting(ProviderTurnWait::Question));
        assert_eq!(turn.waiting, Some(ProviderTurnWait::Question));
        turn.apply(&ProviderTurnEvent::Resolved);
        assert_eq!(turn.waiting, None);
        let left = ProviderTurnBackground {
            shells: 1,
            ..ProviderTurnBackground::default()
        };
        turn.apply(&ProviderTurnEvent::Ended {
            background: Some(left),
        });
        assert_eq!(turn.phase, ProviderTurnPhase::Ended);
        assert_eq!(turn.background, Some(left));
        assert_eq!(turn.sequence, 4);
    }

    #[test]
    fn a_failure_is_kept_until_the_next_turn_and_a_usage_wait_survives_it() {
        let mut turn = ProviderTurn::default();
        turn.apply(&ProviderTurnEvent::Submitted);
        turn.apply(&ProviderTurnEvent::Failed(ProviderTurnFailure::RateLimit));
        assert_eq!(turn.phase, ProviderTurnPhase::Failed);
        turn.apply(&ProviderTurnEvent::Waiting(ProviderTurnWait::UsageLimit));
        // Resolving a tool batch is not answering a usage limit.
        turn.apply(&ProviderTurnEvent::Resolved);
        assert_eq!(turn.waiting, Some(ProviderTurnWait::UsageLimit));
        assert_eq!(turn.failure, Some(ProviderTurnFailure::RateLimit));
        turn.apply(&ProviderTurnEvent::Submitted);
        assert_eq!(turn.failure, None);
        assert_eq!(turn.waiting, None);
    }

    /// Measured: a message typed while Claude streams fires `UserPromptSubmit` at
    /// once, under the running prompt's id. It is part of the same turn.
    #[test]
    fn a_message_typed_mid_turn_keeps_what_the_turn_recorded() {
        let mut turn = ProviderTurn::default();
        let denial = ProviderDenial {
            tool: "Bash".into(),
            reason: "[Data Exfiltration]".into(),
            call: Some("toolu_1".into()),
        };
        turn.apply(&ProviderTurnEvent::Submitted);
        turn.apply(&ProviderTurnEvent::Denied(denial.clone()));
        turn.apply(&ProviderTurnEvent::Submitted);
        assert_eq!(turn.denial, Some(denial.clone()));
        turn.apply(&ProviderTurnEvent::Ended { background: None });
        assert_eq!(turn.denial, Some(denial), "kept after the turn ends");
        turn.apply(&ProviderTurnEvent::Submitted);
        assert_eq!(turn.denial, None, "a new turn starts clean");
    }

    /// Measured on 2.1.310 on 2026-10-25 with a session-only `hard_deny`
    /// rule: `PermissionDenied` names the refused call, and the batch it was
    /// in resolves straight after, naming it too. Platform carried on past a
    /// refusal for 45 minutes of one turn while the roster kept showing it.
    #[test]
    fn a_refusal_clears_once_claude_runs_something_else_and_stays_if_it_stopped() {
        let mut denied = hook("PermissionDenied");
        denied["tool_name"] = "Bash".into();
        denied["tool_input"] = serde_json::json!({"command": "echo swarmprobeblock"});
        denied["tool_use_id"] = "toolu_01Q8hrpWNB".into();
        denied["reason"] =
            "The server-side auto mode classifier judged this action dangerous (it gave no explanation)"
                .into();
        let batch = |id: &str, response: &str| {
            let mut batch = hook("PostToolBatch");
            batch["tool_calls"] = serde_json::json!([{
                "tool_name": "Bash",
                "tool_input": {"command": "private command"},
                "tool_use_id": id,
                "tool_response": response,
            }]);
            event(&batch).unwrap()
        };
        let refused = event(&denied).unwrap();
        let refused_batch = batch(
            "toolu_01Q8hrpWNB",
            "Permission for this action was denied by the Claude Code auto mode classifier.",
        );

        let mut carried_on = ProviderTurn::default();
        carried_on.apply(&ProviderTurnEvent::Submitted);
        carried_on.apply(&refused);
        carried_on.apply(&refused_batch);
        assert!(
            carried_on.denial.is_some(),
            "its own batch is not moving on"
        );
        carried_on.apply(&batch("toolu_01Vo3NLY2X", "carried-on"));
        assert_eq!(carried_on.denial, None);

        let mut stopped = ProviderTurn::default();
        stopped.apply(&ProviderTurnEvent::Submitted);
        stopped.apply(&refused);
        stopped.apply(&refused_batch);
        stopped.apply(&ProviderTurnEvent::Ended {
            background: Some(ProviderTurnBackground::default()),
        });
        assert!(stopped.denial.is_some(), "what stopped the worker is kept");

        // A refusal that names no call cannot be told from its own batch.
        let mut unnamed = ProviderTurn::default();
        denied.as_object_mut().unwrap().remove("tool_use_id");
        unnamed.apply(&event(&denied).unwrap());
        unnamed.apply(&batch("toolu_01Vo3NLY2X", "carried-on"));
        assert!(unnamed.denial.is_some());
    }
}
