mod history;
mod ipc;
mod journal;
mod process;
mod provider;
mod provider_activity;
mod resources;
mod state;

pub use history::{
    HistoryAppendOutcome, HistoryCursor, HistoryDiagnostics, HistoryError, HistoryLimits,
    HistoryPage, HistoryRecord, HistorySessionSummary, HistoryStore, MAX_HISTORY_PAGE_BYTES,
    MAX_HISTORY_PAGE_RECORDS, MAX_HISTORY_RECORD_BYTES, default_terminal_history_path,
};
pub use ipc::{
    HostClient, HostRequest, HostResponse, HostSessionSummary, IpcError, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES, PROTOCOL_VERSION, TerminalHostStatus, default_terminal_socket_path,
};
pub use journal::{JournalLimits, SequencedFrame};
pub use process::{
    MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, MIN_TERMINAL_COLUMNS,
    MIN_TERMINAL_ROWS, ProcessTerminalSession, SessionRegistry, SessionRegistryError, TerminalSize,
};
pub use provider::{
    ClaudeCodeAdapter, ClaudeConversationStart, CodexAdapter, CodexConversationStart,
    ProviderCommand, ProviderCommandError, ProviderTerminalAdapter,
};
pub use provider_activity::{ProviderActivity, classify_provider_activity};
pub use resources::{ProcessResourceSample, sample_current_process};
pub use state::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, CanonicalTerminalState,
    MAX_CANONICAL_SNAPSHOT_BYTES, Resume, TerminalSnapshot,
};
