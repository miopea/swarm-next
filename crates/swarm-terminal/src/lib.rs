mod ipc;
mod journal;
mod process;
mod provider;

pub use ipc::{
    HostClient, HostRequest, HostResponse, HostSessionSummary, IpcError, MAX_REQUEST_BYTES,
    MAX_RESPONSE_BYTES, PROTOCOL_VERSION, default_terminal_socket_path,
};
pub use journal::{BoundedJournal, JournalLimits, Resume, SequencedFrame};
pub use process::{ProcessTerminalSession, SessionRegistry, SessionRegistryError, TerminalSize};
pub use provider::{
    ClaudeCodeAdapter, ProviderCommand, ProviderCommandError, ProviderTerminalAdapter,
};
