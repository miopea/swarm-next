use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    LEGACY_MIGRATION_FORMAT, LEGACY_MIGRATION_VERSION, LegacyMigrationBundle,
    LegacyMigrationSource, LegacyTaskRecord, LegacyWorkerRecord,
};

#[derive(Debug, Error)]
pub enum LegacySourceError {
    #[error("legacy database could not be read: {0}")]
    Read(String),
    #[error("legacy database is not a supported Hive: {0}")]
    Invalid(String),
}

impl From<rusqlite::Error> for LegacySourceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Read(error.to_string())
    }
}

/// Reads a Legacy Hive database into a bounded migration bundle without
/// changing the source database or writing an intermediate package.
///
/// # Errors
/// Returns an error for an unreadable database, failed integrity check, or
/// unsupported Legacy schema.
pub fn read_legacy_migration_bundle(
    source: impl AsRef<Path>,
) -> Result<LegacyMigrationBundle, LegacySourceError> {
    let source = source.as_ref();
    let snapshot =
        std::fs::read(source).map_err(|error| LegacySourceError::Read(error.to_string()))?;
    let snapshot_digest = format!("{:x}", Sha256::digest(&snapshot));
    let connection = open_legacy_read_only(source)?;
    let schema_version = legacy_schema_version(&connection)?;
    if !table_exists(&connection, "tasks")? {
        return Err(LegacySourceError::Invalid(
            "Legacy tasks table is missing".into(),
        ));
    }
    if !table_exists(&connection, "workers")? {
        return Err(LegacySourceError::Invalid(
            "Legacy workers table is missing".into(),
        ));
    }
    let installation_id = legacy_installation_id(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT id, title, description, status, priority, assigned_worker,
                    jira_key, block_reason, acceptance_criteria, attachments,
                    source_email_id, CAST(created_at AS INTEGER), CAST(updated_at AS INTEGER)
             FROM tasks
             WHERE archived_at IS NULL
             ORDER BY COALESCE(number, 9223372036854775807), created_at, id",
        )
        .map_err(LegacySourceError::from)?;
    let rows = statement
        .query_map([], |row| {
            let criteria = parse_json_strings(&row.get::<_, String>(8)?);
            let attachments = parse_json_count(&row.get::<_, String>(9)?);
            Ok(LegacyTaskRecord {
                source_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                status: row.get(3)?,
                priority: row.get(4)?,
                assigned_worker: row.get(5)?,
                jira_key: row.get(6)?,
                block_reason: row.get(7)?,
                acceptance_criteria: criteria,
                attachment_count: attachments,
                source_email_id: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(LegacySourceError::from)?;
    let tasks = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(LegacySourceError::from)?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, path, description, provider, sort_order,
                    CASE WHEN trim(identity) <> '' THEN 1 ELSE 0 END, isolation
             FROM workers ORDER BY sort_order, name, id",
        )
        .map_err(LegacySourceError::from)?;
    let workers = statement
        .query_map([], |row| {
            Ok(LegacyWorkerRecord {
                source_id: row.get(0)?,
                name: row.get(1)?,
                workspace: row.get(2)?,
                description: row.get(3)?,
                provider: row.get(4)?,
                position: row.get(5)?,
                has_identity_file: row.get(6)?,
                isolation: row.get(7)?,
            })
        })
        .map_err(LegacySourceError::from)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(LegacySourceError::from)?;
    Ok(LegacyMigrationBundle {
        format: LEGACY_MIGRATION_FORMAT.into(),
        version: LEGACY_MIGRATION_VERSION,
        source: LegacyMigrationSource {
            installation_id,
            schema_version,
            exported_at: unix_timestamp(),
            snapshot_digest,
        },
        tasks,
        workers,
    })
}

fn open_legacy_read_only(path: &Path) -> Result<Connection, LegacySourceError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(LegacySourceError::from)?;
    let integrity: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(LegacySourceError::from)?;
    if integrity != "ok" {
        return Err(LegacySourceError::Invalid(format!(
            "integrity check failed: {integrity}"
        )));
    }
    Ok(connection)
}

fn legacy_schema_version(connection: &Connection) -> Result<Option<i64>, LegacySourceError> {
    if !table_exists(connection, "schema_version")? {
        return Ok(None);
    }
    connection
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .map_err(LegacySourceError::from)
}

fn legacy_installation_id(connection: &Connection) -> Result<String, LegacySourceError> {
    let mut statement = connection
        .prepare("SELECT id, name FROM workers ORDER BY id")
        .map_err(LegacySourceError::from)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(LegacySourceError::from)?;
    let mut hasher = Sha256::new();
    for row in rows {
        let (id, name) = row.map_err(LegacySourceError::from)?;
        hasher.update(id.as_bytes());
        hasher.update([0]);
        hasher.update(name.as_bytes());
        hasher.update([0xff]);
    }
    Ok(format!("legacy-{:x}", hasher.finalize()))
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool, LegacySourceError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .map_err(LegacySourceError::from)
}

fn parse_json_strings(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn parse_json_count(value: &str) -> usize {
    serde_json::from_str::<Vec<serde_json::Value>>(value).map_or(0, |items| items.len())
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_supported_records_without_changing_the_legacy_database() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("legacy.sqlite3");
        {
            let connection = Connection::open(&source).unwrap();
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
                        '[\"Verified\"]', '[\"one.png\"]', NULL, 100, 200, NULL);",
                )
                .unwrap();
        }
        let before = std::fs::read(&source).unwrap();

        let bundle = read_legacy_migration_bundle(&source).unwrap();

        assert_eq!(bundle.version, LEGACY_MIGRATION_VERSION);
        assert_eq!(bundle.tasks.len(), 1);
        assert_eq!(bundle.tasks[0].attachment_count, 1);
        assert_eq!(bundle.workers.len(), 1);
        assert_eq!(std::fs::read(source).unwrap(), before);
    }

    #[test]
    fn rejects_a_database_without_the_supported_worker_schema() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("not-a-hive.sqlite3");
        let connection = Connection::open(&source).unwrap();
        connection
            .execute("CREATE TABLE tasks (id TEXT)", [])
            .unwrap();

        assert!(matches!(
            read_legacy_migration_bundle(source),
            Err(LegacySourceError::Invalid(_))
        ));
    }
}
