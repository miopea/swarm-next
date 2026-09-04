mod control_gate;
mod control_protocol;
mod history;
mod ipc;
mod journal;
mod process;
mod provider;
mod provider_activity;
mod provider_lifecycle;
mod resources;
mod state;

pub use control_protocol::dispatch_terminal_control;
pub use history::{
    HistoryAppendOutcome, HistoryCursor, HistoryDiagnostics, HistoryError, HistoryLimits,
    HistoryPage, HistoryRecord, HistorySessionSummary, HistoryStore, MAX_HISTORY_PAGE_BYTES,
    MAX_HISTORY_PAGE_RECORDS, MAX_HISTORY_RECORD_BYTES, default_terminal_history_path,
};
#[cfg(unix)]
pub use ipc::HostClient;
pub use ipc::{
    HostRequest, HostResponse, HostSessionSummary, IpcError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
    MAX_WRITE_AUDIT_PAGE, PROTOCOL_VERSION, TerminalHostStatus, TerminalInputKind,
    TerminalTakeoverLease, TerminalWriteActor, TerminalWriteAuditEntry, TerminalWriteProvenance,
    TerminalWriteResult, default_terminal_socket_path,
};
pub use ipc::{
    MAX_CONTROL_INPUT_BYTES, TERMINAL_CONTROL_PROTOCOL_VERSION, TerminalControlCommand,
    TerminalControlCursor, TerminalControlStatus,
};
pub use journal::{JournalLimits, SequencedFrame};
pub use process::{
    MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, MIN_TERMINAL_COLUMNS,
    MIN_TERMINAL_ROWS, ProcessTerminalSession, SessionRegistry, SessionRegistryError, TerminalSize,
};
pub use provider::{
    AlphaProviderAdapter, ClaudeCodeAdapter, ClaudeConversationStart, CodexAdapter,
    CodexConversationStart, ProviderCommand, ProviderCommandError, ProviderRelease,
    ProviderTerminalAdapter, provider_release, provider_release_superseded, shell_command,
};
pub use provider_activity::{
    ProviderActivity, background_work_running, classify_provider_activity,
};
pub use provider_lifecycle::{
    MAX_PROVIDER_LIFECYCLE_BYTES, ProviderSessionStartObservation, read_claude_session_start,
};
pub use resources::{ProcessResourceSample, sample_current_process, sample_process_tree};
pub use state::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, CanonicalTerminalState,
    MAX_CANONICAL_SNAPSHOT_BYTES, Resume, TerminalSnapshot, snapshot_plain_text,
};
