use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use swarm_domain::{FederationStewardTakeoverLeaseId, WorkerSessionId};
use thiserror::Error;
use tokio::sync::watch;
use tracing::warn;

use crate::{
    CanonicalTerminalState, HistoryAppendOutcome, HistoryCursor, HistoryDiagnostics, HistoryError,
    HistoryPage, HistorySessionSummary, HistoryStore, JournalLimits, ProviderCommand, Resume,
    TerminalTakeoverLease,
};

pub const MAX_TERMINAL_ROWS: u16 = 200;
pub const MAX_TERMINAL_COLUMNS: u16 = 320;
pub const MAX_TERMINAL_CELLS: usize = 32_000;
pub const MIN_TERMINAL_ROWS: u16 = 4;
pub const MIN_TERMINAL_COLUMNS: u16 = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub columns: u16,
}

impl TerminalSize {
    #[must_use]
    pub const fn new(rows: u16, columns: u16) -> Self {
        Self { rows, columns }
    }

    fn as_pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn validate(self) -> Result<(), SessionRegistryError> {
        let cells = usize::from(self.rows) * usize::from(self.columns);
        if self.rows < MIN_TERMINAL_ROWS
            || self.columns < MIN_TERMINAL_COLUMNS
            || self.rows > MAX_TERMINAL_ROWS
            || self.columns > MAX_TERMINAL_COLUMNS
            || cells > MAX_TERMINAL_CELLS
        {
            return Err(SessionRegistryError::Terminal(format!(
                "terminal dimensions must be at least {MIN_TERMINAL_ROWS} rows and \
                 {MIN_TERMINAL_COLUMNS} columns, and within {MAX_TERMINAL_ROWS} rows, \
                 {MAX_TERMINAL_COLUMNS} columns, and {MAX_TERMINAL_CELLS} cells"
            )));
        }
        Ok(())
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(24, 80)
    }
}

#[derive(Debug, Error)]
pub enum SessionRegistryError {
    #[error("terminal session limit of {limit} reached")]
    SessionLimitReached { limit: usize },
    #[error("terminal host is draining for an update")]
    HostDraining,
    #[error("workspace is outside the configured roots: {0}")]
    WorkspaceNotAllowed(PathBuf),
    #[error("workspace cannot be resolved: {0}")]
    WorkspaceUnavailable(PathBuf),
    #[error("terminal session was not found")]
    SessionNotFound,
    #[error("terminal takeover authority conflicts with the active lease")]
    TakeoverConflict,
    #[error("terminal takeover authority is missing, stale, or expired")]
    TakeoverDenied,
    #[error("terminal operation failed: {0}")]
    Terminal(String),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("terminal session lock was poisoned")]
    LockPoisoned,
}

pub struct ProcessTerminalSession {
    id: WorkerSessionId,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    terminal_state: Arc<Mutex<CanonicalTerminalState>>,
    output_state: watch::Sender<bool>,
    reader_running: Arc<AtomicBool>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    history: Option<Arc<HistoryStore>>,
}

impl std::fmt::Debug for ProcessTerminalSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessTerminalSession")
            .field("id", &self.id)
            .field(
                "reader_running",
                &self.reader_running.load(Ordering::Acquire),
            )
            .finish_non_exhaustive()
    }
}

impl ProcessTerminalSession {
    /// Spawns a process under a new pseudo-terminal.
    ///
    /// # Errors
    ///
    /// Returns an error when the PTY, reader, writer, or child process cannot
    /// be created.
    pub fn spawn(
        id: WorkerSessionId,
        command: &ProviderCommand,
        limits: JournalLimits,
        size: TerminalSize,
    ) -> Result<Self, SessionRegistryError> {
        Self::spawn_with_history(id, command, limits, size, None)
    }

    fn spawn_with_history(
        id: WorkerSessionId,
        command: &ProviderCommand,
        limits: JournalLimits,
        size: TerminalSize,
        history: Option<Arc<HistoryStore>>,
    ) -> Result<Self, SessionRegistryError> {
        size.validate()?;
        let pair = native_pty_system()
            .openpty(size.as_pty_size())
            .map_err(terminal_error)?;
        let mut command_builder = CommandBuilder::new(&command.executable);
        command_builder.args(&command.arguments);
        command_builder.cwd(&command.working_directory);
        // Provider CLIs inspect terminal capabilities before emitting styled output.
        // The terminal host commonly runs under systemd without TERM, even though it
        // gives the child a real PTY. Declare the xterm contract rendered by the web
        // client so live output and canonical snapshots retain ANSI styling.
        command_builder.env("TERM", "xterm-256color");
        command_builder.env("COLORTERM", "truecolor");
        command_builder.env("FORCE_COLOR", "3");
        command_builder.env("CLICOLOR_FORCE", "1");
        command_builder.env_remove("NO_COLOR");
        // Claude's flicker-free renderer otherwise selects the alternate screen,
        // where xterm intentionally has no scrollback. Keep Claude in the main
        // buffer so operators can review bounded history and restored snapshots.
        if is_claude_executable(&command.executable) {
            command_builder.env("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");
        }
        let child = pair
            .slave
            .spawn_command(command_builder)
            .map_err(terminal_error)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(terminal_error)?;
        let writer = pair.master.take_writer().map_err(terminal_error)?;
        let terminal_state = Arc::new(Mutex::new(CanonicalTerminalState::new(limits, size)));
        if let Some(history) = &history {
            history.start_session(id)?;
            history.append_checkpoint(id, &lock(&terminal_state)?.snapshot())?;
        }
        let reader_terminal_state = Arc::clone(&terminal_state);
        let reader_history = history.as_ref().map(Arc::clone);
        let (output_state, _) = watch::channel(true);
        let reader_output_state = output_state.clone();
        let reader_running = Arc::new(AtomicBool::new(true));
        let reader_state = Arc::clone(&reader_running);
        let reader_thread = thread::Builder::new()
            .name(format!("terminal-reader-{id:?}"))
            .spawn(move || {
                let mut buffer = vec![0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let output = &buffer[..read];
                            let sequence =
                                if let Ok(mut terminal_state) = reader_terminal_state.lock() {
                                    terminal_state.push(output.to_vec())
                                } else {
                                    break;
                                };
                            if let Some(history) = &reader_history {
                                match history.append(id, sequence, output) {
                                    Ok(HistoryAppendOutcome::CheckpointRequired) => {
                                        let snapshot = match reader_terminal_state.lock() {
                                            Ok(terminal_state) => terminal_state.snapshot(),
                                            Err(_) => break,
                                        };
                                        if let Err(error) = history.append_checkpoint(id, &snapshot)
                                        {
                                            warn!(session_id = %id, %error, "terminal history checkpoint failed");
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(error) => {
                                        warn!(session_id = %id, %error, "terminal history append failed");
                                    }
                                }
                            }
                            reader_output_state.send_replace(true);
                        }
                    }
                }
                if let Some(history) = &reader_history
                    && let Err(error) = history.finish_session(id)
                {
                    warn!(session_id = %id, %error, "terminal history finalization failed");
                }
                reader_state.store(false, Ordering::Release);
                reader_output_state.send_replace(false);
            })
            .map_err(terminal_error)?;

        Ok(Self {
            id,
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            terminal_state,
            output_state,
            reader_running,
            reader_thread: Mutex::new(Some(reader_thread)),
            history,
        })
    }

    #[must_use]
    pub const fn id(&self) -> WorkerSessionId {
        self.id
    }

    /// Writes input directly to the PTY master.
    ///
    /// # Errors
    ///
    /// Returns an error when the session lock is poisoned or the PTY rejects
    /// the write.
    pub fn write_input(&self, bytes: &[u8]) -> Result<(), SessionRegistryError> {
        let mut writer = lock(&self.writer)?;
        writer.write_all(bytes).map_err(terminal_error)?;
        writer.flush().map_err(terminal_error)
    }

    /// Commits a new non-zero terminal size.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero dimension, poisoned lock, or PTY failure.
    pub fn resize(&self, size: TerminalSize) -> Result<(), SessionRegistryError> {
        size.validate()?;
        let mut terminal_state = lock(&self.terminal_state)?;
        if terminal_state.size() == size {
            return Ok(());
        }
        lock(&self.master)?
            .resize(size.as_pty_size())
            .map_err(terminal_error)?;
        let canonical_resized = terminal_state.resize(size);
        debug_assert!(canonical_resized);
        if let Some(history) = &self.history {
            let snapshot = terminal_state.snapshot();
            if let Err(error) = history.append_checkpoint(self.id, &snapshot) {
                warn!(session_id = %self.id, %error, "terminal history resize checkpoint failed");
            }
        }
        drop(terminal_state);
        self.output_state.send_replace(true);
        Ok(())
    }

    /// Returns retained deltas or a deterministic snapshot requirement.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounded journal lock was poisoned.
    pub fn resume_after(&self, sequence: Option<u64>) -> Result<Resume, SessionRegistryError> {
        Ok(lock(&self.terminal_state)?.resume_after(sequence))
    }

    /// Waits until output advances or the PTY reader closes, then returns a
    /// bounded resume result. Subscribing before the initial read prevents a
    /// notification race between checking the journal and sleeping.
    ///
    /// # Errors
    ///
    /// Returns an error if the journal lock is poisoned or the notification
    /// channel closes unexpectedly.
    pub async fn wait_after(
        &self,
        sequence: Option<u64>,
    ) -> Result<(Resume, bool), SessionRegistryError> {
        let mut changes = self.output_state.subscribe();
        loop {
            let reader_running = *changes.borrow_and_update();
            let resume = self.resume_after(sequence)?;
            if resume_has_output(&resume) || !reader_running {
                return Ok((resume, reader_running));
            }
            changes.changed().await.map_err(|_| {
                SessionRegistryError::Terminal("terminal output notifier closed".into())
            })?;
        }
    }

    /// Checks process state without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error if the process lock is poisoned or the OS query fails.
    pub fn is_running(&self) -> Result<bool, SessionRegistryError> {
        Ok(lock(&self.child)?
            .try_wait()
            .map_err(terminal_error)?
            .is_none())
    }

    /// Samples the provider process tree owned by this terminal session.
    ///
    /// # Errors
    ///
    /// Returns an error if the child process lock is poisoned.
    pub fn resource_sample(
        &self,
    ) -> Result<Option<crate::ProcessResourceSample>, SessionRegistryError> {
        let process_id = lock(&self.child)?.process_id();
        Ok(process_id.map(crate::sample_process_tree))
    }

    /// Stops the child process explicitly.
    ///
    /// # Errors
    ///
    /// Returns an error when the child lock is poisoned or termination fails.
    pub fn stop(&self) -> Result<(), SessionRegistryError> {
        let mut child = lock(&self.child)?;
        if child.try_wait().map_err(terminal_error)?.is_none() {
            child.kill().map_err(terminal_error)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn reader_running(&self) -> bool {
        self.reader_running.load(Ordering::Acquire)
    }
}

fn is_claude_executable(executable: &std::path::Path) -> bool {
    executable
        .file_name()
        .is_some_and(|name| name == "claude" || name == "claude.exe")
}

fn resume_has_output(resume: &Resume) -> bool {
    match resume {
        Resume::Deltas { frames } => !frames.is_empty(),
        Resume::Snapshot { .. } => true,
    }
}

impl Drop for ProcessTerminalSession {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
        }
        if let Ok(reader_thread) = self.reader_thread.get_mut()
            && let Some(reader_thread) = reader_thread.take()
        {
            // Destructors must not wait indefinitely on an OS reader. The
            // journal stays bounded while this detached reader observes PTY
            // closure after the child is killed.
            drop(reader_thread);
        }
    }
}

#[derive(Debug)]
pub struct SessionRegistry {
    sessions: Mutex<HashMap<WorkerSessionId, Arc<ProcessTerminalSession>>>,
    takeovers: Mutex<HashMap<WorkerSessionId, TerminalTakeoverLease>>,
    limits: JournalLimits,
    max_sessions: usize,
    allowed_roots: Vec<PathBuf>,
    history: Option<Arc<HistoryStore>>,
    draining: AtomicBool,
}

impl SessionRegistry {
    /// Creates a registry and resolves every configured workspace root.
    ///
    /// # Errors
    ///
    /// Returns an error when any allowed root cannot be canonicalized.
    pub fn new(
        limits: JournalLimits,
        max_sessions: usize,
        allowed_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, SessionRegistryError> {
        Self::new_with_history(limits, max_sessions, allowed_roots, None)
    }

    /// Creates a registry with optional host-owned durable history.
    ///
    /// # Errors
    ///
    /// Returns an error when any allowed root cannot be canonicalized.
    pub fn new_with_history(
        limits: JournalLimits,
        max_sessions: usize,
        allowed_roots: impl IntoIterator<Item = PathBuf>,
        history: Option<Arc<HistoryStore>>,
    ) -> Result<Self, SessionRegistryError> {
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|root| {
                root.canonicalize()
                    .map_err(|_| SessionRegistryError::WorkspaceUnavailable(root))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
            takeovers: Mutex::new(HashMap::new()),
            limits,
            max_sessions,
            allowed_roots,
            history,
            draining: AtomicBool::new(false),
        })
    }

    /// Spawns a validated provider command and registers its immutable session.
    ///
    /// # Errors
    ///
    /// Returns an error for a disallowed workspace, capacity exhaustion, lock
    /// poisoning, or PTY/process failure.
    pub fn spawn(
        &self,
        command: &ProviderCommand,
        size: TerminalSize,
    ) -> Result<Arc<ProcessTerminalSession>, SessionRegistryError> {
        self.spawn_with_root_override(command, size, false)
    }

    /// Spawns a command with an explicit trusted-caller exception to configured roots.
    ///
    /// # Errors
    ///
    /// Returns an error when the workspace is unavailable or unsafe, the
    /// registry cannot accept a session, or the PTY process cannot be started.
    pub fn spawn_with_root_override(
        &self,
        command: &ProviderCommand,
        size: TerminalSize,
        allow_outside_roots: bool,
    ) -> Result<Arc<ProcessTerminalSession>, SessionRegistryError> {
        let canonical_workspace = command.working_directory.canonicalize().map_err(|_| {
            SessionRegistryError::WorkspaceUnavailable(command.working_directory.clone())
        })?;
        if !allow_outside_roots
            && !self
                .allowed_roots
                .iter()
                .any(|root| canonical_workspace.starts_with(root))
        {
            return Err(SessionRegistryError::WorkspaceNotAllowed(
                canonical_workspace,
            ));
        }

        let mut sessions = lock(&self.sessions)?;
        if self.draining.load(Ordering::Acquire) {
            return Err(SessionRegistryError::HostDraining);
        }
        if sessions.len() >= self.max_sessions {
            return Err(SessionRegistryError::SessionLimitReached {
                limit: self.max_sessions,
            });
        }
        let id = WorkerSessionId::new();
        let session = Arc::new(ProcessTerminalSession::spawn_with_history(
            id,
            command,
            self.limits,
            size,
            self.history.as_ref().map(Arc::clone),
        )?);
        sessions.insert(id, Arc::clone(&session));
        Ok(session)
    }

    /// Returns a session by immutable identity.
    ///
    /// # Errors
    ///
    /// Returns an error for a poisoned registry lock or unknown session.
    pub fn get(
        &self,
        id: WorkerSessionId,
    ) -> Result<Arc<ProcessTerminalSession>, SessionRegistryError> {
        lock(&self.sessions)?
            .get(&id)
            .cloned()
            .ok_or(SessionRegistryError::SessionNotFound)
    }

    /// Stops and removes an explicitly identified session.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session or stop failure.
    pub fn stop(&self, id: WorkerSessionId) -> Result<(), SessionRegistryError> {
        let session = lock(&self.sessions)?
            .remove(&id)
            .ok_or(SessionRegistryError::SessionNotFound)?;
        lock(&self.takeovers)?.remove(&id);
        session.stop()
    }

    /// Installs or idempotently confirms one exact, unexpired takeover lease.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, expired lease, or conflicting
    /// authority. A later revision of the same lease may replace an older one.
    pub fn install_takeover(
        &self,
        session_id: WorkerSessionId,
        lease: TerminalTakeoverLease,
    ) -> Result<(), SessionRegistryError> {
        if lease.revision == 0 || lease.expires_at <= unix_timestamp() {
            return Err(SessionRegistryError::TakeoverDenied);
        }
        self.get(session_id)?;
        let mut takeovers = lock(&self.takeovers)?;
        if let Some(current) = takeovers.get(&session_id).copied()
            && current.expires_at > unix_timestamp()
            && (current.lease_id != lease.lease_id || current.revision > lease.revision)
        {
            return Err(SessionRegistryError::TakeoverConflict);
        }
        takeovers.insert(session_id, lease);
        Ok(())
    }

    /// Writes only when the exact active takeover authority is installed.
    ///
    /// # Errors
    ///
    /// Returns an error for stale, conflicting, expired, or missing authority.
    pub fn write_takeover(
        &self,
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        bytes: &[u8],
    ) -> Result<(), SessionRegistryError> {
        self.require_takeover(session_id, lease_id, revision)?;
        self.get(session_id)?.write_input(bytes)
    }

    /// Atomically removes exact remote authority before accepting local input.
    /// Local reclaim therefore wins even if Keeper has not yet observed it.
    ///
    /// # Errors
    ///
    /// Returns an error for stale authority or terminal write failure.
    pub fn reclaim_takeover_and_write(
        &self,
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
        bytes: &[u8],
    ) -> Result<(), SessionRegistryError> {
        self.require_takeover(session_id, lease_id, revision)?;
        lock(&self.takeovers)?.remove(&session_id);
        self.get(session_id)?.write_input(bytes)
    }

    /// Releases exact takeover authority without writing terminal input.
    ///
    /// # Errors
    ///
    /// Returns an error for stale or missing authority.
    pub fn release_takeover(
        &self,
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
    ) -> Result<(), SessionRegistryError> {
        self.require_takeover(session_id, lease_id, revision)?;
        lock(&self.takeovers)?.remove(&session_id);
        Ok(())
    }

    /// Writes ordinary local or automation input only when remote takeover is
    /// absent or expired. Reclaim must use the explicit atomic operation.
    ///
    /// # Errors
    ///
    /// Returns an error while an unexpired takeover owns the session.
    pub fn write_local(
        &self,
        session_id: WorkerSessionId,
        bytes: &[u8],
    ) -> Result<(), SessionRegistryError> {
        let now = unix_timestamp();
        let mut takeovers = lock(&self.takeovers)?;
        if takeovers
            .get(&session_id)
            .is_some_and(|takeover| takeover.expires_at > now)
        {
            return Err(SessionRegistryError::TakeoverDenied);
        }
        takeovers.remove(&session_id);
        drop(takeovers);
        self.get(session_id)?.write_input(bytes)
    }

    fn require_takeover(
        &self,
        session_id: WorkerSessionId,
        lease_id: FederationStewardTakeoverLeaseId,
        revision: u64,
    ) -> Result<TerminalTakeoverLease, SessionRegistryError> {
        let now = unix_timestamp();
        let mut takeovers = lock(&self.takeovers)?;
        let Some(lease) = takeovers.get(&session_id).copied() else {
            return Err(SessionRegistryError::TakeoverDenied);
        };
        if lease.expires_at <= now {
            takeovers.remove(&session_id);
            return Err(SessionRegistryError::TakeoverDenied);
        }
        if lease.lease_id != lease_id || lease.revision != revision {
            return Err(SessionRegistryError::TakeoverDenied);
        }
        Ok(lease)
    }

    /// Returns the current bounded session count.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn len(&self) -> Result<usize, SessionRegistryError> {
        Ok(lock(&self.sessions)?.len())
    }

    /// Returns whether the registry is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn is_empty(&self) -> Result<bool, SessionRegistryError> {
        Ok(self.len()? == 0)
    }

    /// Returns immutable identities and current process state.
    ///
    /// # Errors
    ///
    /// Returns an error for a poisoned registry or failed OS process query.
    pub fn session_states(&self) -> Result<Vec<(WorkerSessionId, bool)>, SessionRegistryError> {
        let sessions = lock(&self.sessions)?;
        sessions
            .values()
            .map(|session| Ok((session.id(), session.is_running()?)))
            .collect()
    }

    /// Returns immutable identities, process state, and content-free process-tree resources.
    ///
    /// # Errors
    ///
    /// Returns an error for a poisoned registry or failed OS process query.
    pub fn session_resource_states(
        &self,
    ) -> Result<
        Vec<(WorkerSessionId, bool, Option<crate::ProcessResourceSample>)>,
        SessionRegistryError,
    > {
        let sessions = lock(&self.sessions)?;
        sessions
            .values()
            .map(|session| {
                Ok((
                    session.id(),
                    session.is_running()?,
                    session.resource_sample()?,
                ))
            })
            .collect()
    }

    /// Returns content-free durable-history diagnostics when history is
    /// enabled for this host.
    ///
    /// # Errors
    ///
    /// Returns an error if the history store lock is poisoned.
    pub fn history_diagnostics(&self) -> Result<Option<HistoryDiagnostics>, SessionRegistryError> {
        self.history
            .as_ref()
            .map(|history| history.diagnostics().map_err(SessionRegistryError::from))
            .transpose()
    }

    /// Lists durable terminal sessions when history is enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if history is disabled or unavailable.
    pub fn history_sessions(&self) -> Result<Vec<HistorySessionSummary>, SessionRegistryError> {
        self.history
            .as_ref()
            .ok_or_else(|| SessionRegistryError::Terminal("terminal history is disabled".into()))?
            .sessions()
            .map_err(SessionRegistryError::from)
    }

    /// Reads a bounded durable-history page.
    ///
    /// # Errors
    ///
    /// Returns an error if history is disabled, the session is unknown, or
    /// the store is unavailable.
    pub fn history_page(
        &self,
        id: WorkerSessionId,
        cursor: Option<HistoryCursor>,
    ) -> Result<HistoryPage, SessionRegistryError> {
        self.history
            .as_ref()
            .ok_or_else(|| SessionRegistryError::Terminal("terminal history is disabled".into()))?
            .page(id, cursor)
            .map_err(SessionRegistryError::from)
    }

    /// Atomically prevents new sessions and returns the number of still-running
    /// sessions that must drain before host replacement.
    ///
    /// # Errors
    ///
    /// Returns an error for a poisoned registry or failed process query.
    pub fn begin_drain(&self) -> Result<usize, SessionRegistryError> {
        let sessions = lock(&self.sessions)?;
        self.draining.store(true, Ordering::Release);
        running_sessions(&sessions)
    }

    /// Cancels a pending update drain and allows new sessions again.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry lock is poisoned.
    pub fn cancel_drain(&self) -> Result<(), SessionRegistryError> {
        let _sessions = lock(&self.sessions)?;
        self.draining.store(false, Ordering::Release);
        Ok(())
    }

    /// Returns whether new session creation is currently disabled for update.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }

    /// Returns the number of live provider processes. Exited sessions retained
    /// for status or history do not block host replacement.
    ///
    /// # Errors
    ///
    /// Returns an error for a poisoned registry or failed process query.
    pub fn running_session_count(&self) -> Result<usize, SessionRegistryError> {
        let sessions = lock(&self.sessions)?;
        running_sessions(&sessions)
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn running_sessions(
    sessions: &HashMap<WorkerSessionId, Arc<ProcessTerminalSession>>,
) -> Result<usize, SessionRegistryError> {
    sessions.values().try_fold(0, |count, session| {
        Ok(count + usize::from(session.is_running()?))
    })
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, SessionRegistryError> {
    mutex.lock().map_err(|_| SessionRegistryError::LockPoisoned)
}

fn terminal_error(error: impl std::fmt::Display) -> SessionRegistryError {
    SessionRegistryError::Terminal(error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        env,
        path::Path,
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{HistoryLimits, HistoryRecord};
    use tempfile::TempDir;

    fn shell_command(script: &str) -> ProviderCommand {
        ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), script.into()],
            working_directory: env::temp_dir(),
        }
    }

    #[test]
    fn only_claude_provider_executables_disable_the_alternate_screen() {
        assert!(is_claude_executable(Path::new("claude")));
        assert!(is_claude_executable(Path::new("/opt/bin/claude")));
        assert!(is_claude_executable(Path::new("C:/tools/claude.exe")));
        assert!(!is_claude_executable(Path::new("codex")));
        assert!(!is_claude_executable(Path::new("claude-helper")));
    }

    fn output_until(session: &ProcessTerminalSession, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let output = match session.resume_after(None).unwrap() {
                Resume::Snapshot { snapshot } => snapshot.bytes,
                Resume::Deltas { frames } => frames
                    .into_iter()
                    .flat_map(|frame| frame.bytes)
                    .collect::<Vec<_>>(),
            };
            let output = String::from_utf8_lossy(&output).into_owned();
            if output.contains(text) {
                return output;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {text:?}; output={output:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn takeover_authority_is_exact_bounded_and_local_reclaim_wins() {
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry =
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace]).expect("registry");
        let session = registry
            .spawn(
                &shell_command(
                    "read first; printf 'remote:%s\\n' \"$first\"; read second; printf 'local:%s\\n' \"$second\"",
                ),
                TerminalSize::default(),
            )
            .expect("session");
        let lease_id = FederationStewardTakeoverLeaseId::new();
        let lease = TerminalTakeoverLease {
            lease_id,
            revision: 2,
            expires_at: unix_timestamp() + 300,
        };
        registry
            .install_takeover(session.id(), lease)
            .expect("install");
        assert!(matches!(
            registry.write_local(session.id(), b"unsafe-local\n"),
            Err(SessionRegistryError::TakeoverDenied)
        ));
        assert!(matches!(
            registry.write_takeover(session.id(), lease_id, 1, b"stale\n"),
            Err(SessionRegistryError::TakeoverDenied)
        ));
        registry
            .write_takeover(session.id(), lease_id, 2, b"bounded\n")
            .expect("remote write");
        assert!(output_until(&session, "remote:bounded").contains("remote:bounded"));

        registry
            .reclaim_takeover_and_write(session.id(), lease_id, 2, b"returned\n")
            .expect("local reclaim");
        assert!(output_until(&session, "local:returned").contains("local:returned"));
        assert!(matches!(
            registry.write_takeover(session.id(), lease_id, 2, b"too-late\n"),
            Err(SessionRegistryError::TakeoverDenied)
        ));
    }

    #[test]
    fn captures_real_pty_output_with_sequences() {
        let session = ProcessTerminalSession::spawn(
            WorkerSessionId::new(),
            &shell_command("printf first; printf second"),
            JournalLimits::new(1024, 16),
            TerminalSize::default(),
        )
        .unwrap();
        assert!(output_until(&session, "firstsecond").contains("firstsecond"));
    }

    #[test]
    fn provider_pty_declares_color_terminal_capabilities() {
        let session = ProcessTerminalSession::spawn(
            WorkerSessionId::new(),
            &shell_command(
                "printf '%s|%s|%s|%s|%s' \"$TERM\" \"$COLORTERM\" \"$FORCE_COLOR\" \"$CLICOLOR_FORCE\" \"${NO_COLOR-unset}\"",
            ),
            JournalLimits::new(1024, 16),
            TerminalSize::default(),
        )
        .unwrap();

        assert!(
            output_until(&session, "xterm-256color|truecolor|3|1|unset")
                .contains("xterm-256color|truecolor|3|1|unset")
        );
    }

    #[test]
    fn real_pty_output_is_persisted_at_the_canonical_sequence() {
        let temp = TempDir::new().unwrap();
        let history = Arc::new(
            HistoryStore::open(temp.path().join("history"), HistoryLimits::default()).unwrap(),
        );
        let registry = SessionRegistry::new_with_history(
            JournalLimits::new(1024, 16),
            1,
            [env::temp_dir().canonicalize().unwrap()],
            Some(Arc::clone(&history)),
        )
        .unwrap();
        let session = registry
            .spawn(
                &shell_command("printf 'durable-output'"),
                TerminalSize::default(),
            )
            .unwrap();
        assert!(output_until(&session, "durable-output").contains("durable-output"));

        let records = history.read_session(session.id()).unwrap();
        assert!(!records.is_empty());
        let Resume::Snapshot { snapshot } = session.resume_after(None).unwrap() else {
            panic!("fresh read must return a canonical snapshot");
        };
        assert_eq!(records.last().unwrap().sequence(), snapshot.sequence);
        assert!(
            String::from_utf8_lossy(
                &records
                    .into_iter()
                    .flat_map(|record| match record {
                        HistoryRecord::Output { bytes, .. } => bytes,
                        HistoryRecord::Checkpoint { snapshot, .. } => snapshot.bytes,
                    })
                    .collect::<Vec<_>>()
            )
            .contains("durable-output")
        );
    }

    #[test]
    fn writes_input_and_stops_explicitly() {
        let session = ProcessTerminalSession::spawn(
            WorkerSessionId::new(),
            &shell_command("read value; printf 'received:%s' \"$value\""),
            JournalLimits::new(1024, 16),
            TerminalSize::default(),
        )
        .unwrap();
        session.write_input(b"hello\n").unwrap();
        assert!(output_until(&session, "received:hello").contains("received:hello"));
        session.stop().unwrap();
    }

    #[test]
    fn resize_updates_pty_and_canonical_dimensions() {
        let session = ProcessTerminalSession::spawn(
            WorkerSessionId::new(),
            &shell_command("sleep 5"),
            JournalLimits::new(1024, 16),
            TerminalSize::default(),
        )
        .unwrap();
        let resized = TerminalSize::new(41, 154);

        session.resize(resized).unwrap();

        let Resume::Snapshot { snapshot } = session.resume_after(None).unwrap() else {
            panic!("fresh attachment must produce a canonical snapshot");
        };
        assert_eq!(
            (snapshot.rows, snapshot.columns),
            (resized.rows, resized.columns)
        );
        session.stop().unwrap();
    }

    #[tokio::test]
    async fn wait_wakes_for_output_without_polling() {
        let session = Arc::new(
            ProcessTerminalSession::spawn(
                WorkerSessionId::new(),
                &shell_command("read value; printf 'event:%s' \"$value\""),
                JournalLimits::new(1024, 16),
                TerminalSize::default(),
            )
            .unwrap(),
        );
        let waiting_session = Arc::clone(&session);
        let initial_sequence = match session.resume_after(None).unwrap() {
            Resume::Snapshot { snapshot } => snapshot.sequence,
            Resume::Deltas { .. } => panic!("fresh attachment must produce a canonical snapshot"),
        };
        let waiter =
            tokio::spawn(async move { waiting_session.wait_after(Some(initial_sequence)).await });

        tokio::task::yield_now().await;
        session.write_input(b"ready\n").unwrap();

        let first = tokio::time::timeout(Duration::from_secs(3), waiter)
            .await
            .expect("event-driven wait timed out")
            .unwrap()
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut next = Some(first);
        let mut sequence = initial_sequence;
        let mut output = Vec::new();
        loop {
            let (resume, _) = match next.take() {
                Some(resume) => resume,
                None => tokio::time::timeout_at(deadline, session.wait_after(Some(sequence)))
                    .await
                    .expect("event-driven follow-up timed out")
                    .unwrap(),
            };
            let Resume::Deltas { frames } = resume else {
                panic!("expected retained deltas");
            };
            for frame in frames {
                sequence = frame.sequence;
                output.extend(frame.bytes);
            }
            if String::from_utf8_lossy(&output).contains("event:ready") {
                break;
            }
        }
        session.stop().unwrap();
    }

    #[test]
    fn enforces_workspace_and_session_bounds() {
        let root = env::temp_dir().canonicalize().unwrap();
        let registry = SessionRegistry::new(JournalLimits::new(1024, 16), 1, [root]).unwrap();
        let first = registry
            .spawn(&shell_command("sleep 5"), TerminalSize::default())
            .unwrap();
        assert!(matches!(
            registry.spawn(&shell_command("sleep 5"), TerminalSize::default()),
            Err(SessionRegistryError::SessionLimitReached { limit: 1 })
        ));
        registry.stop(first.id()).unwrap();
        assert!(registry.is_empty().unwrap());
    }

    #[test]
    fn explicit_root_override_still_requires_a_real_non_root_workspace() {
        let allowed = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let registry = SessionRegistry::new(
            JournalLimits::new(1024, 16),
            1,
            [allowed.path().canonicalize().unwrap()],
        )
        .unwrap();
        let mut command = shell_command("printf outside-root");
        command.working_directory = outside.path().to_path_buf();

        assert!(matches!(
            registry.spawn(&command, TerminalSize::default()),
            Err(SessionRegistryError::WorkspaceNotAllowed(_))
        ));
        let session = registry
            .spawn_with_root_override(&command, TerminalSize::default(), true)
            .unwrap();
        assert!(output_until(&session, "outside-root").contains("outside-root"));
        session.stop().unwrap();
    }

    #[test]
    fn drain_rejects_new_sessions_without_disrupting_existing_worker() {
        let root = env::temp_dir().canonicalize().unwrap();
        let registry = SessionRegistry::new(JournalLimits::new(1024, 16), 2, [root]).unwrap();
        let existing = registry
            .spawn(
                &shell_command("read value; printf 'during-drain:%s' \"$value\""),
                TerminalSize::default(),
            )
            .unwrap();

        assert_eq!(registry.begin_drain().unwrap(), 1);
        assert!(registry.is_draining());
        assert!(matches!(
            registry.spawn(&shell_command("sleep 1"), TerminalSize::default()),
            Err(SessionRegistryError::HostDraining)
        ));
        existing.write_input(b"preserved\n").unwrap();
        assert!(
            output_until(&existing, "during-drain:preserved").contains("during-drain:preserved")
        );

        registry.cancel_drain().unwrap();
        let replacement = registry
            .spawn(&shell_command("sleep 1"), TerminalSize::default())
            .unwrap();
        registry.stop(existing.id()).unwrap();
        registry.stop(replacement.id()).unwrap();
    }

    #[test]
    fn exited_sessions_do_not_block_drain_readiness() {
        let root = env::temp_dir().canonicalize().unwrap();
        let registry = SessionRegistry::new(JournalLimits::new(1024, 16), 1, [root]).unwrap();
        let session = registry
            .spawn(&shell_command("printf finished"), TerminalSize::default())
            .unwrap();
        assert!(output_until(&session, "finished").contains("finished"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.is_running().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(registry.begin_drain().unwrap(), 0);
        assert_eq!(registry.len().unwrap(), 1);
    }

    #[test]
    fn rejects_terminal_dimensions_outside_memory_bounds_before_spawn() {
        let result = ProcessTerminalSession::spawn(
            WorkerSessionId::new(),
            &shell_command("sleep 1"),
            JournalLimits::new(1024, 16),
            TerminalSize::new(MAX_TERMINAL_ROWS + 1, 80),
        );
        assert!(matches!(result, Err(SessionRegistryError::Terminal(_))));
    }
}
