use std::{ffi::OsString, path::PathBuf, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use swarm_domain::WorkerSessionId;
use swarm_persistence::{LegacyMigrationBundle, read_legacy_migration_bundle};

use swarm_terminal::{
    HostClient, HostRequest, HostResponse, IpcError, PROTOCOL_VERSION, TerminalHostStatus,
};
use thiserror::Error;

pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MAX_READY_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
pub const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleCommand {
    Status,
    BeginDrain,
    CancelDrain,
    WaitReady {
        timeout: Duration,
    },
    StopSession {
        session_id: WorkerSessionId,
    },
    VerifyDatabase {
        path: std::path::PathBuf,
    },
    InspectLegacy {
        path: std::path::PathBuf,
    },
    ExportLegacyTasks {
        source: std::path::PathBuf,
        output: std::path::PathBuf,
    },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(
        "usage: swarmctl <status|drain|cancel-drain|wait-ready [timeout-seconds]|stop-session UUID|verify-database PATH|inspect-legacy PATH|export-legacy SOURCE OUTPUT>"
    )]
    Usage,
    #[error("wait timeout must be an integer from 1 through 86400 seconds")]
    InvalidTimeout,
    #[error("session id must be a UUID")]
    InvalidSessionId,
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
    #[error("database verification failed: {0}")]
    Database(String),
    #[error("legacy database inspection failed: {0}")]
    LegacyDatabase(String),
}

impl CliError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage | Self::InvalidTimeout | Self::InvalidSessionId => 2,
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
        "stop-session" => {
            let session_id = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .and_then(|value| value.parse().ok())
                .ok_or(CliError::InvalidSessionId)?;
            if arguments.next().is_some() {
                return Err(CliError::Usage);
            }
            Ok(LifecycleCommand::StopSession { session_id })
        }
        "verify-database" => {
            let path = arguments
                .next()
                .map(std::path::PathBuf::from)
                .ok_or(CliError::Usage)?;
            if arguments.next().is_some() || !path.is_absolute() {
                return Err(CliError::Usage);
            }
            Ok(LifecycleCommand::VerifyDatabase { path })
        }
        "inspect-legacy" => {
            let path = arguments.next().map(PathBuf::from).ok_or(CliError::Usage)?;
            if arguments.next().is_some() || !path.is_absolute() {
                return Err(CliError::Usage);
            }
            Ok(LifecycleCommand::InspectLegacy { path })
        }
        "export-legacy" | "export-legacy-tasks" => {
            let source = arguments.next().map(PathBuf::from).ok_or(CliError::Usage)?;
            let output = arguments.next().map(PathBuf::from).ok_or(CliError::Usage)?;
            if arguments.next().is_some() || !source.is_absolute() || !output.is_absolute() {
                return Err(CliError::Usage);
            }
            Ok(LifecycleCommand::ExportLegacyTasks { source, output })
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
        LifecycleCommand::StopSession { session_id } => {
            match client.request(&HostRequest::Stop { session_id }).await? {
                HostResponse::Acknowledged => request_status(client, HostRequest::HostStatus).await,
                HostResponse::Error { message, .. } => Err(CliError::HostRejected(message)),
                _ => Err(CliError::UnexpectedResponse),
            }
        }
        LifecycleCommand::VerifyDatabase { .. }
        | LifecycleCommand::InspectLegacy { .. }
        | LifecycleCommand::ExportLegacyTasks { .. } => Err(CliError::Usage),
    }
}

/// Exports a versioned, portable task package from a Legacy snapshot.
/// The source is opened read-only and the output path must not already exist.
///
/// # Errors
/// Returns an error for an invalid snapshot, unsupported task schema, malformed
/// Legacy JSON fields, or an output collision.
pub fn export_legacy_tasks(
    source: impl AsRef<std::path::Path>,
    output: impl AsRef<std::path::Path>,
) -> Result<LegacyMigrationBundle, CliError> {
    let source = source.as_ref();
    let output = output.as_ref();
    if output.exists() {
        return Err(CliError::LegacyDatabase(
            "migration output already exists; choose a new file".into(),
        ));
    }
    let bundle = read_legacy_migration_bundle(source)
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&bundle)
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    std::fs::write(output, bytes).map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    Ok(bundle)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LegacyInspectionReport {
    pub format: &'static str,
    pub schema_version: Option<i64>,
    pub workers: LegacyTableReport,
    pub tasks: LegacyTableReport,
    pub groups: LegacyTableReport,
    pub warnings: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LegacyTableReport {
    pub present: bool,
    pub records: i64,
    pub eligible: i64,
    pub invalid: i64,
}

/// Opens a legacy Swarm snapshot read-only and reports migration eligibility.
/// It never attaches the database to the Next store and never changes either file.
///
/// # Errors
/// Returns an error when the snapshot cannot be opened read-only, fails its
/// integrity check, or contains unreadable supported tables.
pub fn inspect_legacy_database(
    path: impl AsRef<std::path::Path>,
) -> Result<LegacyInspectionReport, CliError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    let integrity: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    if integrity != "ok" {
        return Err(CliError::LegacyDatabase(format!(
            "integrity check failed: {integrity}"
        )));
    }
    let schema_version = if table_exists(&connection, "schema_version")? {
        connection
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|error| CliError::LegacyDatabase(error.to_string()))?
            .flatten()
    } else {
        None
    };
    let workers = inspect_table(
        &connection,
        "workers",
        "trim(name) <> '' AND trim(path) <> ''",
    )?;
    let tasks = inspect_table(&connection, "tasks", "trim(title) <> ''")?;
    let groups = inspect_table(&connection, "groups", "trim(name) <> ''")?;
    let mut warnings = Vec::new();
    if schema_version.is_none() {
        warnings.push("legacy schema version is missing");
    }
    warnings.push("inspection only; no records were imported or modified");
    warnings.push("sessions, terminal history, credentials, drones, and approval rules are intentionally excluded");
    Ok(LegacyInspectionReport {
        format: "swarm-legacy-sqlite",
        schema_version,
        workers,
        tasks,
        groups,
        warnings,
    })
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool, CliError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))
}

fn inspect_table(
    connection: &Connection,
    name: &str,
    eligible: &str,
) -> Result<LegacyTableReport, CliError> {
    if !table_exists(connection, name)? {
        return Ok(LegacyTableReport::default());
    }
    let records: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |row| {
            row.get(0)
        })
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    let eligible: i64 = connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {name} WHERE {eligible}"),
            [],
            |row| row.get(0),
        )
        .map_err(|error| CliError::LegacyDatabase(error.to_string()))?;
    Ok(LegacyTableReport {
        present: true,
        records,
        eligible,
        invalid: records.saturating_sub(eligible),
    })
}

/// Opens an exported Hive database and verifies its schema and `SQLite` integrity.
///
/// # Errors
/// Returns an error when the database cannot be opened, migrated, or verified.
pub fn verify_database(path: impl AsRef<std::path::Path>) -> Result<(), CliError> {
    let store = swarm_persistence::TaskStore::open(path)
        .map_err(|error| CliError::Database(error.to_string()))?;
    store
        .verify_integrity()
        .map_err(|error| CliError::Database(error.to_string()))
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
        assert!(matches!(
            parse_command([
                OsString::from("verify-database"),
                OsString::from("/tmp/hive.sqlite3")
            ]),
            Ok(LifecycleCommand::VerifyDatabase { .. })
        ));
        assert!(matches!(
            parse_command([
                OsString::from("inspect-legacy"),
                OsString::from("/tmp/legacy.sqlite3")
            ]),
            Ok(LifecycleCommand::InspectLegacy { .. })
        ));
        let session_id = WorkerSessionId::new();
        assert_eq!(
            parse_command([
                OsString::from("stop-session"),
                OsString::from(session_id.to_string())
            ])
            .unwrap(),
            LifecycleCommand::StopSession { session_id }
        );
        assert!(matches!(
            parse_command([OsString::from("stop-session"), OsString::from("not-a-uuid")]),
            Err(CliError::InvalidSessionId)
        ));
    }

    #[test]
    fn rejects_a_mismatched_host_protocol() {
        let error = validate_status(TerminalHostStatus {
            protocol_version: PROTOCOL_VERSION + 1,
            host_version: "future".into(),
            host_build_id: None,
            draining: false,
            running_sessions: 0,
            retained_sessions: 0,
            resources: None,
            takeover_relay: false,
        })
        .unwrap_err();
        assert!(matches!(error, CliError::ProtocolMismatch { .. }));
    }

    #[test]
    fn machine_status_is_one_compact_json_line() {
        let output = format_status(&TerminalHostStatus {
            protocol_version: PROTOCOL_VERSION,
            host_version: "0.1.0".into(),
            host_build_id: None,
            draining: true,
            running_sessions: 1,
            retained_sessions: 2,
            resources: None,
            takeover_relay: true,
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

    #[test]
    fn legacy_inspection_is_read_only_and_reports_only_supported_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.sqlite3");
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection.execute_batch(
                "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at REAL NOT NULL);
                 INSERT INTO schema_version VALUES (19, 1);
                 CREATE TABLE workers (id TEXT, name TEXT, path TEXT);
                 INSERT INTO workers VALUES ('1', 'Daisy', '/projects/daisy'), ('2', '', '/projects/invalid');
                 CREATE TABLE tasks (id TEXT, title TEXT);
                 INSERT INTO tasks VALUES ('1', 'Ship this'), ('2', '');
                 CREATE TABLE groups (id TEXT, name TEXT);
                 INSERT INTO groups VALUES ('1', 'Web');",
            ).unwrap();
        }
        let before = std::fs::metadata(&path).unwrap().len();
        let report = inspect_legacy_database(&path).unwrap();
        assert_eq!(report.schema_version, Some(19));
        assert_eq!(
            report.workers,
            LegacyTableReport {
                present: true,
                records: 2,
                eligible: 1,
                invalid: 1
            }
        );
        assert_eq!(report.tasks.eligible, 1);
        assert_eq!(report.groups.invalid, 0);
        assert_eq!(std::fs::metadata(path).unwrap().len(), before);
    }

    #[test]
    fn legacy_task_export_is_portable_read_only_and_refuses_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("legacy.sqlite3");
        let output = directory.path().join("legacy-tasks.json");
        {
            let connection = rusqlite::Connection::open(&source).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at REAL NOT NULL);
                     INSERT INTO schema_version VALUES (19, 1);
                     CREATE TABLE workers (
                         id TEXT, name TEXT, path TEXT, description TEXT, provider TEXT,
                         isolation TEXT, identity TEXT, sort_order INTEGER
                     );
                     INSERT INTO workers VALUES
                       ('worker-1', 'Daisy', '/private/path', 'Owns Daisy', 'claude', '', '', 3);
                     CREATE TABLE tasks (
                         id TEXT, number INTEGER, title TEXT, description TEXT, status TEXT,
                         priority TEXT, assigned_worker TEXT, jira_key TEXT, block_reason TEXT,
                         acceptance_criteria TEXT, attachments TEXT, source_email_id TEXT,
                         created_at REAL, updated_at REAL, archived_at REAL
                     );
                     INSERT INTO tasks VALUES
                       ('task-1', 1, 'Ship this', 'Outcome', 'active', 'high', 'Daisy', NULL, '',
                        '[\"Verified\"]', '[\"one.png\"]', NULL, 100, 200, NULL),
                       ('task-2', 2, 'Jira work', '', 'assigned', 'normal', NULL, 'WWD-2', '',
                        '[]', '[]', NULL, 100, 200, NULL),
                       ('archived', 3, 'Old', '', 'done', 'normal', NULL, NULL, '',
                        '[]', '[]', NULL, 100, 200, 300);",
                )
                .unwrap();
        }
        let before = std::fs::read(&source).unwrap();
        let bundle = export_legacy_tasks(&source, &output).unwrap();
        assert_eq!(bundle.version, swarm_persistence::LEGACY_MIGRATION_VERSION);
        assert_eq!(bundle.tasks.len(), 2);
        assert_eq!(bundle.tasks[0].acceptance_criteria, vec!["Verified"]);
        assert_eq!(bundle.tasks[0].attachment_count, 1);
        assert_eq!(bundle.workers.len(), 1);
        assert_eq!(bundle.workers[0].workspace, "/private/path");
        assert_eq!(bundle.workers[0].description, "Owns Daisy");
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert_eq!(read_legacy_migration_bundle(&source).unwrap(), bundle);
        assert!(matches!(
            export_legacy_tasks(&source, &output),
            Err(CliError::LegacyDatabase(_))
        ));
    }

    #[test]
    fn verifies_a_real_database_and_rejects_non_database_input() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("hive.sqlite3");
        swarm_persistence::TaskStore::open(&database).unwrap();
        verify_database(&database).unwrap();
        let invalid = directory.path().join("not-a-database");
        std::fs::write(&invalid, b"not sqlite").unwrap();
        assert!(matches!(
            verify_database(invalid),
            Err(CliError::Database(_))
        ));
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

        let stopped = execute(
            &client,
            LifecycleCommand::StopSession {
                session_id: session.id(),
            },
        )
        .await
        .unwrap();
        assert_eq!(stopped.running_sessions, 0);
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
