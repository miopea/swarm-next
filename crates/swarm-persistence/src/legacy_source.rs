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
                provider_conversation_id: discover_provider_conversation(
                    &row.get::<_, String>(4)?,
                    &row.get::<_, String>(2)?,
                ),
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

fn discover_provider_conversation(provider: &str, workspace: &str) -> Option<String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let workspace = expand_workspace_home(workspace, &home);
    match provider.trim().to_ascii_lowercase().as_str() {
        "" | "claude" | "claude_code" => discover_claude_conversation(&workspace),
        "codex" => discover_codex_conversation(&workspace),
        _ => None,
    }
}

fn expand_workspace_home(workspace: &str, home: &Path) -> String {
    let workspace = workspace.trim();
    if workspace == "~" {
        return home.to_string_lossy().into_owned();
    }
    workspace.strip_prefix("~/").map_or_else(
        || workspace.to_owned(),
        |relative| home.join(relative).to_string_lossy().into_owned(),
    )
}

/// The directory names Claude may have stored a workspace's transcripts under,
/// current encoding first.
///
/// ONE OWNER, BECAUSE A WRONG COPY OF THIS FAILS AS AN EMPTY RESULT. Claude
/// encodes the absolute workspace path by replacing '/' AND '.' with '-'; an
/// older form replaced only '/', and both exist on disk, so every lookup has to
/// try the current form and fall back.
///
/// This was reimplemented four separate times across three files. That is worse
/// than ordinary duplication because a copy that forgets the '.' does not throw
/// — it returns an empty directory listing, and every caller reads that as "this
/// workspace has no transcripts", which is a legitimate state. No exception, no
/// mismatch, no log line.
///
/// It bit exactly that way on 2026-09-02: a fleet sweep slugged with '/' only
/// and reported thirteen workers as having no transcripts while the directories
/// were sitting there. A path with no dot works fine under the wrong encoding,
/// so a bad copy passes every test it is likely to be given until it meets a
/// dotted one.
#[must_use]
pub fn claude_project_slugs(workspace: &str) -> [String; 2] {
    [
        workspace.replace(['/', '.'], "-"),
        workspace.replace('/', "-"),
    ]
}

/// The directory Claude actually stored this workspace's transcripts in, if any.
///
/// Returns `None` only when NEITHER encoding names an existing directory, which
/// is what lets a caller tell "no such workspace" from "no transcripts yet" —
/// the ambiguity that made the wrong slug look like a finding rather than a bug.
#[must_use]
pub fn claude_project_directory(root: &Path, workspace: &str) -> Option<std::path::PathBuf> {
    claude_project_slugs(workspace)
        .into_iter()
        .map(|slug| root.join(slug))
        .find(|path| path.is_dir())
}

fn discover_claude_conversation(workspace: &str) -> Option<String> {
    let root = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)?
        .join(".claude")
        .join("projects");
    discover_claude_conversation_in(&root, workspace)
}

fn discover_claude_conversation_in(root: &Path, workspace: &str) -> Option<String> {
    let mut candidates = Vec::new();
    for encoded in claude_project_slugs(workspace) {
        let directory = root.join(encoded);
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(2_000) {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if uuid::Uuid::parse_str(id).is_err() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |value| value.as_secs());
            candidates.push((modified, id.to_owned()));
        }
    }
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().next().map(|(_, id)| id)
}

fn discover_codex_conversation(workspace: &str) -> Option<String> {
    let root = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)?
        .join(".codex")
        .join("sessions");
    discover_codex_conversation_in(&root, workspace)
}

fn discover_codex_conversation_in(root: &Path, workspace: &str) -> Option<String> {
    let mut pending = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    let mut visited = 0_usize;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 20_000 {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            let mut lines = std::io::BufRead::lines(std::io::BufReader::new(file));
            let Some(Ok(line)) = lines.next() else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let payload = &value["payload"];
            if value["type"].as_str() != Some("session_meta")
                || payload["cwd"].as_str() != Some(workspace)
            {
                continue;
            }
            let Some(id) = payload["id"].as_str() else {
                continue;
            };
            if uuid::Uuid::parse_str(id).is_err() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |value| value.as_secs());
            candidates.push((modified, id.to_owned()));
        }
        if visited > 20_000 {
            break;
        }
    }
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().next().map(|(_, id)| id)
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
    /// THE '.' IS THE CHARACTER A REIMPLEMENTATION FORGETS, and a copy that
    /// forgets it works until it meets a dotted path.
    ///
    /// /home/x/projects/personal/aria has no dot, so the wrong encoding returns
    /// the right answer for it and for most workspaces anyone would test with.
    /// The failure only appears against a path like /home/x/.config — and it
    /// appears as an EMPTY LISTING, which every caller reads as "this workspace
    /// has no transcripts". That is a legitimate state, so nothing looks wrong.
    ///
    /// Measured 2026-09-02: a fleet sweep slugged with '/' only and reported
    /// thirteen workers as having no transcripts while their directories
    /// existed.
    #[test]
    fn the_project_slug_replaces_dots_as_well_as_slashes() {
        assert_eq!(
            claude_project_slugs("/home/x/.config/thing")[0],
            "-home-x--config-thing",
            "a slug that keeps the dot addresses a directory Claude never wrote"
        );
        assert_eq!(
            claude_project_slugs("/home/x/.config/thing")[1],
            "-home-x-.config-thing",
            "and the legacy form is kept, because both exist on disk"
        );
        // A dotless path is identical under both encodings, which is exactly
        // why a wrong copy passes the tests it is likely to be given.
        let dotless = claude_project_slugs("/home/x/projects/aria");
        assert_eq!(dotless[0], dotless[1]);
        assert_eq!(dotless[0], "-home-x-projects-aria");
    }

    /// "No such workspace" and "no transcripts yet" were indistinguishable, and
    /// that ambiguity is what let a wrong string read as a finding.
    #[test]
    fn a_workspace_with_no_directory_under_either_encoding_is_none() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("projects");
        std::fs::create_dir_all(root.join("-home-x--config-thing")).unwrap();

        assert!(
            claude_project_directory(&root, "/home/x/.config/thing").is_some(),
            "the current encoding resolves"
        );
        assert!(
            claude_project_directory(&root, "/home/x/nowhere").is_none(),
            "and a workspace with no directory under EITHER encoding says so"
        );
    }

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

    #[test]
    fn finds_exact_provider_conversations_without_reading_transcript_content() {
        let directory = tempfile::tempdir().unwrap();
        let claude_root = directory.path().join("claude-projects");
        let workspace = "/projects/daisy";
        let claude_id = uuid::Uuid::now_v7().to_string();
        let encoded = workspace.replace(['/', '.'], "-");
        std::fs::create_dir_all(claude_root.join(encoded)).unwrap();
        std::fs::write(
            claude_root
                .join(workspace.replace(['/', '.'], "-"))
                .join(format!("{claude_id}.jsonl")),
            "private transcript content\n",
        )
        .unwrap();
        assert_eq!(
            discover_claude_conversation_in(&claude_root, workspace),
            Some(claude_id)
        );

        let codex_root = directory.path().join("codex-sessions");
        std::fs::create_dir_all(codex_root.join("2026/08/18")).unwrap();
        let codex_id = uuid::Uuid::now_v7().to_string();
        std::fs::write(
            codex_root.join("2026/08/18/session.jsonl"),
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{codex_id}\",\"cwd\":\"{workspace}\"}}}}\nprivate transcript content\n"
            ),
        )
        .unwrap();
        assert_eq!(
            discover_codex_conversation_in(&codex_root, workspace),
            Some(codex_id)
        );
    }

    #[test]
    fn expands_legacy_tilde_workspaces_before_provider_discovery() {
        let home = Path::new("/home/operator");

        assert_eq!(
            expand_workspace_home("~/projects/rcg/rcg-hub", home),
            "/home/operator/projects/rcg/rcg-hub"
        );
        assert_eq!(
            expand_workspace_home("/srv/projects/platform", home),
            "/srv/projects/platform"
        );
    }
}
