use std::{
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use swarm_domain::WorkerSessionId;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::{
    ClaudeConversationStart, HistoryCursor, HistoryDiagnostics, HistoryPage, HistorySessionSummary,
    Resume, TerminalSize,
};

pub const PROTOCOL_VERSION: u16 = 6;
pub const MAX_REQUEST_BYTES: u64 = 256 * 1024;
pub const MAX_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;

#[must_use]
pub fn default_terminal_socket_path() -> PathBuf {
    let home = env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local/state/swarm-next/run/terminal.sock")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    Ping,
    HostStatus,
    BeginDrain,
    CancelDrain,
    StartClaude {
        workspace: PathBuf,
        size: TerminalSize,
        conversation: ClaudeConversationStart,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalHostStatus {
    pub protocol_version: u16,
    pub host_version: String,
    pub draining: bool,
    pub running_sessions: usize,
    pub retained_sessions: usize,
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
