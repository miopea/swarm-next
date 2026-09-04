#[cfg(unix)]
use std::path::Path;
use std::{env, path::PathBuf};

use serde::{Deserialize, Serialize};
use swarm_domain::{
    FederationStewardTakeoverLeaseId, PresenceDeviceId, ProviderKind, TerminalControlIdentity,
    WorkerSessionId,
};
use thiserror::Error;
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::{
    ClaudeConversationStart, CodexConversationStart, HistoryCursor, HistoryDiagnostics,
    HistoryPage, HistorySessionSummary, ProcessResourceSample, Resume, TerminalSize,
};

// Protocol 11 adds generation-bound control and control-aware output waits.
// build-release.sh records this number and reconcile_host refuses an unsafe
// engine swap across a protocol change. Older engines must be detected before
// sending the new requests, never worked around with unrestricted legacy input.
//
// Safe to bump mid-flight: reconcile_host drains with $host_release/bin/swarmctl,
// the binary from the release the RUNNING host came from, so the drain stays
// version-matched to itself and a newer checkout cannot break the update that
// carries it.
pub const PROTOCOL_VERSION: u16 = 11;
pub const TERMINAL_CONTROL_PROTOCOL_VERSION: u16 = 11;
pub const MAX_CONTROL_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_REQUEST_BYTES: u64 = 256 * 1024;
pub const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_WRITE_AUDIT_PAGE: u16 = 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalWriteActor {
    Operator {
        #[serde(default)]
        device_id: Option<PresenceDeviceId>,
    },
    SwarmCoordination,
    Steward {
        lease_id: FederationStewardTakeoverLeaseId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalInputKind {
    Text,
    Paste,
    Submit,
    Interrupt,
    Navigation,
    Control,
    Coordination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalWriteProvenance {
    pub actor: TerminalWriteActor,
    pub input_kind: TerminalInputKind,
}

impl TerminalWriteProvenance {
    #[must_use]
    pub fn operator(device_id: Option<PresenceDeviceId>, bytes: &[u8]) -> Self {
        Self {
            actor: TerminalWriteActor::Operator { device_id },
            input_kind: classify_input(bytes),
        }
    }

    #[must_use]
    pub const fn coordination() -> Self {
        Self {
            actor: TerminalWriteActor::SwarmCoordination,
            input_kind: TerminalInputKind::Coordination,
        }
    }

    #[must_use]
    pub fn steward(lease_id: FederationStewardTakeoverLeaseId, bytes: &[u8]) -> Self {
        Self {
            actor: TerminalWriteActor::Steward { lease_id },
            input_kind: classify_input(bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalWriteResult {
    Acknowledged,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalWriteAuditEntry {
    pub sequence: u64,
    pub session_id: WorkerSessionId,
    pub actor: TerminalWriteActor,
    pub input_kind: TerminalInputKind,
    pub byte_count: u32,
    pub result: TerminalWriteResult,
    pub occurred_at: i64,
}

fn classify_input(bytes: &[u8]) -> TerminalInputKind {
    if bytes == [3] {
        TerminalInputKind::Interrupt
    } else if matches!(bytes, [b'\r' | b'\n'] | [b'\r', b'\n']) {
        TerminalInputKind::Submit
    } else if bytes.starts_with(b"\x1b[200~") || bytes.windows(6).any(|part| part == b"\x1b[201~") {
        TerminalInputKind::Paste
    } else if bytes.starts_with(b"\x1b[") || matches!(bytes, [b'\t' | b'\x1b']) {
        TerminalInputKind::Navigation
    } else if bytes.iter().any(u8::is_ascii_control) {
        TerminalInputKind::Control
    } else {
        TerminalInputKind::Text
    }
}

#[must_use]
pub fn default_terminal_socket_path() -> PathBuf {
    let home = env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local/state/swarm/run/terminal.sock")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    Ping,
    HostStatus,
    ProviderCapabilities,
    BeginDrain,
    CancelDrain,
    StartClaude {
        workspace: PathBuf,
        size: TerminalSize,
        conversation: ClaudeConversationStart,
        mcp_config: Option<PathBuf>,
        #[serde(default)]
        allow_outside_roots: bool,
    },
    StartCodex {
        workspace: PathBuf,
        size: TerminalSize,
        conversation: CodexConversationStart,
        /// The per-worker MCP config, so a Codex session can reach the board.
        ///
        /// A Codex worker received its assignment notification and had no swarm
        /// tools, so it could not open the task, could not read the brief, and
        /// could not move it. It stopped rather than guessing, which was right
        /// and which left the work sitting with nothing saying anything was
        /// wrong. The reported workaround was putting the entire brief in the
        /// task TITLE, because the notification was all it could see.
        ///
        /// ⚠️ AN OLDER HOST IGNORES THIS AND THAT IS DELIBERATELY SURVIVABLE.
        /// `#[serde(default)]` means it is tolerated, and tolerated means
        /// dropped — so a Codex worker on a host that predates this starts with
        /// no tools, WHICH IS EXACTLY WHAT IT DOES TODAY. The skew reproduces
        /// the current behaviour rather than a new failure, and the API says so
        /// out loud rather than assuming the host took it. That is the shape
        /// operator decision 01a05b83 asked for: "Proceed with a loud warning
        /// that says it could not check."
        #[serde(default)]
        mcp_config: Option<PathBuf>,
        #[serde(default)]
        allow_outside_roots: bool,
    },
    /// A scratch shell in a workspace, deliberately NOT a worker session.
    ///
    /// It carries no conversation and no MCP configuration because there is no
    /// agent to configure. Nothing binds the returned session to a worker, so
    /// the roster never shows it and provider activity never classifies it.
    StartShell {
        workspace: PathBuf,
        size: TerminalSize,
        #[serde(default)]
        allow_outside_roots: bool,
    },
    /// An ALPHA provider: gemini, grok or opencode.
    ///
    /// One variant for all three rather than one each, because they are started
    /// identically -- bare CLI in the workspace, no conversation and no MCP
    /// configuration. Their resume contracts and configuration flags are not
    /// known here and would be guesses; when one is learned it earns its own
    /// variant and a protocol bump, which is the same cost as adding it now and
    /// carries evidence instead of assumption.
    ///
    /// Refuses `ClaudeCode`, Codex and Unsupported at the host, so this cannot
    /// become a second way to start a first-class provider.
    StartAlphaProvider {
        provider: ProviderKind,
        workspace: PathBuf,
        size: TerminalSize,
        #[serde(default)]
        allow_outside_roots: bool,
    },
    ListSessions,
    HistoryDiagnostics,
    ListHistorySessions,
    ReadHistory {
        session_id: WorkerSessionId,
        cursor: Option<HistoryCursor>,
    },
    Read {
        session_id: WorkerSessionId,
        after_sequence: Option<u64>,
    },
    Wait {
        session_id: WorkerSessionId,
        after_sequence: Option<u64>,
    },
    Write {
        session_id: WorkerSessionId,
        bytes: Vec<u8>,
        provenance: TerminalWriteProvenance,
    },
    WriteAudit {
        limit: u16,
    },
    InstallTakeover {
        session_id: WorkerSessionId,
        lease: TerminalTakeoverLease,
    },
    TakeoverWrite {
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        bytes: Vec<u8>,
    },
    ReclaimTakeoverAndWrite {
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        bytes: Vec<u8>,
    },
    ReleaseTakeover {
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
    },
    Resize {
        session_id: WorkerSessionId,
        size: TerminalSize,
    },
    Control {
        session_id: WorkerSessionId,
        command: TerminalControlCommand,
    },
    WaitControlled {
        session_id: WorkerSessionId,
        after_sequence: Option<u64>,
        after_control: TerminalControlCursor,
    },
    Stop {
        session_id: WorkerSessionId,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalControlCommand {
    Status,
    Claim {
        identity: TerminalControlIdentity,
        observed_generation: Option<u64>,
        size: TerminalSize,
    },
    Renew {
        identity: TerminalControlIdentity,
        generation: u64,
    },
    Release {
        identity: TerminalControlIdentity,
        generation: u64,
    },
    Input {
        identity: TerminalControlIdentity,
        generation: u64,
        bytes: Vec<u8>,
    },
    Resize {
        identity: TerminalControlIdentity,
        generation: u64,
        size: TerminalSize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalControlCursor {
    pub generation: u64,
    pub occupied: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalControlStatus {
    pub generation: u64,
    pub owner: Option<TerminalControlIdentity>,
    /// Remaining duration, not the engine's private monotonic clock epoch.
    pub lease_remaining_ms: u64,
}

impl TerminalControlStatus {
    #[must_use]
    pub const fn cursor(self) -> TerminalControlCursor {
        TerminalControlCursor {
            generation: self.generation,
            occupied: self.owner.is_some(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostSessionSummary {
    pub session_id: WorkerSessionId,
    pub running: bool,
    #[serde(default)]
    pub resources: Option<ProcessResourceSample>,
    /// Wall-clock second of this terminal's most recent output. Absent from a
    /// host that predates the field, so a rolling update stays compatible.
    #[serde(default)]
    pub last_output_at: Option<i64>,
    /// Startup attempt only. Missing on older hosts; never implies restoration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_attempt: Option<swarm_domain::ConversationRecoveryAttempt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalTakeoverLease {
    pub lease_id: FederationStewardTakeoverLeaseId,
    pub revision: u64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalHostStatus {
    pub protocol_version: u16,
    pub host_version: String,
    #[serde(default)]
    pub host_build_id: Option<String>,
    pub draining: bool,
    pub running_sessions: usize,
    pub retained_sessions: usize,
    /// Live sessions that are mid-turn: active, or waiting on a person.
    ///
    /// `None` from a host too old to answer, which is NOT the same as zero and
    /// must never be read as one. Operator ruling 01a05b83 settles what a
    /// caller does with that: proceed, and say loudly that it could not check.
    #[serde(default)]
    pub busy_sessions: Option<usize>,
    /// Live sessions whose provider this host cannot classify.
    ///
    /// Separate from `busy_sessions` because the honest answers differ: one is
    /// "work is happening", the other is "I do not know". Collapsing them is
    /// how a session count came to stand in for a reading.
    #[serde(default)]
    pub unreadable_sessions: Option<usize>,
    #[serde(default)]
    pub resources: Option<ProcessResourceSample>,
    #[serde(default)]
    pub takeover_relay: bool,
}

impl TerminalHostStatus {
    /// Unknown future protocols are not silently assumed compatible.
    #[must_use]
    pub fn supports_terminal_control(&self) -> bool {
        (TERMINAL_CONTROL_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&self.protocol_version)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostResponse {
    Pong {
        protocol_version: u16,
    },
    HostStatus {
        status: TerminalHostStatus,
    },
    ProviderCapabilities {
        claude_code: bool,
        codex: bool,
        /// What each provider executable resolves to right now.
        ///
        /// Defaulted so an older host, which reports availability only, stays
        /// readable: a missing release is "not known", not "not superseded".
        #[serde(default)]
        claude_release: Option<crate::ProviderRelease>,
        #[serde(default)]
        codex_release: Option<crate::ProviderRelease>,
    },
    SessionStarted {
        session_id: WorkerSessionId,
    },
    Sessions {
        sessions: Vec<HostSessionSummary>,
    },
    HistoryDiagnostics {
        diagnostics: Option<HistoryDiagnostics>,
    },
    HistorySessions {
        sessions: Vec<HistorySessionSummary>,
    },
    HistoryPage {
        page: HistoryPage,
    },
    WriteAudit {
        entries: Vec<TerminalWriteAuditEntry>,
    },
    Output {
        session_id: WorkerSessionId,
        resume: Resume,
        running: bool,
    },
    Control {
        session_id: WorkerSessionId,
        control: TerminalControlStatus,
    },
    ControlledOutput {
        session_id: WorkerSessionId,
        resume: Resume,
        running: bool,
        control: TerminalControlStatus,
    },
    Acknowledged,
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("terminal host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal host protocol failed: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("terminal host response exceeded {limit} bytes")]
    ResponseTooLarge { limit: u64 },
    #[error("terminal host closed without a response")]
    EmptyResponse,
}

#[derive(Clone, Debug)]
#[cfg(unix)]
pub struct HostClient {
    socket_path: PathBuf,
}

#[cfg(unix)]
impl HostClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Sends one bounded request over an authenticated local socket.
    ///
    /// # Errors
    ///
    /// Returns an error for connection, framing, size, or JSON failures.
    pub async fn request(&self, request: &HostRequest) -> Result<HostResponse, IpcError> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;
        let mut payload = serde_json::to_vec(request)?;
        payload.push(b'\n');
        if payload.len() as u64 > MAX_REQUEST_BYTES {
            return Err(IpcError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "request exceeded the bounded frame",
            )));
        }
        stream.write_all(&payload).await?;

        let mut response = Vec::new();
        let mut reader = BufReader::new(stream).take(MAX_RESPONSE_BYTES + 1);
        reader.read_until(b'\n', &mut response).await?;
        if response.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(IpcError::ResponseTooLarge {
                limit: MAX_RESPONSE_BYTES,
            });
        }
        if response.is_empty() {
            return Err(IpcError::EmptyResponse);
        }
        Ok(serde_json::from_slice(&response)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Adding a host request without bumping `PROTOCOL_VERSION` fails here.
    ///
    /// THE CHECK ALREADY EXISTED AND ITS INPUT WAS MAINTAINED BY HAND, which is
    /// the whole defect. The version is scraped into a PROTOCOL file by
    /// build-release.sh and compared by `reconcile_host`, which refuses to swap a
    /// host across a change. All of that machinery worked perfectly and reported
    /// nothing, because `StartShell` was added while the number stayed at 9 --
    /// so it compared 9 to 9. The operator met "unknown variant `start_shell`".
    ///
    /// A check whose input a human has to remember to update is a document
    /// wearing a check's clothes. This is what makes the number trustworthy.
    ///
    /// Read through serde rather than from a hand-written list, because serde's
    /// view IS the wire surface -- the thing an older host actually fails to
    /// parse. A list maintained beside the enum could drift from it in exactly
    /// the way the version drifted.
    #[test]
    fn a_new_host_request_requires_a_protocol_bump() {
        // Pinned deliberately. Changing this list without changing the version
        // below is the mistake being prevented, so both must move together.
        const PINNED: &[&str] = &[
            "ping",
            "host_status",
            "provider_capabilities",
            "begin_drain",
            "cancel_drain",
            "start_claude",
            "start_codex",
            "start_shell",
            "start_alpha_provider",
            "list_sessions",
            "history_diagnostics",
            "list_history_sessions",
            "read_history",
            "read",
            "wait",
            "write",
            "write_audit",
            "install_takeover",
            "takeover_write",
            "reclaim_takeover_and_write",
            "release_takeover",
            "resize",
            "control",
            "wait_controlled",
            "stop",
        ];
        let error = serde_json::from_value::<HostRequest>(serde_json::json!({
            "type": "a_request_that_does_not_exist"
        }))
        .expect_err("an unknown request must not parse")
        .to_string();
        let listed: Vec<String> = error
            .split("expected one of ")
            .nth(1)
            .expect("serde names the variants it knows")
            .split(", ")
            .map(|name| name.trim().trim_matches('`').to_owned())
            .collect();

        assert_eq!(
            listed, PINNED,
            "the host request surface changed. An older terminal host cannot \
             parse what this build now sends, and only PROTOCOL_VERSION tells \
             anyone -- bump it, then update this list."
        );
        assert_eq!(
            PROTOCOL_VERSION, 11,
            "the pinned surface above belongs to protocol 11; if you changed \
             the requests, this number moves with them"
        );
    }

    #[test]
    fn older_host_status_without_resources_remains_compatible() {
        let status: TerminalHostStatus = serde_json::from_str(
            r#"{"protocol_version":8,"host_version":"0.1.0","draining":false,"running_sessions":1,"retained_sessions":1}"#,
        )
        .unwrap();
        // Read as 8 because the payload says 8, and asserted OLDER rather than
        // exactly-one-behind. This was `PROTOCOL_VERSION - 1`, which tied a
        // fixed payload to a moving constant and so failed on the next bump
        // without anything about compatibility having changed.
        assert_eq!(status.protocol_version, 8);
        assert!(
            status.protocol_version < PROTOCOL_VERSION,
            "this fixture exists to model a host older than the current build"
        );
        assert_eq!(status.host_build_id, None);
        assert_eq!(status.resources, None);
        assert!(!status.takeover_relay);
        assert!(!status.supports_terminal_control());
        let mut current = status;
        current.protocol_version = PROTOCOL_VERSION;
        assert!(current.supports_terminal_control());
        current.protocol_version = PROTOCOL_VERSION + 1;
        assert!(!current.supports_terminal_control());
    }

    #[test]
    fn nested_control_commands_are_part_of_the_pinned_protocol() {
        let error = serde_json::from_value::<TerminalControlCommand>(
            serde_json::json!({ "kind": "unknown" }),
        )
        .unwrap_err()
        .to_string();
        let variants: Vec<_> = error
            .split("expected one of ")
            .nth(1)
            .unwrap()
            .split(", ")
            .map(|name| name.trim().trim_matches('`'))
            .collect();
        assert_eq!(
            variants,
            ["status", "claim", "renew", "release", "input", "resize"]
        );
        assert_eq!(PROTOCOL_VERSION, 11);
    }

    #[test]
    fn control_cursor_preserves_full_generation_and_distinguishes_expiry() {
        let status = TerminalControlStatus {
            generation: u64::MAX,
            owner: None,
            lease_remaining_ms: 0,
        };
        let cursor = status.cursor();
        let encoded = serde_json::to_string(&cursor).unwrap();
        assert_eq!(
            serde_json::from_str::<TerminalControlCursor>(&encoded).unwrap(),
            cursor
        );
        assert!(!cursor.occupied);
        let occupied = TerminalControlCursor {
            occupied: true,
            ..cursor
        };
        assert_ne!(cursor, occupied);
        assert!(
            serde_json::from_str::<TerminalControlCursor>(r#"{"generation":-1,"occupied":true}"#)
                .is_err()
        );
    }

    #[test]
    fn operator_provenance_classifies_input_without_retaining_content() {
        let operator = |bytes: &[u8]| TerminalWriteProvenance::operator(None, bytes).input_kind;
        assert_eq!(operator(b"hello"), TerminalInputKind::Text);
        assert_eq!(
            operator(b"\x1b[200~hello\x1b[201~"),
            TerminalInputKind::Paste
        );
        assert_eq!(operator(b"\r"), TerminalInputKind::Submit);
        assert_eq!(operator(&[3]), TerminalInputKind::Interrupt);
        assert_eq!(operator(b"\x1b[A"), TerminalInputKind::Navigation);
        assert_eq!(operator(&[4]), TerminalInputKind::Control);
    }

    #[test]
    fn session_recovery_provenance_is_optional_and_round_trips() {
        let id = WorkerSessionId::new();
        let mut summary: HostSessionSummary = serde_json::from_value(serde_json::json!({
            "session_id": id, "running": true
        }))
        .unwrap();
        assert_eq!(summary.recovery_attempt, None);
        assert!(
            serde_json::to_value(&summary)
                .unwrap()
                .get("recovery_attempt")
                .is_none()
        );
        let recovery = swarm_domain::ConversationRecovery::new(None, true);
        let swarm_domain::ConversationRecoveryState::Attempt { attempt } = recovery.state() else {
            panic!("expected continuation attempt");
        };
        summary.recovery_attempt = Some(attempt);
        assert_eq!(
            serde_json::from_value::<HostSessionSummary>(serde_json::to_value(&summary).unwrap())
                .unwrap(),
            summary
        );
    }
}
