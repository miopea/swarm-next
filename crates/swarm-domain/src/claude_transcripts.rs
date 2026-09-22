//! Where Claude keeps a workspace's transcripts, and which ones exist.
//!
//! ⚠️ ONE OWNER FOR THE ENCODING, and it lives here because two crates need it:
//! swarm-persistence judges whether a worker's pin has gone stale, and the
//! terminal host looks for something to resume when it has not. A second copy
//! that forgets the `.` variant names a directory that does not exist, returns
//! an empty listing rather than failing, and every caller reads that as "no
//! transcripts" — a wrong answer that looks like a finding.

use std::path::{Path, PathBuf};

/// Both encodings Claude has used for a workspace path.
///
/// Claude replaces the separators in the absolute path; which characters it
/// replaces has changed, so a caller has to try both rather than pick one.
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
/// is what lets a caller tell "no such workspace" from "no transcripts yet".
#[must_use]
pub fn claude_project_directory(root: &Path, workspace: &str) -> Option<PathBuf> {
    claude_project_slugs(workspace)
        .into_iter()
        .map(|slug| root.join(slug))
        .find(|path| path.is_dir())
}

/// How many directory entries are examined before giving up.
///
/// A workspace that has accumulated transcripts for months is ordinary; reading
/// all of them to answer "what is recent" is not.
const MAX_TRANSCRIPTS_SCANNED: usize = 2_000;

/// The conversations Claude holds for this workspace, newest first.
///
/// ⚠️ THIS IS A LIST OF CANDIDATES, NOT A LIST OF ANSWERS. A transcript file is
/// not a promise that Claude will open it — the caller must confirm each one
/// with Claude itself. Inferring availability from the filesystem is what made
/// a directory scan untrustworthy enough to need replacing (ADR 0109).
///
/// Ordered by modification time because recency is the only ranking available
/// without reading the files, and it is the one `--continue` was approximating.
#[must_use]
pub fn claude_conversations_newest_first(root: &Path, workspace: &str) -> Vec<String> {
    let Some(directory) = claude_project_directory(root, workspace) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten().take(MAX_TRANSCRIPTS_SCANNED) {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        // A transcript Claude wrote is named by a conversation id. Anything
        // else in this directory belongs to something other than a
        // conversation, and offering it as one would send Claude a resume it
        // cannot honour.
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
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().map(|(_, id)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_encodings_are_offered_because_claude_has_used_both() {
        assert_eq!(
            claude_project_slugs("/home/bee/projects/my.hive"),
            [
                "-home-bee-projects-my-hive".to_owned(),
                "-home-bee-projects-my.hive".to_owned(),
            ]
        );
    }

    #[test]
    fn transcripts_are_newest_first_and_non_conversations_are_left_out() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("-workspace");
        std::fs::create_dir_all(&directory).unwrap();
        let older = uuid::Uuid::now_v7().to_string();
        let newer = uuid::Uuid::now_v7().to_string();
        std::fs::write(directory.join(format!("{older}.jsonl")), b"{}").unwrap();
        // Written second, so its modification time is at or after the first.
        std::fs::write(directory.join(format!("{newer}.jsonl")), b"{}").unwrap();
        // Neither of these names a conversation, and offering one as a resume
        // would send Claude an id it cannot honour.
        std::fs::write(directory.join("notes.txt"), b"x").unwrap();
        std::fs::write(directory.join("not-a-uuid.jsonl"), b"{}").unwrap();

        let found = claude_conversations_newest_first(root.path(), "/workspace");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.contains(&older));
        assert!(found.contains(&newer));
    }

    #[test]
    fn a_workspace_claude_has_never_seen_lists_nothing_rather_than_failing() {
        let root = tempfile::tempdir().unwrap();
        assert!(claude_conversations_newest_first(root.path(), "/nowhere").is_empty());
    }
}
