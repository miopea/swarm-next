use std::{
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use swarm_domain::{FederationStewardTakeoverLeaseId, PresenceDeviceId, WorkerSessionId};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::{
    ClaudeConversationStart, CodexConversationStart, HistoryCursor, HistoryDiagnostics,
    HistoryPage, HistorySessionSummary, ProcessResourceSample, Resume, TerminalSize,
};

pub const PROTOCOL_VERSION: u16 = 9;
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
    Stop {
        session_id: WorkerSessionId,
    },
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
    #[serde(default)]
    pub resources: Option<ProcessResourceSample>,
    #[serde(default)]
    pub takeover_relay: bool,
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
pub struct HostClient {
    socket_path: PathBuf,
}

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
        // THIS PAIRING IS KNOWN TO BE WRONG AND CANNOT BE FIXED BY BUMPING.
        // `start_shell` was added at protocol 9 without a bump, so the surface
        // above is NOT what a v0.8.17 host serves even though both say 9 --
        // which is exactly the 422 the operator met.
        //
        // The bump that would correct it was made and then reverted, because
        // swarm-package refuses to install across a protocol change:
        // install_or_update dies with "protocol change requires an explicit
        // compatibility migration", and reconcile_host refuses too. With the
        // installed host at 9, a build at 10 cannot be installed by any path
        // this packaging offers, so bumping silently broke every reload.
        //
        // So the number is pinned to the truth about THIS repository -- 9, with
        // start_shell present -- and the disagreement with a shipped 0.8.17
        // host is recorded rather than papered over. Fixing it needs a way to
        // migrate the host across a protocol change, which does not exist yet.
        assert_eq!(
            PROTOCOL_VERSION, 9,
            "the pinned surface above belongs to protocol 9; if you changed \
             the requests, this number moves with them -- but read the note \
             above first, because there is currently no way to INSTALL a \
             protocol change"
        );
    }

    #[test]
    fn older_host_status_without_resources_remains_compatible() {
        let status: TerminalHostStatus = serde_json::from_str(
            r#"{"protocol_version":8,"host_version":"0.1.0","draining":false,"running_sessions":1,"retained_sessions":1}"#,
        )
        .unwrap();
        assert_eq!(status.protocol_version, PROTOCOL_VERSION - 1);
        assert_eq!(status.host_build_id, None);
        assert_eq!(status.resources, None);
        assert!(!status.takeover_relay);
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
}
