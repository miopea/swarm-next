use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use swarm_domain::{
    ConversationRecoveryAttempt, FederationStewardTakeoverLeaseId, ProviderKind,
    TerminalControlError, TerminalControlGrant, TerminalControlIdentity, WorkerSessionId,
};
use thiserror::Error;
use tokio::sync::watch;
use tracing::warn;

use crate::control_gate::{ControlGateError, TerminalControlGate};

use crate::{
    CanonicalTerminalState, HistoryAppendOutcome, HistoryCursor, HistoryDiagnostics, HistoryError,
    HistoryPage, HistorySessionSummary, HistoryStore, JournalLimits, ProviderActivity,
    ProviderCommand, Resume, TerminalTakeoverLease, TerminalWriteAuditEntry,
    TerminalWriteProvenance, TerminalWriteResult, classify_provider_activity,
};

pub const MAX_TERMINAL_ROWS: u16 = 200;
pub const MAX_TERMINAL_COLUMNS: u16 = 320;
pub const MAX_TERMINAL_CELLS: usize = 32_000;
pub const MIN_TERMINAL_ROWS: u16 = 4;
pub const MIN_TERMINAL_COLUMNS: u16 = 20;
const MAX_WRITE_AUDIT_ENTRIES: usize = 10_000;
const WRITE_AUDIT_RETENTION_SECONDS: i64 = 24 * 60 * 60;

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
    #[error("terminal control refused: {0:?}")]
    ControlDenied(TerminalControlError),
    #[error("this terminal requires generation-bound control")]
    ControlGenerationRequired,
    #[error("terminal input must contain between 1 and 65536 bytes")]
    InvalidControlInput,
    #[error("terminal operation failed: {0}")]
    Terminal(String),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("terminal session lock was poisoned")]
    LockPoisoned,
}

/// What the host knows about one live terminal without reading its contents.
#[derive(Clone, Copy, Debug)]
pub struct SessionResourceState {
    pub session_id: WorkerSessionId,
    pub running: bool,
    pub resources: Option<crate::ProcessResourceSample>,
    /// Wall-clock second of this terminal's most recent output.
    pub last_output_at: i64,
    pub recovery_attempt: Option<ConversationRecoveryAttempt>,
    pub provider_start: Option<crate::ProviderSessionStartObservation>,
    pub provider_selection: Option<swarm_domain::ProviderConversationSelection>,
}

/// Seconds since the Unix epoch, saturating rather than panicking on a clock
/// set before it.
fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs().cast_signed())
}

pub struct ProcessTerminalSession {
    id: WorkerSessionId,
    recovery_attempt: OnceLock<ConversationRecoveryAttempt>,
    provider_lifecycle: Mutex<Option<crate::ProviderLifecycleGate>>,
    control: TerminalControlGate,
    control_changes: watch::Sender<()>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    terminal_state: Arc<Mutex<CanonicalTerminalState>>,
    output_state: watch::Sender<bool>,
    reader_running: Arc<AtomicBool>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    history: Option<Arc<HistoryStore>>,
    /// Wall-clock second of the most recent PTY output. The reader already sees
    /// every byte, so this is the one owner of how long a worker has been
    /// silent; nothing has to poll a terminal to find out.
    last_output_at: Arc<AtomicI64>,
    /// Which provider this session is running, when that is known.
    ///
    /// `None` for a shell, which carries no agent and is deliberately excluded
    /// from activity classification — see `HostRequest::StartShell`. The host
    /// knows the provider from the request variant it handled, so this is
    /// recorded rather than guessed from the executable name.
    provider: Option<ProviderKind>,
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
        Self::spawn_with_history(id, command, limits, size, None, None)
    }

    fn spawn_with_history(
        id: WorkerSessionId,
        command: &ProviderCommand,
        limits: JournalLimits,
        size: TerminalSize,
        history: Option<Arc<HistoryStore>>,
        provider: Option<ProviderKind>,
    ) -> Result<Self, SessionRegistryError> {
        size.validate()?;
        let pair = native_pty_system()
            .openpty(size.as_pty_size())
            .map_err(terminal_error)?;
        let mut command_builder = CommandBuilder::new(&command.executable);
        command_builder.args(&command.arguments);
        command_builder.cwd(&command.working_directory);
        let provider_lifecycle = configure_provider_lifecycle(id, provider, &mut command_builder)?;
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
        let last_output_at = Arc::new(AtomicI64::new(unix_seconds()));
        let reader_last_output_at = Arc::clone(&last_output_at);
        let reader_thread = thread::Builder::new()
            .name(format!("terminal-reader-{id:?}"))
            .spawn(move || {
                let mut buffer = vec![0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let output = &buffer[..read];
                            reader_last_output_at.store(unix_seconds(), Ordering::Release);
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
            control: TerminalControlGate::default(),
            recovery_attempt: OnceLock::new(),
            provider_lifecycle: Mutex::new(provider_lifecycle),
            control_changes: watch::channel(()).0,
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            terminal_state,
            output_state,
            reader_running,
            reader_thread: Mutex::new(Some(reader_thread)),
            history,
            last_output_at,
            provider,
        })
    }

    #[must_use]
    pub const fn id(&self) -> WorkerSessionId {
        self.id
    }

    /// Records startup provenance, not evidence of restored provider context.
    /// Owned by this process incarnation and retained across browser/API reloads.
    /// Returns false if a caller tries to replace already recorded provenance.
    pub fn record_recovery_attempt(&self, attempt: ConversationRecoveryAttempt) -> bool {
        self.recovery_attempt.set(attempt).is_ok()
    }

    #[must_use]
    pub fn recovery_attempt(&self) -> Option<ConversationRecoveryAttempt> {
        self.recovery_attempt.get().copied()
    }

    /// Returns retained startup evidence, not a claim of current process liveness
    /// or permission to replace a worker's durable conversation binding.
    ///
    /// # Errors
    /// Returns an error if the lifecycle lock is poisoned.
    pub fn provider_start(
        &self,
    ) -> Result<Option<crate::ProviderSessionStartObservation>, SessionRegistryError> {
        Ok(lock(&self.provider_lifecycle)?
            .as_ref()
            .and_then(crate::ProviderLifecycleGate::observation))
    }

    /// Returns the latest accepted selection revision for this process.
    /// # Errors
    /// Returns an error if the lifecycle lock is poisoned.
    pub fn provider_selection(
        &self,
    ) -> Result<Option<swarm_domain::ProviderConversationSelection>, SessionRegistryError> {
        Ok(lock(&self.provider_lifecycle)?
            .as_ref()
            .and_then(crate::ProviderLifecycleGate::selection))
    }

    /// Arms an interactive-resume boundary only for this still-live process.
    /// # Errors
    /// Returns lock or process-status errors without exposing capability data.
    pub fn observe_resume_end(
        &self,
        capability: &[u8; 32],
        previous: swarm_domain::ProviderConversationId,
    ) -> Result<bool, SessionRegistryError> {
        let mut child = lock(&self.child)?;
        let mut gate = lock(&self.provider_lifecycle)?;
        let Some(gate) = gate.as_mut() else {
            return Ok(false);
        };
        if child.try_wait().map_err(terminal_error)?.is_some() {
            gate.revoke();
            return Ok(false);
        }
        Ok(gate.begin_resume(self.id, capability, previous))
    }

    /// Accepts startup evidence only while the bound provider process is live.
    ///
    /// # Errors
    /// Returns lock or process-status errors, without exposing the capability.
    pub fn observe_provider_start(
        &self,
        capability: &[u8; 32],
        observation: crate::ProviderSessionStartObservation,
    ) -> Result<crate::ProviderLifecycleAcceptance, SessionRegistryError> {
        let mut child = lock(&self.child)?;
        let mut gate = lock(&self.provider_lifecycle)?;
        let Some(gate) = gate.as_mut() else {
            return Ok(crate::ProviderLifecycleAcceptance::Denied);
        };
        if child.try_wait().map_err(terminal_error)?.is_some() {
            gate.revoke();
            return Ok(crate::ProviderLifecycleAcceptance::Denied);
        }
        Ok(gate.observe(self.id, capability, observation))
    }

    /// Writes input directly to the PTY master.
    ///
    /// # Errors
    ///
    /// Returns an error when the session lock is poisoned or the PTY rejects
    /// the write.
    pub fn write_input(&self, bytes: &[u8]) -> Result<(), SessionRegistryError> {
        self.control
            .legacy(false, || self.write_input_unchecked(bytes))
            .map_err(control_error)
    }

    fn write_input_unchecked(&self, bytes: &[u8]) -> Result<(), SessionRegistryError> {
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
        self.control
            .legacy(false, || self.resize_unchecked(size))
            .map_err(control_error)
    }

    fn resize_unchecked(&self, size: TerminalSize) -> Result<(), SessionRegistryError> {
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

    /// Current engine-owned control revision and live owner. Reading never renews.
    ///
    /// # Errors
    /// Fails closed if the session control lock is poisoned.
    pub fn control_status(
        &self,
    ) -> Result<(u64, Option<TerminalControlGrant>), SessionRegistryError> {
        self.control.status().map_err(control_error)
    }

    /// A wire-safe snapshot with remaining duration instead of a private epoch.
    ///
    /// # Errors
    /// Fails closed on a poisoned control lock.
    pub fn control_wire_status(
        &self,
    ) -> Result<crate::TerminalControlStatus, SessionRegistryError> {
        let (generation, owner, now) = self.control.snapshot().map_err(control_error)?;
        Ok(crate::TerminalControlStatus {
            generation,
            owner: owner.map(|grant| grant.identity),
            lease_remaining_ms: owner.map_or(0, |grant| grant.expires_at_ms.saturating_sub(now)),
        })
    }

    /// Acquires an unowned view, or explicitly takes over the observed revision.
    /// Geometry and ownership commit under one session guard; no input is sent.
    ///
    /// # Errors
    /// Refuses competing/stale ownership, invalid geometry, or PTY failure.
    pub fn claim_control(
        &self,
        identity: TerminalControlIdentity,
        observed_generation: Option<u64>,
        size: TerminalSize,
    ) -> Result<TerminalControlGrant, SessionRegistryError> {
        let grant = self
            .control
            .claim(identity, observed_generation, || {
                self.resize_unchecked(size)
            })
            .map_err(control_error)?;
        self.control_changes.send_replace(());
        Ok(grant)
    }

    /// Renews a confirmed foreground view, without changing geometry or input.
    ///
    /// # Errors
    /// Refuses missing, stale, or expired control.
    pub fn renew_control(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
    ) -> Result<TerminalControlGrant, SessionRegistryError> {
        self.control
            .renew(identity, generation)
            .map_err(control_error)
    }

    /// Releases only this exact live owner; disconnect must not call this.
    ///
    /// # Errors
    /// Refuses missing, stale, or expired control.
    pub fn release_control(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
    ) -> Result<(), SessionRegistryError> {
        self.control
            .release(identity, generation)
            .map_err(control_error)?;
        self.control_changes.send_replace(());
        Ok(())
    }

    /// Changes geometry only while this exact view still owns the generation.
    ///
    /// # Errors
    /// Refuses stale control, invalid dimensions, poisoned locks, or PTY failure.
    pub fn resize_controlled(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
        size: TerminalSize,
    ) -> Result<(), SessionRegistryError> {
        self.control
            .resize(identity, generation, || self.resize_unchecked(size))
            .map_err(control_error)
    }

    fn write_controlled(
        &self,
        identity: TerminalControlIdentity,
        generation: u64,
        bytes: &[u8],
    ) -> Result<(), SessionRegistryError> {
        self.control
            .input(identity, generation, || self.write_input_unchecked(bytes))
            .map_err(control_error)
    }

    fn write_coordination(&self, bytes: &[u8]) -> Result<(), SessionRegistryError> {
        self.control
            .legacy(true, || self.write_input_unchecked(bytes))
            .map_err(control_error)
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

    /// Waits for bytes, a control transition, process exit, or lease expiry.
    /// Subscription precedes observation; no periodic poll establishes truth.
    ///
    /// # Errors
    /// Fails if terminal/control state or an owned notification channel fails.
    pub async fn wait_controlled_after(
        &self,
        sequence: Option<u64>,
        after_control: crate::TerminalControlCursor,
    ) -> Result<(Resume, bool, crate::TerminalControlStatus), SessionRegistryError> {
        let mut output = self.output_state.subscribe();
        let mut controls = self.control_changes.subscribe();
        loop {
            let running = *output.borrow_and_update();
            drop(controls.borrow_and_update());
            let control = self.control_wire_status()?;
            let resume = self.resume_after(sequence)?;
            if control.cursor() != after_control || resume_has_output(&resume) || !running {
                return Ok((resume, running, control));
            }
            if control.owner.is_some() {
                // This is the authoritative lease deadline, not a guessed delay.
                // Renewal may extend it; waking at the previous deadline simply
                // rechecks state without revoking or publishing a false expiry.
                tokio::select! {
                    result = output.changed() => result.map_err(terminal_error)?,
                    result = controls.changed() => result.map_err(terminal_error)?,
                    () = tokio::time::sleep(Duration::from_millis(control.lease_remaining_ms)) => {},
                }
            } else {
                tokio::select! {
                    result = output.changed() => result.map_err(terminal_error)?,
                    result = controls.changed() => result.map_err(terminal_error)?,
                }
            }
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

    /// Wall-clock second of the most recent output this terminal produced.
    #[must_use]
    pub fn last_output_at(&self) -> i64 {
        self.last_output_at.load(Ordering::Acquire)
    }

    /// Stops the child process explicitly.
    ///
    /// # Errors
    ///
    /// Returns an error when the child lock is poisoned or termination fails.
    pub fn stop(&self) -> Result<(), SessionRegistryError> {
        let mut child = lock(&self.child)?;
        if let Some(gate) = lock(&self.provider_lifecycle)?.as_mut() {
            gate.revoke();
        }
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

fn configure_provider_lifecycle(
    session: WorkerSessionId,
    provider: Option<ProviderKind>,
    command: &mut CommandBuilder,
) -> Result<Option<crate::ProviderLifecycleGate>, SessionRegistryError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    command.env_remove("SWARM_PROVIDER_SESSION");
    command.env_remove("SWARM_PROVIDER_START_CAPABILITY");
    if provider != Some(ProviderKind::ClaudeCode) {
        return Ok(None);
    }
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).map_err(|_| {
        SessionRegistryError::Terminal("provider startup entropy unavailable".into())
    })?;
    let encoded: String = secret
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect();
    command.env("SWARM_PROVIDER_SESSION", session.to_string());
    command.env("SWARM_PROVIDER_START_CAPABILITY", encoded);
    Ok(Some(crate::ProviderLifecycleGate::new(session, secret)))
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
    write_audit: Mutex<VecDeque<TerminalWriteAuditEntry>>,
    next_write_audit_sequence: Mutex<u64>,
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
            write_audit: Mutex::new(VecDeque::new()),
            next_write_audit_sequence: Mutex::new(1),
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

    /// Spawns a session and RECORDS which provider it is, so the host can later
    /// say whether that session is mid-turn.
    ///
    /// The host knows this from the request variant it handled — `StartClaude`,
    /// `StartCodex`, `StartAlphaProvider` — so nothing here infers a provider
    /// from an executable name. `None` is for a shell, which has no agent and
    /// is excluded from classification by design.
    ///
    /// # Errors
    ///
    /// As [`SessionRegistry::spawn_with_root_override`].
    pub fn spawn_provider_session(
        &self,
        command: &ProviderCommand,
        size: TerminalSize,
        allow_outside_roots: bool,
        provider: Option<ProviderKind>,
    ) -> Result<Arc<ProcessTerminalSession>, SessionRegistryError> {
        self.spawn_inner(command, size, allow_outside_roots, provider)
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
        self.spawn_inner(command, size, allow_outside_roots, None)
    }

    fn spawn_inner(
        &self,
        command: &ProviderCommand,
        size: TerminalSize,
        allow_outside_roots: bool,
        provider: Option<ProviderKind>,
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

        // Held through process spawn and registration. A startup hook's get()
        // waits for insertion rather than observing an unregistered live child.
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
            provider,
        )?);
        sessions.insert(id, Arc::clone(&session));
        Ok(session)
    }

    /// Counts live sessions that are mid-turn, and those that cannot be read.
    ///
    /// # Errors
    ///
    /// Returns an error when a lock is poisoned or a child cannot be polled.
    pub fn activity_census(&self) -> Result<SessionActivityCensus, SessionRegistryError> {
        let sessions = lock(&self.sessions)?;
        activity_census(&sessions)
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
        if self.get(session_id)?.control_status()?.1.is_some() {
            return Err(SessionRegistryError::TakeoverConflict);
        }
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
        self.audit_write(
            session_id,
            TerminalWriteProvenance::steward(lease_id, bytes),
            bytes,
            || {
                self.require_takeover(session_id, lease_id, revision)?;
                self.get(session_id)?.write_input(bytes)
            },
        )
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
        self.audit_write(
            session_id,
            TerminalWriteProvenance::operator(None, bytes),
            bytes,
            || {
                self.require_takeover(session_id, lease_id, revision)?;
                lock(&self.takeovers)?.remove(&session_id);
                self.get(session_id)?.write_input(bytes)
            },
        )
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
        provenance: TerminalWriteProvenance,
    ) -> Result<(), SessionRegistryError> {
        self.audit_write(session_id, provenance, bytes, || {
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
            let session = self.get(session_id)?;
            if matches!(
                provenance.actor,
                crate::TerminalWriteActor::SwarmCoordination
            ) {
                session.write_coordination(bytes)
            } else {
                session.write_input(bytes)
            }
        })
    }

    /// Claims are routed through the registry so a remote takeover cannot appear
    /// between checking that boundary and committing local geometry/ownership.
    ///
    /// # Errors
    /// Refuses active remote authority, stale control, invalid size, or PTY failure.
    pub fn claim_control(
        &self,
        session_id: WorkerSessionId,
        identity: TerminalControlIdentity,
        observed_generation: Option<u64>,
        size: TerminalSize,
    ) -> Result<TerminalControlGrant, SessionRegistryError> {
        let mut takeovers = lock(&self.takeovers)?;
        if takeovers
            .get(&session_id)
            .is_some_and(|lease| lease.expires_at > unix_timestamp())
        {
            return Err(SessionRegistryError::TakeoverDenied);
        }
        takeovers.remove(&session_id);
        self.get(session_id)?
            .claim_control(identity, observed_generation, size)
    }

    /// Audited operator input whose generation is checked through the PTY write.
    ///
    /// # Errors
    /// Refuses active remote takeover, stale control, or terminal failure. A write
    /// failure may have accepted a prefix and must never be retried automatically.
    pub fn write_controlled(
        &self,
        session_id: WorkerSessionId,
        identity: TerminalControlIdentity,
        generation: u64,
        bytes: &[u8],
    ) -> Result<(), SessionRegistryError> {
        if bytes.is_empty() || bytes.len() > crate::MAX_CONTROL_INPUT_BYTES {
            return Err(SessionRegistryError::InvalidControlInput);
        }
        self.audit_write(
            session_id,
            TerminalWriteProvenance::operator(Some(identity.device), bytes),
            bytes,
            || {
                let mut takeovers = lock(&self.takeovers)?;
                if takeovers
                    .get(&session_id)
                    .is_some_and(|lease| lease.expires_at > unix_timestamp())
                {
                    return Err(SessionRegistryError::TakeoverDenied);
                }
                takeovers.remove(&session_id);
                // Keep the takeover lock until the session guard has checked and
                // executed the write. Remote authority cannot appear in that gap.
                self.get(session_id)?
                    .write_controlled(identity, generation, bytes)
            },
        )
    }

    /// Returns the newest bounded, content-free terminal write records.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero or oversized page or a poisoned audit lock.
    pub fn recent_write_audit(
        &self,
        limit: u16,
    ) -> Result<Vec<TerminalWriteAuditEntry>, SessionRegistryError> {
        if limit == 0 || limit > crate::MAX_WRITE_AUDIT_PAGE {
            return Err(SessionRegistryError::Terminal(format!(
                "terminal write audit limit must be between 1 and {}",
                crate::MAX_WRITE_AUDIT_PAGE
            )));
        }
        let now = unix_timestamp();
        let mut audit = lock(&self.write_audit)?;
        prune_write_audit(&mut audit, now);
        Ok(audit
            .iter()
            .rev()
            .take(usize::from(limit))
            .cloned()
            .collect())
    }

    fn audit_write(
        &self,
        session_id: WorkerSessionId,
        provenance: TerminalWriteProvenance,
        bytes: &[u8],
        write: impl FnOnce() -> Result<(), SessionRegistryError>,
    ) -> Result<(), SessionRegistryError> {
        let now = unix_timestamp();
        let mut audit = lock(&self.write_audit)?;
        prune_write_audit(&mut audit, now);
        let result = write();
        let mut sequence = lock(&self.next_write_audit_sequence)?;
        audit.push_back(TerminalWriteAuditEntry {
            sequence: *sequence,
            session_id,
            actor: provenance.actor,
            input_kind: provenance.input_kind,
            byte_count: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            result: if result.is_ok() {
                TerminalWriteResult::Acknowledged
            } else {
                TerminalWriteResult::Rejected
            },
            occurred_at: now,
        });
        *sequence = sequence.saturating_add(1);
        while audit.len() > MAX_WRITE_AUDIT_ENTRIES {
            audit.pop_front();
        }
        result
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
    ) -> Result<Vec<SessionResourceState>, SessionRegistryError> {
        let sessions = lock(&self.sessions)?;
        sessions
            .values()
            .map(|session| {
                Ok(SessionResourceState {
                    session_id: session.id(),
                    running: session.is_running()?,
                    resources: session.resource_sample()?,
                    last_output_at: session.last_output_at(),
                    recovery_attempt: session.recovery_attempt(),
                    provider_start: session.provider_start()?,
                    provider_selection: session.provider_selection()?,
                })
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

fn prune_write_audit(audit: &mut VecDeque<TerminalWriteAuditEntry>, now: i64) {
    let oldest = now.saturating_sub(WRITE_AUDIT_RETENTION_SECONDS);
    while audit
        .front()
        .is_some_and(|entry| entry.occurred_at < oldest)
    {
        audit.pop_front();
    }
}

/// How many live sessions are mid-turn, and how many cannot be read.
///
/// THE COUNT OF SESSIONS WAS NEVER THE QUESTION. Autostart guarantees sessions
/// exist, so `running_sessions` is a constant rather than a reading — which is
/// why the reconcile deferred forever on it. This answers the question that was
/// actually being asked: is anyone doing something losable.
///
/// Three outcomes rather than two, because "I cannot tell" is a real answer and
/// collapsing it into either of the others is what makes a safety check lie.
/// Shells are in neither count: they carry no agent, and classifying one would
/// invent a busy worker out of a person's prompt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionActivityCensus {
    /// Active or awaiting an operator. Stopping one of these loses work.
    pub busy: usize,
    /// A provider whose screen this build cannot classify. Not safe to treat as
    /// idle, and not a reason to defer forever either.
    pub unreadable: usize,
}

fn activity_census(
    sessions: &HashMap<WorkerSessionId, Arc<ProcessTerminalSession>>,
) -> Result<SessionActivityCensus, SessionRegistryError> {
    let mut census = SessionActivityCensus::default();
    for session in sessions.values() {
        if !session.is_running()? {
            continue;
        }
        let Some(provider) = session.provider else {
            continue;
        };
        let snapshot = lock(&session.terminal_state)?.snapshot();
        match classify_provider_activity(provider, &snapshot) {
            ProviderActivity::Active | ProviderActivity::AwaitingOperator => census.busy += 1,
            ProviderActivity::Unknown => census.unreadable += 1,
            ProviderActivity::Resting => {}
        }
    }
    Ok(census)
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

fn control_error<E: std::fmt::Display>(error: ControlGateError<E>) -> SessionRegistryError {
    match error {
        ControlGateError::Authority(reason) => SessionRegistryError::ControlDenied(reason),
        ControlGateError::GenerationRequired => SessionRegistryError::ControlGenerationRequired,
        ControlGateError::Poisoned => SessionRegistryError::LockPoisoned,
        ControlGateError::Effect(error) => terminal_error(error),
    }
}

fn terminal_error(error: impl std::fmt::Display) -> SessionRegistryError {
    SessionRegistryError::Terminal(error.to_string())
}

#[cfg(all(test, any(unix, windows)))]
pub(crate) mod control_tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, TerminalViewId};

    fn identity() -> TerminalControlIdentity {
        TerminalControlIdentity {
            device: PresenceDeviceId::new(),
            view: TerminalViewId::new(),
        }
    }

    pub(crate) fn fixture() -> (SessionRegistry, Arc<ProcessTerminalSession>) {
        let workspace = std::env::temp_dir();
        let registry =
            SessionRegistry::new(JournalLimits::default(), 1, [workspace.clone()]).unwrap();
        let (executable, arguments) = if cfg!(windows) {
            (
                PathBuf::from(
                    std::env::var_os("COMSPEC")
                        .unwrap_or_else(|| "C:/Windows/System32/cmd.exe".into()),
                ),
                vec!["/D".into(), "/Q".into()],
            )
        } else {
            (PathBuf::from("/bin/sh"), vec!["-c".into(), "cat".into()])
        };
        let command = ProviderCommand {
            executable,
            arguments,
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::new(24, 80)).unwrap();
        (registry, session)
    }

    fn size(session: &ProcessTerminalSession) -> TerminalSize {
        let Resume::Snapshot { snapshot } = session.resume_after(None).unwrap() else {
            panic!("fresh snapshot required")
        };
        TerminalSize::new(snapshot.rows, snapshot.columns)
    }

    #[test]
    fn real_pty_handoff_preserves_process_and_rejects_previous_view() {
        let (registry, session) = fixture();
        let id = session.id();
        let desktop = identity();
        let phone = identity();
        let original = session
            .claim_control(desktop, None, TerminalSize::new(24, 100))
            .unwrap();
        assert!(
            session
                .claim_control(phone, None, TerminalSize::new(40, 36))
                .is_err()
        );
        assert_eq!(size(&session), TerminalSize::new(24, 100));
        let transferred = session
            .claim_control(phone, Some(original.generation), TerminalSize::new(40, 36))
            .unwrap();
        assert_eq!(size(&session), TerminalSize::new(40, 36));
        assert!(
            session
                .resize_controlled(desktop, original.generation, TerminalSize::new(24, 100))
                .is_err()
        );
        assert!(
            registry
                .write_controlled(id, desktop, original.generation, b"stale")
                .is_err()
        );
        assert!(session.resize(TerminalSize::new(24, 100)).is_err());
        assert!(
            registry
                .write_local(
                    id,
                    b"legacy",
                    TerminalWriteProvenance::operator(Some(desktop.device), b"legacy")
                )
                .is_err()
        );
        assert_eq!(size(&session), TerminalSize::new(40, 36));
        registry
            .write_controlled(
                id,
                phone,
                transferred.generation,
                b"echo SWARM_CONTROL_OK\r\n",
            )
            .unwrap();
        let audit = registry.recent_write_audit(10).unwrap();
        assert_eq!(audit[0].result, TerminalWriteResult::Acknowledged);
        assert_eq!(audit[1].result, TerminalWriteResult::Rejected);
        assert_eq!(audit[2].result, TerminalWriteResult::Rejected);
        assert_eq!(session.id(), id);
        assert!(session.is_running().unwrap());
        session
            .release_control(phone, transferred.generation)
            .unwrap();
        assert!(session.is_running().unwrap());
    }

    #[test]
    fn invalid_handoff_geometry_leaves_current_owner_and_pty_unchanged() {
        let (_registry, session) = fixture();
        let desktop = identity();
        let original = session
            .claim_control(desktop, None, TerminalSize::new(24, 100))
            .unwrap();
        assert!(
            session
                .claim_control(
                    identity(),
                    Some(original.generation),
                    TerminalSize::new(0, 0)
                )
                .is_err()
        );
        assert_eq!(
            session.control_status().unwrap(),
            (original.generation, Some(original))
        );
        assert_eq!(size(&session), TerminalSize::new(24, 100));
        session
            .resize_controlled(desktop, original.generation, TerminalSize::new(25, 100))
            .unwrap();
        assert_eq!(size(&session), TerminalSize::new(25, 100));
    }

    #[tokio::test]
    async fn control_changes_are_observable_without_requiring_terminal_output() {
        let (registry, session) = fixture();
        let initial = session.control_wire_status().unwrap();
        let sequence = || {
            let Resume::Snapshot { snapshot } = session.resume_after(None).unwrap() else {
                panic!("expected snapshot");
            };
            Some(snapshot.sequence)
        };
        let mut changes = session.control_changes.subscribe();
        let desktop = identity();
        // Keep geometry unchanged: the control notification must own this event.
        let grant = registry
            .claim_control(session.id(), desktop, None, TerminalSize::new(24, 80))
            .unwrap();
        assert!(changes.has_changed().unwrap());
        drop(changes.borrow_and_update());
        let (_, _, claimed) = tokio::time::timeout(
            Duration::from_secs(2),
            session.wait_controlled_after(sequence(), initial.cursor()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(claimed.owner, Some(desktop));
        session.release_control(desktop, grant.generation).unwrap();
        assert!(changes.has_changed().unwrap());
        let (_, _, released) = tokio::time::timeout(
            Duration::from_secs(2),
            session.wait_controlled_after(sequence(), claimed.cursor()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(released.owner, None);
        assert_ne!(released.cursor(), claimed.cursor());
    }

    #[test]
    fn remote_takeover_and_local_control_do_not_overlap() {
        let (registry, session) = fixture();
        let lease = TerminalTakeoverLease {
            lease_id: FederationStewardTakeoverLeaseId::new(),
            revision: 1,
            expires_at: unix_timestamp() + 300,
        };
        registry.install_takeover(session.id(), lease).unwrap();
        let desktop = identity();
        assert!(matches!(
            registry.claim_control(session.id(), desktop, None, TerminalSize::new(24, 100)),
            Err(SessionRegistryError::TakeoverDenied)
        ));
        assert_eq!(session.control_status().unwrap().1, None);
        assert_eq!(size(&session), TerminalSize::new(24, 80));
        registry
            .release_takeover(session.id(), lease.lease_id, lease.revision)
            .unwrap();
        registry
            .claim_control(session.id(), desktop, None, TerminalSize::new(24, 100))
            .unwrap();
        assert!(matches!(
            registry.install_takeover(session.id(), lease),
            Err(SessionRegistryError::TakeoverConflict)
        ));
    }
}

#[cfg(test)]
mod lifecycle_environment_tests {
    #[test]
    fn provider_lifecycle_environment_is_private_to_the_selected_claude_process() {
        let session = swarm_domain::WorkerSessionId::new();
        let mut command = portable_pty::CommandBuilder::new("unused");
        let gate = super::configure_provider_lifecycle(
            session,
            Some(swarm_domain::ProviderKind::ClaudeCode),
            &mut command,
        )
        .unwrap();
        assert!(gate.is_some());
        assert_eq!(
            command
                .get_env("SWARM_PROVIDER_SESSION")
                .unwrap()
                .to_str()
                .unwrap(),
            session.to_string()
        );
        let secret = command
            .get_env("SWARM_PROVIDER_START_CAPABILITY")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(secret.len(), 64);
        assert!(secret.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!format!("{gate:?}").contains(secret));
        assert!(
            super::configure_provider_lifecycle(session, None, &mut command)
                .unwrap()
                .is_none()
        );
        assert!(command.get_env("SWARM_PROVIDER_SESSION").is_none());
        assert!(command.get_env("SWARM_PROVIDER_START_CAPABILITY").is_none());
    }
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
    use crate::{HistoryLimits, HistoryRecord, TerminalWriteActor};
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
            registry.write_local(
                session.id(),
                b"unsafe-local\n",
                TerminalWriteProvenance::operator(None, b"unsafe-local\n"),
            ),
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
        let audit = registry.recent_write_audit(10).unwrap();
        assert_eq!(audit.len(), 5);
        assert_eq!(audit[0].result, TerminalWriteResult::Rejected);
        assert_eq!(audit[1].result, TerminalWriteResult::Acknowledged);
        assert_eq!(audit[2].result, TerminalWriteResult::Acknowledged);
        assert_eq!(audit[3].result, TerminalWriteResult::Rejected);
        assert_eq!(audit[4].result, TerminalWriteResult::Rejected);
        assert!(matches!(
            audit[1].actor,
            TerminalWriteActor::Operator { device_id: None }
        ));
        assert!(matches!(
            audit[2].actor,
            TerminalWriteActor::Steward { lease_id: recorded } if recorded == lease_id
        ));
        assert!(audit.iter().all(|entry| entry.byte_count > 0));
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
    fn recovery_startup_provenance_is_owned_by_the_session_and_cannot_be_replaced() {
        let root = env::temp_dir().canonicalize().unwrap();
        let registry = SessionRegistry::new(JournalLimits::new(1024, 16), 1, [root]).unwrap();
        let session = registry
            .spawn(&shell_command("sleep 5"), TerminalSize::default())
            .unwrap();
        let swarm_domain::ConversationRecoveryState::Attempt { attempt } =
            swarm_domain::ConversationRecovery::new(None, true).state()
        else {
            panic!("expected attempt");
        };
        assert!(session.record_recovery_attempt(attempt));
        assert!(
            !session.record_recovery_attempt(ConversationRecoveryAttempt {
                number: 3,
                ..attempt
            })
        );
        assert_eq!(session.recovery_attempt(), Some(attempt));
        assert_eq!(session.provider_start().unwrap(), None);
        *session.provider_lifecycle.lock().unwrap() =
            Some(crate::ProviderLifecycleGate::new(session.id(), [173; 32]));
        let observation = crate::ProviderSessionStartObservation {
            conversation: swarm_domain::ProviderConversationId::new(),
            kind: swarm_domain::ProviderSessionStartKind::Resumed,
        };
        assert_eq!(
            session
                .observe_provider_start(&[173; 32], observation)
                .unwrap(),
            crate::ProviderLifecycleAcceptance::Accepted
        );
        let listed = registry.session_resource_states().unwrap();
        assert_eq!(listed[0].recovery_attempt, Some(attempt));
        assert_eq!(listed[0].provider_start, Some(observation));
        // Reading again (as a replacement API would) neither consumes nor resets it.
        assert_eq!(
            registry.session_resource_states().unwrap()[0].recovery_attempt,
            Some(attempt)
        );
        assert_eq!(
            registry.session_resource_states().unwrap()[0].provider_start,
            Some(observation)
        );
        assert!(
            session
                .observe_resume_end(&[173; 32], observation.conversation)
                .unwrap()
        );
        let switched = crate::ProviderSessionStartObservation {
            conversation: swarm_domain::ProviderConversationId::new(),
            ..observation
        };
        assert_eq!(
            session
                .observe_provider_start(&[173; 32], switched)
                .unwrap(),
            crate::ProviderLifecycleAcceptance::ConversationChanged
        );
        let selection = registry.session_resource_states().unwrap()[0]
            .provider_selection
            .unwrap();
        assert_eq!(selection.revision, 2);
        assert_eq!(selection.conversation, switched.conversation);
        registry.stop(session.id()).unwrap();
        assert!(
            !session
                .observe_resume_end(&[173; 32], switched.conversation)
                .unwrap()
        );
        assert_eq!(session.provider_start().unwrap(), Some(observation));
        assert_eq!(
            session
                .observe_provider_start(&[173; 32], observation)
                .unwrap(),
            crate::ProviderLifecycleAcceptance::Denied
        );
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
