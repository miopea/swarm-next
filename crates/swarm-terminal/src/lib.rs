mod control_gate;
mod control_protocol;
mod history;
mod history_retention;
mod ipc;
mod journal;
mod maintenance;
mod native_interview_capture;
mod process;
mod provider;
mod provider_activity;
mod provider_interview;
mod provider_lifecycle;
mod provider_lifecycle_gate;
mod provider_turn;

/// The response header carrying the revision of the tools Swarm serves an
/// agent. The relay compares it across responses and tells Claude when it
/// moves, so a running worker refetches its tools (ADR 0123).
pub const AGENT_TOOL_SURFACE_HEADER: &str = "x-swarm-tool-surface";
/// Appended to a worker's MCP URL to reach the endpoint its permission hook
/// asks before each shell command, to spend a grant the operator pressed
/// (ADR 0129).
pub const COMMAND_GRANT_PATH_SUFFIX: &str = "/command-grant";
/// Appended to a worker's MCP URL to reach the endpoint its grant mod tells
/// when Claude's auto mode has refused a command (ADR 0130).
pub const COMMAND_REFUSED_PATH_SUFFIX: &str = "/command-refused";
/// The key in a worker's `<worker>.settings.json` saying the operator turned
/// on "Queen releases reviewed work" (ADR 0131). The API writes it and the
/// terminal host reads it at launch.
pub const QUEEN_RELEASES_SETTING_KEY: &str = "swarmQueenReleases";
mod resources;
mod startup_failure;
mod state;

pub use control_protocol::dispatch_terminal_control;
pub use history::{
    HistoryAppendOutcome, HistoryCursor, HistoryDiagnostics, HistoryError, HistoryLimits,
    HistoryPage, HistoryRecord, HistorySessionSummary, HistoryStore, MAX_HISTORY_PAGE_BYTES,
    MAX_HISTORY_PAGE_RECORDS, MAX_HISTORY_RECORD_BYTES, default_terminal_history_path,
};
pub use history_retention::{
    history_retention_applied_path, history_retention_request_path, read_history_retention,
    write_history_retention,
};
#[cfg(unix)]
pub use ipc::HostClient;
pub use ipc::{
    HostRequest, HostResponse, HostSessionSummary, IpcError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
    MAX_WRITE_AUDIT_PAGE, NativeInterviewPayload, PROTOCOL_VERSION, ProviderLifecycleCapability,
    TerminalHostStatus, TerminalInputKind, TerminalTakeoverLease, TerminalWriteActor,
    TerminalWriteAuditEntry, TerminalWriteProvenance, TerminalWriteResult,
    default_terminal_socket_path, supports_continuation_recovery,
};
pub use ipc::{
    MAX_CONTROL_INPUT_BYTES, TERMINAL_CONTROL_PROTOCOL_VERSION, TerminalControlCommand,
    TerminalControlCursor, TerminalControlStatus,
};
pub use journal::{JournalLimits, SequencedFrame};
pub use maintenance::{MaintenanceOutcome, MaintenanceRefusal};
pub use native_interview_capture::NativeInterviewEvidence;
pub use process::{
    ContinuationRecoveryOutcome, FreshRecoveryLaunch, MAX_TERMINAL_CELLS, MAX_TERMINAL_COLUMNS,
    MAX_TERMINAL_ROWS, MIN_TERMINAL_COLUMNS, MIN_TERMINAL_ROWS, ProcessTerminalSession,
    SessionRegistry, SessionRegistryError, TerminalSize,
};
pub use provider::{
    AlphaProviderAdapter, ClaudeCodeAdapter, ClaudeConversationStart, CodexAdapter,
    CodexConversationStart, ProviderCommand, ProviderCommandError, ProviderRelease,
    ProviderTerminalAdapter, claude_global_config, claude_restart_enabled, provider_release,
    provider_release_superseded, running_executable, running_release_current, shell_command,
};
pub use provider_activity::{
    BackgroundWork, ProviderActivity, background_work, classify_provider_activity,
    claude_restart_unknown,
};
pub use provider_interview::{
    NativeInterviewObservation, NativeInterviewOption, NativeInterviewPhase,
    NativeInterviewQuestion, read_claude_interview,
};
pub use provider_lifecycle::{
    MAX_PROVIDER_LIFECYCLE_BYTES, ProviderSessionStartObservation, read_claude_resume_end,
    read_claude_session_start,
};
pub use provider_lifecycle_gate::{ProviderLifecycleAcceptance, ProviderLifecycleGate};
pub use provider_turn::{
    MAX_PROVIDER_TURN_INPUT_BYTES, ProviderDenial, ProviderTurn, ProviderTurnBackground,
    ProviderTurnEvent, ProviderTurnFailure, ProviderTurnObservation, ProviderTurnPhase,
    ProviderTurnWait, compact_turn_payload, read_claude_turn_event,
};
mod provider_prompt;
pub use provider_prompt::{ProviderPromptObservation, read_claude_prompt_submission};
pub use resources::{ProcessResourceSample, sample_current_process, sample_process_tree};
pub use state::{
    CANONICAL_COMPACTION_INPUT_BYTES, CANONICAL_SCROLLBACK_ROWS, CanonicalTerminalState,
    MAX_CANONICAL_SNAPSHOT_BYTES, Resume, TerminalSnapshot, snapshot_plain_text,
};
