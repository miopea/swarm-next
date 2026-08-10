use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use swarm_domain::WorkerSessionId;
use thiserror::Error;

use crate::{BoundedJournal, JournalLimits, ProviderCommand, Resume};

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
    #[error("workspace is outside the configured roots: {0}")]
    WorkspaceNotAllowed(PathBuf),
    #[error("workspace cannot be resolved: {0}")]
    WorkspaceUnavailable(PathBuf),
    #[error("terminal session was not found")]
    SessionNotFound,
    #[error("terminal operation failed: {0}")]
    Terminal(String),
    #[error("terminal session lock was poisoned")]
    LockPoisoned,
}

pub struct ProcessTerminalSession {
    id: WorkerSessionId,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    journal: Arc<Mutex<BoundedJournal>>,
    reader_running: Arc<AtomicBool>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
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
        let pair = native_pty_system()
            .openpty(size.as_pty_size())
            .map_err(terminal_error)?;
        let mut command_builder = CommandBuilder::new(&command.executable);
        command_builder.args(&command.arguments);
        command_builder.cwd(&command.working_directory);
        let child = pair
            .slave
            .spawn_command(command_builder)
            .map_err(terminal_error)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(terminal_error)?;
        let writer = pair.master.take_writer().map_err(terminal_error)?;
        let journal = Arc::new(Mutex::new(BoundedJournal::new(limits)));
        let reader_journal = Arc::clone(&journal);
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
                            if let Ok(mut journal) = reader_journal.lock() {
                                journal.push(buffer[..read].to_vec());
                            } else {
                                break;
                            }
                        }
                    }
                }
                reader_state.store(false, Ordering::Release);
            })
            .map_err(terminal_error)?;

        Ok(Self {
            id,
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            journal,
            reader_running,
            reader_thread: Mutex::new(Some(reader_thread)),
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
        if size.rows == 0 || size.columns == 0 {
            return Err(SessionRegistryError::Terminal(
                "terminal dimensions must be non-zero".into(),
            ));
        }
        lock(&self.master)?
            .resize(size.as_pty_size())
            .map_err(terminal_error)
    }

    /// Returns retained deltas or a deterministic snapshot requirement.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounded journal lock was poisoned.
    pub fn resume_after(&self, sequence: u64) -> Result<Resume, SessionRegistryError> {
        Ok(lock(&self.journal)?.resume_after(sequence))
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
            let _ = reader_thread.join();
        }
    }
}

#[derive(Debug)]
pub struct SessionRegistry {
    sessions: Mutex<HashMap<WorkerSessionId, Arc<ProcessTerminalSession>>>,
    limits: JournalLimits,
    max_sessions: usize,
    allowed_roots: Vec<PathBuf>,
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
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|root| {
                root.canonicalize()
                    .map_err(|_| SessionRegistryError::WorkspaceUnavailable(root))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
            limits,
            max_sessions,
            allowed_roots,
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
        let canonical_workspace = command.working_directory.canonicalize().map_err(|_| {
            SessionRegistryError::WorkspaceUnavailable(command.working_directory.clone())
        })?;
        if !self
            .allowed_roots
            .iter()
            .any(|root| canonical_workspace.starts_with(root))
        {
            return Err(SessionRegistryError::WorkspaceNotAllowed(
                canonical_workspace,
            ));
        }

        let mut sessions = lock(&self.sessions)?;
        if sessions.len() >= self.max_sessions {
            return Err(SessionRegistryError::SessionLimitReached {
                limit: self.max_sessions,
            });
        }
        let id = WorkerSessionId::new();
        let session = Arc::new(ProcessTerminalSession::spawn(
            id,
            command,
            self.limits,
            size,
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
        session.stop()
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
        env, thread,
        time::{Duration, Instant},
    };

    use super::*;

    fn shell_command(script: &str) -> ProviderCommand {
        ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), script.into()],
            working_directory: env::temp_dir(),
        }
    }

    fn output_until(session: &ProcessTerminalSession, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let Resume::Deltas { frames } = session.resume_after(0).unwrap() else {
                panic!("test output exceeded its journal");
            };
            let output = frames
                .into_iter()
                .flat_map(|frame| frame.bytes)
                .collect::<Vec<_>>();
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
}
