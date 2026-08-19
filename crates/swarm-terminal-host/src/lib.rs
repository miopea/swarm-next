use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use nix::unistd::Uid;
use swarm_domain::WorkerSessionId;
use swarm_terminal::{
    ClaudeCodeAdapter, CodexAdapter, HostRequest, HostResponse, HostSessionSummary,
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PROTOCOL_VERSION, SessionRegistry, TerminalHostStatus,
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
};
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum HostServerError {
    #[error("terminal socket must have a parent directory")]
    MissingSocketParent,
    #[error("terminal runtime directory is not a secure owned directory: {0}")]
    InsecureRuntimeDirectory(PathBuf),
    #[error("terminal socket already exists: {0}")]
    SocketAlreadyExists(PathBuf),
    #[error("terminal host I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal host serialization failed: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("terminal connection limit was closed")]
    ConnectionLimitClosed,
    #[error("terminal request timed out")]
    RequestTimedOut,
}

pub struct HostServer {
    listener: UnixListener,
    socket_path: PathBuf,
    registry: Arc<SessionRegistry>,
    host_version: Arc<str>,
    host_build_id: Arc<str>,
    connection_limit: Arc<Semaphore>,
}

impl std::fmt::Debug for HostServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostServer")
            .field("socket_path", &self.socket_path)
            .finish_non_exhaustive()
    }
}

impl HostServer {
    /// Binds a same-user Unix socket inside a private runtime directory.
    ///
    /// # Errors
    ///
    /// Returns an error for insecure ownership, an existing socket, permission
    /// failures, or bind failures.
    pub fn bind(
        socket_path: impl Into<PathBuf>,
        registry: Arc<SessionRegistry>,
    ) -> Result<Self, HostServerError> {
        Self::bind_with_identity(
            socket_path,
            registry,
            build_version(),
            worker_engine_build_id(),
        )
    }

    /// Binds a host that reports an explicit release identity.
    ///
    /// This supports package-compatibility acceptance without changing the
    /// process-global build identity used by production hosts.
    ///
    /// # Errors
    ///
    /// Returns the same secure socket and filesystem failures as [`Self::bind`].
    pub fn bind_with_version(
        socket_path: impl Into<PathBuf>,
        registry: Arc<SessionRegistry>,
        host_version: impl Into<Arc<str>>,
    ) -> Result<Self, HostServerError> {
        let host_version = host_version.into();
        Self::bind_with_identity(
            socket_path,
            registry,
            Arc::clone(&host_version),
            host_version,
        )
    }

    /// Binds a host with independently comparable release and engine identities.
    ///
    /// # Errors
    ///
    /// Returns the same secure socket and filesystem failures as [`Self::bind`].
    pub fn bind_with_identity(
        socket_path: impl Into<PathBuf>,
        registry: Arc<SessionRegistry>,
        host_version: impl Into<Arc<str>>,
        host_build_id: impl Into<Arc<str>>,
    ) -> Result<Self, HostServerError> {
        let socket_path = socket_path.into();
        let parent = socket_path
            .parent()
            .ok_or(HostServerError::MissingSocketParent)?;
        secure_runtime_directory(parent)?;
        if fs::symlink_metadata(&socket_path).is_ok() {
            return Err(HostServerError::SocketAlreadyExists(socket_path));
        }
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            socket_path,
            registry,
            host_version: host_version.into(),
            host_build_id: host_build_id.into(),
            connection_limit: Arc::new(Semaphore::new(64)),
        })
    }

    /// Serves requests until the owning task is cancelled or the listener
    /// fails. Dropping the server removes only its own socket path.
    ///
    /// # Errors
    ///
    /// Returns an error when accepting a connection fails.
    pub async fn run(self) -> Result<(), HostServerError> {
        info!(socket = %self.socket_path.display(), "terminal host listening");
        loop {
            let permit = Arc::clone(&self.connection_limit)
                .acquire_owned()
                .await
                .map_err(|_| HostServerError::ConnectionLimitClosed)?;
            let (stream, _) = self.listener.accept().await?;
            let registry = Arc::clone(&self.registry);
            let host_version = Arc::clone(&self.host_version);
            let host_build_id = Arc::clone(&self.host_build_id);
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(error) =
                    serve_connection(stream, registry, host_version, host_build_id).await
                {
                    warn!(%error, "terminal host rejected connection");
                }
            });
        }
    }
}

impl Drop for HostServer {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.socket_path)
            && metadata.file_type().is_socket()
        {
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}

fn secure_runtime_directory(path: &Path) -> Result<(), HostServerError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(HostServerError::InsecureRuntimeDirectory(
            path.to_path_buf(),
        ));
    }
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != Uid::effective().as_raw()
    {
        return Err(HostServerError::InsecureRuntimeDirectory(
            path.to_path_buf(),
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

async fn serve_connection(
    stream: UnixStream,
    registry: Arc<SessionRegistry>,
    host_version: Arc<str>,
    host_build_id: Arc<str>,
) -> Result<(), HostServerError> {
    let credentials = stream.peer_cred()?;
    if credentials.uid() != Uid::effective().as_raw() {
        return Err(HostServerError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "terminal IPC peer user did not match the host user",
        )));
    }

    let (reader, mut writer) = stream.into_split();
    let mut payload = Vec::new();
    let mut reader = BufReader::new(reader).take(MAX_REQUEST_BYTES + 1);
    tokio::time::timeout(
        Duration::from_secs(5),
        reader.read_until(b'\n', &mut payload),
    )
    .await
    .map_err(|_| HostServerError::RequestTimedOut)??;
    let response = if payload.len() as u64 > MAX_REQUEST_BYTES {
        error_response("request_too_large", "request exceeded the bounded frame")
    } else {
        match serde_json::from_slice::<HostRequest>(&payload) {
            Ok(request) if matches!(&request, HostRequest::Wait { .. }) => {
                tokio::select! {
                    response = dispatch(registry, host_version, host_build_id, request) => response,
                    _ = reader.read_u8() => return Ok(()),
                }
            }
            Ok(request) => dispatch(registry, host_version, host_build_id, request).await,
            Err(error) => error_response("invalid_request", &error.to_string()),
        }
    };

    let mut response = serde_json::to_vec(&response)?;
    if response.len() as u64 > MAX_RESPONSE_BYTES {
        response = serde_json::to_vec(&error_response(
            "response_too_large",
            "response exceeded the bounded frame",
        ))?;
    }
    response.push(b'\n');
    writer.write_all(&response).await?;
    Ok(())
}

async fn dispatch(
    registry: Arc<SessionRegistry>,
    host_version: Arc<str>,
    host_build_id: Arc<str>,
    request: HostRequest,
) -> HostResponse {
    match request {
        HostRequest::Wait {
            session_id,
            after_sequence,
        } => dispatch_wait(&registry, session_id, after_sequence).await,
        request => tokio::task::spawn_blocking(move || {
            dispatch_blocking(&registry, &host_version, &host_build_id, request)
        })
        .await
        .unwrap_or_else(|error| error_response("host_task_failed", &error.to_string())),
    }
}

async fn dispatch_wait(
    registry: &SessionRegistry,
    session_id: WorkerSessionId,
    after_sequence: Option<u64>,
) -> HostResponse {
    let session = match registry.get(session_id) {
        Ok(session) => session,
        Err(error) => return error_response("terminal_operation_failed", &error.to_string()),
    };
    let result =
        tokio::time::timeout(Duration::from_secs(30), session.wait_after(after_sequence)).await;
    match result {
        Ok(Ok((resume, running))) => HostResponse::Output {
            session_id,
            resume,
            running,
        },
        Ok(Err(error)) => error_response("terminal_operation_failed", &error.to_string()),
        Err(_) => match (session.resume_after(after_sequence), session.is_running()) {
            (Ok(resume), Ok(running)) => HostResponse::Output {
                session_id,
                resume,
                running,
            },
            (Err(error), _) | (_, Err(error)) => {
                error_response("terminal_operation_failed", &error.to_string())
            }
        },
    }
}

#[allow(clippy::too_many_lines)]
fn dispatch_blocking(
    registry: &SessionRegistry,
    host_version: &str,
    host_build_id: &str,
    request: HostRequest,
) -> HostResponse {
    let result = match request {
        HostRequest::Ping => {
            return HostResponse::Pong {
                protocol_version: PROTOCOL_VERSION,
            };
        }
        HostRequest::HostStatus => terminal_host_status(registry, host_version, host_build_id)
            .map(|status| HostResponse::HostStatus { status })
            .map_err(|error| error.to_string()),
        HostRequest::ProviderCapabilities => {
            // Resolved here rather than in the API because this is the process
            // that spawns providers, so its PATH is the one that decides which
            // release a worker actually gets.
            let search_path = std::env::var("PATH").ok();
            Ok(HostResponse::ProviderCapabilities {
                claude_code: executable_in_path("claude"),
                codex: executable_in_path("codex"),
                claude_release: swarm_terminal::provider_release(
                    std::path::Path::new("claude"),
                    search_path.as_deref(),
                ),
                codex_release: swarm_terminal::provider_release(
                    std::path::Path::new("codex"),
                    search_path.as_deref(),
                ),
            })
        }
        HostRequest::BeginDrain => registry
            .begin_drain()
            .and_then(|_| terminal_host_status(registry, host_version, host_build_id))
            .map(|status| HostResponse::HostStatus { status })
            .map_err(|error| error.to_string()),
        HostRequest::CancelDrain => registry
            .cancel_drain()
            .and_then(|()| terminal_host_status(registry, host_version, host_build_id))
            .map(|status| HostResponse::HostStatus { status })
            .map_err(|error| error.to_string()),
        HostRequest::StartClaude {
            workspace,
            size,
            conversation,
            mcp_config,
            allow_outside_roots,
        } => ClaudeCodeAdapter
            .command_for_with_configuration(
                &workspace,
                conversation,
                mcp_config.as_deref(),
                std::env::var_os("SWARM_CLAUDE_SETTINGS_PATH")
                    .map(PathBuf::from)
                    .filter(|path| path.is_file())
                    .as_deref(),
            )
            .map_err(|error| error.to_string())
            .and_then(|command| {
                registry
                    .spawn_with_root_override(&command, size, allow_outside_roots)
                    .map_err(|error| error.to_string())
            })
            .map(|session| HostResponse::SessionStarted {
                session_id: session.id(),
            }),
        HostRequest::StartCodex {
            workspace,
            size,
            conversation,
            allow_outside_roots,
        } => CodexAdapter
            .command_for(&workspace, conversation)
            .map_err(|error| error.to_string())
            .and_then(|command| {
                registry
                    .spawn_with_root_override(&command, size, allow_outside_roots)
                    .map_err(|error| error.to_string())
            })
            .map(|session| HostResponse::SessionStarted {
                session_id: session.id(),
            }),
        HostRequest::ListSessions => registry
            .session_resource_states()
            .map(|sessions| HostResponse::Sessions {
                sessions: sessions
                    .into_iter()
                    .map(|state| HostSessionSummary {
                        session_id: state.session_id,
                        running: state.running,
                        resources: state.resources,
                        last_output_at: Some(state.last_output_at),
                    })
                    .collect(),
            })
            .map_err(|error| error.to_string()),
        HostRequest::HistoryDiagnostics => registry
            .history_diagnostics()
            .map(|diagnostics| HostResponse::HistoryDiagnostics { diagnostics })
            .map_err(|error| error.to_string()),
        HostRequest::ListHistorySessions => registry
            .history_sessions()
            .map(|sessions| HostResponse::HistorySessions { sessions })
            .map_err(|error| error.to_string()),
        HostRequest::ReadHistory { session_id, cursor } => registry
            .history_page(session_id, cursor)
            .map(|page| HostResponse::HistoryPage { page })
            .map_err(|error| error.to_string()),
        HostRequest::Read {
            session_id,
            after_sequence,
        } => registry
            .get(session_id)
            .and_then(|session| Ok((session.resume_after(after_sequence)?, session.is_running()?)))
            .map(|(resume, running)| HostResponse::Output {
                session_id,
                resume,
                running,
            })
            .map_err(|error| error.to_string()),
        HostRequest::Wait { .. } => {
            return error_response(
                "invalid_dispatch",
                "wait requests must use the asynchronous dispatcher",
            );
        }
        HostRequest::Write {
            session_id,
            bytes,
            provenance,
        } => registry
            .write_local(session_id, &bytes, provenance)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::WriteAudit { limit } => registry
            .recent_write_audit(limit)
            .map(|entries| HostResponse::WriteAudit { entries })
            .map_err(|error| error.to_string()),
        HostRequest::InstallTakeover { session_id, lease } => registry
            .install_takeover(session_id, lease)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::TakeoverWrite {
            session_id,
            lease_id,
            revision,
            bytes,
        } => registry
            .write_takeover(session_id, lease_id, revision, &bytes)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::ReclaimTakeoverAndWrite {
            session_id,
            lease_id,
            revision,
            bytes,
        } => registry
            .reclaim_takeover_and_write(session_id, lease_id, revision, &bytes)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::ReleaseTakeover {
            session_id,
            lease_id,
            revision,
        } => registry
            .release_takeover(session_id, lease_id, revision)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::Resize { session_id, size } => registry
            .get(session_id)
            .and_then(|session| session.resize(size))
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
        HostRequest::Stop { session_id } => registry
            .stop(session_id)
            .map(|()| HostResponse::Acknowledged)
            .map_err(|error| error.to_string()),
    };
    result.unwrap_or_else(|message| error_response("terminal_operation_failed", &message))
}

fn executable_in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| {
            fs::metadata(directory.join(name)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn terminal_host_status(
    registry: &SessionRegistry,
    host_version: &str,
    host_build_id: &str,
) -> Result<TerminalHostStatus, swarm_terminal::SessionRegistryError> {
    Ok(TerminalHostStatus {
        protocol_version: PROTOCOL_VERSION,
        host_version: host_version.into(),
        host_build_id: Some(host_build_id.into()),
        draining: registry.is_draining(),
        running_sessions: registry.running_session_count()?,
        retained_sessions: registry.len()?,
        resources: Some(swarm_terminal::sample_current_process()),
        takeover_relay: true,
    })
}

fn build_version() -> &'static str {
    option_env!("SWARM_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn worker_engine_build_id() -> &'static str {
    option_env!("SWARM_WORKER_ENGINE_BUILD_ID").unwrap_or(build_version())
}

fn error_response(code: &str, message: &str) -> HostResponse {
    HostResponse::Error {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{env, time::Duration};

    use swarm_terminal::{HostClient, JournalLimits, ProviderCommand, Resume, TerminalSize};
    use tempfile::TempDir;

    use super::*;

    async fn assert_single_content_free_write_audit(
        client: &HostClient,
        session_id: WorkerSessionId,
    ) {
        let HostResponse::WriteAudit { entries } = client
            .request(&HostRequest::WriteAudit { limit: 10 })
            .await
            .unwrap()
        else {
            panic!("expected terminal write audit");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, session_id);
        assert_eq!(entries[0].byte_count, 14);
        assert_eq!(
            entries[0].result,
            swarm_terminal::TerminalWriteResult::Acknowledged
        );
        assert!(
            !serde_json::to_string(&entries)
                .unwrap()
                .contains("restart-proof")
        );
    }

    #[tokio::test]
    async fn client_reconnect_preserves_a_real_terminal_session() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![
                "-lc".into(),
                "printf before; read value; printf 'after:%s' \"$value\"".into(),
            ],
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, Arc::clone(&registry)).unwrap();
        let server_task = tokio::spawn(server.run());

        let first_client = HostClient::new(&socket);
        assert!(matches!(
            first_client.request(&HostRequest::Ping).await.unwrap(),
            HostResponse::Pong {
                protocol_version: PROTOCOL_VERSION
            }
        ));
        drop(first_client);

        let replacement_client = HostClient::new(&socket);
        let sessions = replacement_client
            .request(&HostRequest::ListSessions)
            .await
            .unwrap();
        let HostResponse::Sessions { sessions } = sessions else {
            panic!("expected session list");
        };
        assert_eq!(sessions[0].session_id, session.id());

        replacement_client
            .request(&HostRequest::Write {
                session_id: session.id(),
                bytes: b"restart-proof\n".to_vec(),
                provenance: swarm_terminal::TerminalWriteProvenance::operator(
                    None,
                    b"restart-proof\n",
                ),
            })
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let response = replacement_client
                .request(&HostRequest::Read {
                    session_id: session.id(),
                    after_sequence: Some(0),
                })
                .await
                .unwrap();
            let HostResponse::Output {
                resume: Resume::Deltas { frames },
                ..
            } = response
            else {
                panic!("expected retained deltas");
            };
            let output = frames
                .into_iter()
                .flat_map(|frame| frame.bytes)
                .collect::<Vec<_>>();
            if String::from_utf8_lossy(&output).contains("after:restart-proof") {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for output"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_single_content_free_write_audit(&replacement_client, session.id()).await;

        replacement_client
            .request(&HostRequest::Stop {
                session_id: session.id(),
            })
            .await
            .unwrap();
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn ipc_drain_status_is_atomic_with_session_creation() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 2, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "sleep 5".into()],
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, Arc::clone(&registry)).unwrap();
        let server_task = tokio::spawn(server.run());
        let client = HostClient::new(&socket);

        let HostResponse::HostStatus { status } =
            client.request(&HostRequest::BeginDrain).await.unwrap()
        else {
            panic!("begin drain must return host status");
        };
        assert!(status.draining);
        assert_eq!(status.running_sessions, 1);
        assert_eq!(status.retained_sessions, 1);
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert_eq!(
            status.host_build_id.as_deref(),
            Some(worker_engine_build_id())
        );
        assert!(matches!(
            registry.spawn(&command, TerminalSize::default()),
            Err(swarm_terminal::SessionRegistryError::HostDraining)
        ));

        let HostResponse::HostStatus { status } =
            client.request(&HostRequest::CancelDrain).await.unwrap()
        else {
            panic!("cancel drain must return host status");
        };
        assert!(!status.draining);
        session.stop().unwrap();
        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn socket_directory_and_socket_are_private() {
        let runtime = TempDir::new().unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry =
            Arc::new(SessionRegistry::new(JournalLimits::default(), 1, [workspace]).unwrap());
        let socket = runtime.path().join("private").join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();

        assert_eq!(
            fs::metadata(socket.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(server);
        assert!(!socket.exists());
    }

    #[tokio::test]
    async fn disconnected_wait_releases_its_connection_immediately() {
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry = Arc::new(
            SessionRegistry::new(JournalLimits::new(4096, 64), 1, [workspace.clone()]).unwrap(),
        );
        let command = ProviderCommand {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec!["-lc".into(), "sleep 5".into()],
            working_directory: workspace,
        };
        let session = registry.spawn(&command, TerminalSize::default()).unwrap();
        let (mut client, server) = UnixStream::pair().unwrap();
        let connection = tokio::spawn(serve_connection(
            server,
            Arc::clone(&registry),
            Arc::from(build_version()),
            Arc::from(worker_engine_build_id()),
        ));
        let mut request = serde_json::to_vec(&HostRequest::Wait {
            session_id: session.id(),
            after_sequence: Some(0),
        })
        .unwrap();
        request.push(b'\n');
        client.write_all(&request).await.unwrap();
        drop(client);

        tokio::time::timeout(Duration::from_secs(1), connection)
            .await
            .expect("disconnected wait retained the host connection")
            .unwrap()
            .unwrap();
        session.stop().unwrap();
    }

    #[tokio::test]
    async fn symlink_runtime_directory_is_rejected_without_chmod() {
        let runtime = TempDir::new().unwrap();
        let target = runtime.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        let alias = runtime.path().join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let workspace = env::temp_dir().canonicalize().unwrap();
        let registry =
            Arc::new(SessionRegistry::new(JournalLimits::default(), 1, [workspace]).unwrap());

        assert!(matches!(
            HostServer::bind(alias.join("terminal.sock"), registry),
            Err(HostServerError::InsecureRuntimeDirectory(_))
        ));
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
