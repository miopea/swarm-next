mod history;
mod ipc;
mod journal;
mod process;
mod provider;
mod state;

pub use history::{
    HistoryAppendOutcome, HistoryDiagnostics, HistoryError, HistoryLimits, HistoryRecord,
    HistoryStore, default_terminal_history_path,
};
pub use ipc::{
    HostClient, HostRequest, HostResponse, HostSessionSummary, IpcError, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES, PROTOCOL_VERSION, default_terminal_socket_path,
};
pub use journal::{JournalLimits, SequencedFrame};
pub use process::{
    MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, ProcessTerminalSession,
    SessionRegistry, SessionRegistryError, TerminalSize,
};
pub use provider::{
    ClaudeCodeAdapter, ProviderCommand, ProviderCommandError, ProviderTerminalAdapter,
};
pub use state::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, CanonicalTerminalState,
    MAX_CANONICAL_SNAPSHOT_BYTES, Resume, TerminalSnapshot,
};
