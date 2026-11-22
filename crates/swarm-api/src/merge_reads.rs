//! When parked work merged, read from each workspace's own refs (ADR 0180).
//!
//! Swarm stores no merge time, and the stall alert needs one: a wait counted
//! from entering Awaiting Release alarms on work that has not merged yet,
//! which more than half of the measured stays had not.
//!
//! WHAT THIS WILL NOT DO:
//!
//! - **It never fetches.** It reads the refs the workspace already has. A
//!   stale ref only delays the row, and measured on this Hive a workspace's
//!   remote-tracking ref carried the merge within 15 minutes in 90% of stays.
//! - **It does not guess.** A workspace git cannot read, or a commit it cannot
//!   place, records nothing, so the next sweep asks again.
//! - **It does not see squash or rebase merges.** Their recorded commits never
//!   reach the default branch, so such a stay reads as not merged.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use swarm_persistence::ReleaseWaitMerge;

use crate::AppState;
use crate::runtime::git_output;

impl AppState {
    /// The release sweep: records the parked work a pushed tag released, then
    /// when the work still parked merged, which the release-wait alert
    /// decides from (ADR 0180). One thread and one cadence, because both read
    /// the same workspaces' refs.
    pub fn sweep_releases(&self) {
        self.record_released_tasks();
        self.read_release_waits();
    }

    /// Reads the merge of every stay in Awaiting Release, and a bounded batch
    /// of ended ones, and records what git said.
    ///
    /// Synchronous on purpose, like `record_released_tasks`: it runs git and
    /// holds a database connection, so its caller puts it on a blocking thread.
    pub fn read_release_waits(&self) {
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let now = chrono::Utc::now().timestamp();
        let reads = match store.release_wait_reads(now) {
            Ok(reads) => reads,
            Err(error) => {
                tracing::warn!(%error, "could not list the stays whose merge to read");
                return;
            }
        };
        let mut branches: HashMap<String, Option<String>> = HashMap::new();
        let mut observed = Vec::with_capacity(reads.len());
        for read in reads {
            let workspace = Path::new(read.workspace.as_str());
            let branch = branches
                .entry(read.workspace.clone())
                .or_insert_with(|| default_branch(workspace))
                .clone();
            let Some(branch) = branch else {
                continue;
            };
            if let Some(merge) = merge_of_stay(workspace, &branch, &read.commits) {
                observed.push((read, merge));
            }
        }
        if let Err(error) = store.record_release_waits(&observed, now) {
            tracing::warn!(%error, "could not record when parked work merged");
        }
    }
}

/// The workspace's remote default branch: what `origin/HEAD` names, or
/// `origin/main` when that is not set.
fn default_branch(workspace: &Path) -> Option<String> {
    if let Some(head) = git_output(
        workspace,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .filter(|head| !head.is_empty())
    {
        return Some(head);
    }
    git_output(
        workspace,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ],
    )
    .map(|_| "refs/remotes/origin/main".to_owned())
}

/// Where one commit stands against the default branch.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CommitMerge {
    /// On the branch: the merge time and the commit that brought it.
    Merged(i64, String),
    /// Not on the branch.
    NotMerged,
}

/// When all of a stay's commits had merged, or that one has not. `None` when
/// git could not say.
fn merge_of_stay(workspace: &Path, branch: &str, commits: &[String]) -> Option<ReleaseWaitMerge> {
    let mut latest: Option<(i64, String)> = None;
    for commit in commits {
        match merge_of_commit(workspace, branch, commit)? {
            CommitMerge::NotMerged => return Some(ReleaseWaitMerge::NotMerged),
            CommitMerge::Merged(merged_at, merge_commit) => {
                if latest.as_ref().is_none_or(|(time, _)| merged_at > *time) {
                    latest = Some((merged_at, merge_commit));
                }
            }
        }
    }
    let (merged_at, merge_commit) = latest?;
    Some(ReleaseWaitMerge::Merged {
        merged_at,
        merge_commit,
    })
}

/// When one commit reached the default branch, and the commit that brought it.
///
/// That is the first commit on the branch's first-parent line that contains
/// it: the merge commit of a merged pull request, or the commit itself when it
/// was pushed straight onto the line. Its committer time is the merge time.
///
/// `None` when git could not say.
fn merge_of_commit(workspace: &Path, branch: &str, commit: &str) -> Option<CommitMerge> {
    // A recorded commit is a hex name. Anything else could be read as an
    // option or a revision expression, and is not evidence of a merge.
    if !(7..=64).contains(&commit.len()) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if !is_ancestor(workspace, commit, branch)? {
        return Some(CommitMerge::NotMerged);
    }
    let line = git_output(
        workspace,
        &[
            "rev-list",
            "--first-parent",
            "--ancestry-path",
            &format!("{commit}..{branch}"),
        ],
    )?;
    // Newest first, so the last is the first on the line to contain it.
    let mut bringer = line
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_owned);
    if let Some(candidate) = bringer.as_deref() {
        // The commit was already on the line when its parent there contains
        // it: it was pushed onto the line rather than merged into it.
        let parents = git_output(workspace, &["show", "-s", "--format=%P", candidate])?;
        let on_line = match parents.split_whitespace().next() {
            Some(first_parent) => is_ancestor(workspace, commit, first_parent)?,
            None => false,
        };
        if on_line {
            bringer = None;
        }
    }
    let bringer = bringer.unwrap_or_else(|| commit.to_owned());
    let shown = git_output(workspace, &["show", "-s", "--format=%H %ct", &bringer])?;
    let (sha, time) = shown.split_once(' ')?;
    Some(CommitMerge::Merged(
        time.trim().parse().ok()?,
        sha.to_owned(),
    ))
}

/// Whether `ancestor` is an ancestor of `descendant`. `None` when git failed.
fn is_ancestor(workspace: &Path, ancestor: &str, descendant: &str) -> Option<bool> {
    let status = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()
        .ok()?
        .status;
    match status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use swarm_persistence::ReleaseWaitMerge;

    use super::{CommitMerge, default_branch, merge_of_commit, merge_of_stay};

    /// A real repository with a real remote, because every answer here is a
    /// fact about what git says. A stub would only repeat this author's
    /// expectations of git back to them.
    struct Repository {
        _root: tempfile::TempDir,
        checkout: PathBuf,
    }

    impl Repository {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let remote = root.path().join("remote.git");
            let checkout = root.path().join("checkout");
            run(
                root.path(),
                &[
                    "init",
                    "--quiet",
                    "--bare",
                    "-b",
                    "main",
                    remote.to_str().unwrap(),
                ],
            );
            run(
                root.path(),
                &["init", "--quiet", "-b", "main", checkout.to_str().unwrap()],
            );
            let repository = Self {
                _root: root,
                checkout,
            };
            repository.git(&["config", "user.email", "test@example.test"]);
            repository.git(&["config", "user.name", "Test"]);
            repository.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
            repository.commit("base", 1_000);
            repository.git(&["push", "--quiet", "origin", "main"]);
            repository
        }

        fn git(&self, arguments: &[&str]) -> String {
            run(&self.checkout, arguments)
        }

        fn git_at(&self, arguments: &[&str], time: i64) -> String {
            let date = format!("@{time} +0000");
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.checkout)
                .args(arguments)
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }

        fn commit(&self, message: &str, time: i64) -> String {
            std::fs::write(self.checkout.join(message), message).unwrap();
            self.git(&["add", message]);
            self.git_at(&["commit", "--quiet", "-m", message], time);
            self.git(&["rev-parse", "HEAD"])
        }

        fn path(&self) -> &Path {
            &self.checkout
        }
    }

    fn run(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn a_merged_pull_request_is_dated_by_its_merge_commit() {
        let repository = Repository::new();
        repository.git(&["checkout", "--quiet", "-b", "feature"]);
        let work = repository.commit("work", 2_000);
        repository.git(&["checkout", "--quiet", "main"]);
        repository.commit("meanwhile", 2_500);
        repository.git_at(
            &["merge", "--quiet", "--no-ff", "-m", "merge", "feature"],
            3_000,
        );
        let merge = repository.git(&["rev-parse", "HEAD"]);
        repository.commit("after", 4_000);
        repository.git(&["push", "--quiet", "origin", "main"]);
        repository.git(&["fetch", "--quiet", "origin"]);

        let branch = default_branch(repository.path()).unwrap();
        assert_eq!(branch, "refs/remotes/origin/main");
        assert_eq!(
            merge_of_commit(repository.path(), &branch, &work[..12]),
            Some(CommitMerge::Merged(3_000, merge)),
            "the merge, not the commit's own time or a later commit's"
        );
    }

    #[test]
    fn a_commit_pushed_onto_the_line_is_dated_by_itself() {
        let repository = Repository::new();
        let direct = repository.commit("direct", 2_000);
        repository.commit("after", 3_000);
        repository.git(&["push", "--quiet", "origin", "main"]);
        let branch = default_branch(repository.path()).unwrap();
        assert_eq!(
            merge_of_commit(repository.path(), &branch, &direct),
            Some(CommitMerge::Merged(2_000, direct.clone()))
        );
        let tip = repository.git(&["rev-parse", "HEAD"]);
        assert_eq!(
            merge_of_commit(repository.path(), &branch, &tip),
            Some(CommitMerge::Merged(3_000, tip)),
            "the branch tip itself"
        );
    }

    #[test]
    fn work_the_remote_branch_does_not_carry_is_not_merged() {
        let repository = Repository::new();
        let first = repository.commit("first", 2_000);
        let branch = default_branch(repository.path()).unwrap();
        assert_eq!(
            merge_of_commit(repository.path(), &branch, &first),
            Some(CommitMerge::NotMerged)
        );
        repository.git(&["push", "--quiet", "origin", "main"]);
        let later = repository.commit("later", 3_000);
        assert_eq!(
            merge_of_stay(repository.path(), &branch, &[first.clone(), later]),
            Some(ReleaseWaitMerge::NotMerged),
            "one commit not on the branch means the stay has not merged"
        );
        assert_eq!(
            merge_of_stay(repository.path(), &branch, std::slice::from_ref(&first)),
            Some(ReleaseWaitMerge::Merged {
                merged_at: 2_000,
                merge_commit: first,
            })
        );
    }

    #[test]
    fn what_git_cannot_read_records_nothing() {
        let repository = Repository::new();
        let branch = default_branch(repository.path()).unwrap();
        assert_eq!(merge_of_commit(repository.path(), &branch, "--all"), None);
        assert_eq!(merge_of_commit(repository.path(), &branch, "HEAD~1"), None);
        assert_eq!(
            merge_of_commit(repository.path(), &branch, "0123456789abcdef0123"),
            None,
            "an unknown commit is not a commit that has not merged"
        );
        let nowhere = tempfile::tempdir().unwrap();
        assert_eq!(default_branch(nowhere.path()), None);
    }
}
