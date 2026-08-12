use std::{ffi::OsString, time::Duration};

use swarm_terminal::{
    HostClient, HostRequest, HostResponse, IpcError, PROTOCOL_VERSION, TerminalHostStatus,
};
use thiserror::Error;

pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MAX_READY_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
pub const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleCommand {
    Status,
    BeginDrain,
    CancelDrain,
    WaitReady { timeout: Duration },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("usage: swarmctl <status|drain|cancel-drain|wait-ready [timeout-seconds]>")]
    Usage,
    #[error("wait timeout must be an integer from 1 through 86400 seconds")]
    InvalidTimeout,
    #[error("terminal host IPC failed: {0}")]
    Ipc(#[from] IpcError),
    #[error("terminal host rejected the lifecycle request: {0}")]
    HostRejected(String),
    #[error("terminal host returned an unexpected response")]
    UnexpectedResponse,
    #[error("terminal host protocol {actual} is incompatible with swarmctl protocol {expected}")]
    ProtocolMismatch { expected: u16, actual: u16 },
    #[error("terminal host is not draining; begin drain before waiting for readiness")]
    NotDraining,
    #[error(
        "terminal host did not become ready within the timeout; {running_sessions} sessions remain"
    )]
    ReadyTimeout { running_sessions: usize },
}

impl CliError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage | Self::InvalidTimeout => 2,
            Self::ReadyTimeout { .. } => 3,
            _ => 1,
        }
    }
}

/// Parses the bounded lifecycle command surface.
///
/// # Errors
///
/// Returns an error for unknown commands, extra arguments, or timeout values
/// outside the product bound.
pub fn parse_command(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<LifecycleCommand, CliError> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next().and_then(|value| value.into_string().ok()) else {
        return Err(CliError::Usage);
    };
    match command.as_str() {
        "status" if arguments.next().is_none() => Ok(LifecycleCommand::Status),
        "drain" if arguments.next().is_none() => Ok(LifecycleCommand::BeginDrain),
        "cancel-drain" if arguments.next().is_none() => Ok(LifecycleCommand::CancelDrain),
        "wait-ready" => {
            let timeout = arguments
                .next()
                .map_or(Ok(DEFAULT_READY_TIMEOUT), parse_timeout)?;
            if arguments.next().is_some() {
                return Err(CliError::Usage);
            }
            Ok(LifecycleCommand::WaitReady { timeout })
        }
        _ => Err(CliError::Usage),
    }
}

fn parse_timeout(value: OsString) -> Result<Duration, CliError> {
    let seconds = value
        .into_string()
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(CliError::InvalidTimeout)?;
    let timeout = Duration::from_secs(seconds);
    if timeout.is_zero() || timeout > MAX_READY_TIMEOUT {
        return Err(CliError::InvalidTimeout);
    }
    Ok(timeout)
}

/// Executes one lifecycle command against the same-user terminal-host socket.
///
/// # Errors
///
/// Returns an error for IPC, host rejection, protocol mismatch, invalid drain
/// state, or readiness timeout.
pub async fn execute(
    client: &HostClient,
    command: LifecycleCommand,
) -> Result<TerminalHostStatus, CliError> {
    match command {
        LifecycleCommand::Status => request_status(client, HostRequest::HostStatus).await,
        LifecycleCommand::BeginDrain => request_status(client, HostRequest::BeginDrain).await,
        LifecycleCommand::CancelDrain => request_status(client, HostRequest::CancelDrain).await,
        LifecycleCommand::WaitReady { timeout } => {
            wait_until_ready(client, timeout, READY_POLL_INTERVAL).await
        }
    }
}

/// Serializes one machine-readable status object without embedded newlines.
///
/// # Errors
///
/// Returns an error only if the typed status cannot be serialized.
pub fn format_status(status: &TerminalHostStatus) -> Result<String, serde_json::Error> {
    serde_json::to_string(status)
}

async fn request_status(
    client: &HostClient,
    request: HostRequest,
) -> Result<TerminalHostStatus, CliError> {
    match client.request(&request).await? {
        HostResponse::HostStatus { status } => validate_status(status),
        HostResponse::Error { message, .. } => Err(CliError::HostRejected(message)),
        _ => Err(CliError::UnexpectedResponse),
    }
}

fn validate_status(status: TerminalHostStatus) -> Result<TerminalHostStatus, CliError> {
    if status.protocol_version != PROTOCOL_VERSION {
        return Err(CliError::ProtocolMismatch {
            expected: PROTOCOL_VERSION,
            actual: status.protocol_version,
        });
    }
    Ok(status)
}

async fn wait_until_ready(
    client: &HostClient,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<TerminalHostStatus, CliError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let status = request_status(client, HostRequest::HostStatus).await?;
        if !status.draining {
            return Err(CliError::NotDraining);
        }
        if status.running_sessions == 0 {
            return Ok(status);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(CliError::ReadyTimeout {
                running_sessions: status.running_sessions,
            });
        }
        tokio::time::sleep(poll_interval.min(deadline - now)).await;
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, sync::Arc};

    use swarm_terminal::{JournalLimits, ProviderCommand, SessionRegistry, TerminalSize};
    use swarm_terminal_host::HostServer;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn parses_only_the_bounded_lifecycle_surface() {
        assert_eq!(
            parse_command([OsString::from("status")]).unwrap(),
            LifecycleCommand::Status
        );
        assert_eq!(
            parse_command([OsString::from("wait-ready"), OsString::from("60")]).unwrap(),
            LifecycleCommand::WaitReady {
                timeout: Duration::from_secs(60)
            }
        );
        assert!(matches!(
            parse_command([OsString::from("wait-ready"), OsString::from("0")]),
            Err(CliError::InvalidTimeout)
        ));
        assert!(matches!(
            parse_command([OsString::from("restart")]),
            Err(CliError::Usage)
        ));
    }

    #[test]
    fn rejects_a_mismatched_host_protocol() {
        let error = validate_status(TerminalHostStatus {
            protocol_version: PROTOCOL_VERSION + 1,
            host_version: "future".into(),
            draining: false,
            running_sessions: 0,
            retained_sessions: 0,
            resources: None,
        })
        .unwrap_err();
        assert!(matches!(error, CliError::ProtocolMismatch { .. }));
    }

    #[test]
    fn machine_status_is_one_compact_json_line() {
        let output = format_status(&TerminalHostStatus {
            protocol_version: PROTOCOL_VERSION,
            host_version: "0.1.0".into(),
            draining: true,
            running_sessions: 1,
            retained_sessions: 2,
            resources: None,
        })
        .unwrap();
        assert!(!output.contains('\n'));
        assert_eq!(
            serde_json::from_str::<TerminalHostStatus>(&output)
                .unwrap()
                .running_sessions,
            1
        );
    }

    #[tokio::test]
    async fn drives_drain_cancel_and_bounded_readiness_over_real_ipc() {
        let runtime = TempDir::new().unwrap();
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
        let socket = runtime.path().join("terminal.sock");
        let server = HostServer::bind(&socket, registry).unwrap();
        let server_task = tokio::spawn(server.run());
        let client = HostClient::new(&socket);

        let draining = execute(&client, LifecycleCommand::BeginDrain)
            .await
            .unwrap();
        assert!(draining.draining);
        assert_eq!(draining.running_sessions, 1);
        let timeout =
            wait_until_ready(&client, Duration::from_millis(20), Duration::from_millis(5))
                .await
                .unwrap_err();
        assert!(matches!(
            timeout,
            CliError::ReadyTimeout {
                running_sessions: 1
            }
        ));

        session.stop().unwrap();
        let ready = wait_until_ready(&client, Duration::from_secs(1), Duration::from_millis(5))
            .await
            .unwrap();
        assert_eq!(ready.running_sessions, 0);
        let cancelled = execute(&client, LifecycleCommand::CancelDrain)
            .await
            .unwrap();
        assert!(!cancelled.draining);
        assert!(matches!(
            execute(
                &client,
                LifecycleCommand::WaitReady {
                    timeout: Duration::from_secs(1)
                }
            )
            .await,
            Err(CliError::NotDraining)
        ));
        server_task.abort();
        let _ = server_task.await;
    }
}
