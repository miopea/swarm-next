//! How a Hive tells its terminal host how long to keep history, and how the
//! host says what it is using (ADR 0118).
//!
//! ⚠️ FILES, NOT A HOST REQUEST. A new `HostRequest` is a protocol bump, and a
//! protocol bump is a migration that stops every worker on every Hive. Two
//! small files beside the history directory carry the same thing with no
//! protocol change: the API writes what it wants, and the host writes what it
//! applied, so "in effect" is the host's own word rather than an assumption.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Neither file is ever more than a line of JSON; anything larger is not ours.
const MAX_RETENTION_FILE_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct HistoryRetentionFile {
    max_age_seconds: u64,
}

fn beside(history_dir: &Path, suffix: &str) -> PathBuf {
    let name = history_dir
        .file_name()
        .map_or_else(|| "terminal-history".into(), |name| name.to_string_lossy());
    history_dir.with_file_name(format!("{name}.{suffix}.json"))
}

/// What the Hive asks for. The API writes it; the host reads it.
#[must_use]
pub fn history_retention_request_path(history_dir: &Path) -> PathBuf {
    beside(history_dir, "settings")
}

/// What the host is using. The host writes it; the API reads it.
#[must_use]
pub fn history_retention_applied_path(history_dir: &Path) -> PathBuf {
    beside(history_dir, "applied")
}

/// The retention in a file, in seconds: `None` when it is missing, too large,
/// unreadable, or names no positive age. Never an error, because both readers
/// treat "nothing usable" the same way: keep what they have.
#[must_use]
pub fn read_history_retention(path: &Path) -> Option<u64> {
    let mut contents = String::new();
    File::open(path)
        .ok()?
        .take(MAX_RETENTION_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .ok()?;
    if contents.len() as u64 > MAX_RETENTION_FILE_BYTES {
        return None;
    }
    serde_json::from_str::<HistoryRetentionFile>(&contents)
        .ok()
        .map(|file| file.max_age_seconds)
        .filter(|seconds| *seconds > 0)
}

/// Writes a retention file privately and atomically, so a reader never sees
/// half of one.
///
/// # Errors
/// Returns I/O errors.
pub fn write_history_retention(path: &Path, max_age_seconds: u64) -> std::io::Result<()> {
    let payload = serde_json::to_vec(&HistoryRetentionFile { max_age_seconds })
        .map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&payload)?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_files_sit_beside_the_history_directory() {
        let dir = Path::new("/state/swarm/terminal-history");
        assert_eq!(
            history_retention_request_path(dir),
            Path::new("/state/swarm/terminal-history.settings.json")
        );
        assert_eq!(
            history_retention_applied_path(dir),
            Path::new("/state/swarm/terminal-history.applied.json")
        );
    }

    #[test]
    fn a_retention_round_trips_and_nonsense_reads_as_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("terminal-history.settings.json");
        assert_eq!(read_history_retention(&path), None, "missing");
        write_history_retention(&path, 86_400).unwrap();
        assert_eq!(read_history_retention(&path), Some(86_400));
        for nonsense in [
            "{}".to_owned(),
            r#"{"max_age_seconds":0}"#.to_owned(),
            "not json".to_owned(),
            format!(r#"{{"max_age_seconds":1,"pad":"{}"}}"#, "x".repeat(5_000)),
        ] {
            fs::write(&path, nonsense).unwrap();
            assert_eq!(read_history_retention(&path), None);
        }
    }
}
