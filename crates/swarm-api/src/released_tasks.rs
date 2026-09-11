//! A release closes the tickets it carried.
//!
//! ⚠️ THE STEP THIS REPLACES WAS ALWAYS GOING TO BE SKIPPED. The release
//! procedure ends with "record deployment evidence against any task this release
//! closes", nothing checked that it happened, and it was missed across several
//! releases -- most visibly on 2026-09-11, when the operator asked why three
//! tickets sat in Awaiting Release and two of them had been in people's hands
//! since v1.7.1 the day before.
//!
//! Awaiting Release completes ITSELF on a deployment record. So the missing
//! piece was never a state machine or a new tool to remember; it was that
//! nothing computed the answer `git tag --contains` already holds.
//!
//! WHAT THIS WILL NOT DO, and each restraint has a reason:
//!
//! - **Only Awaiting Release.** A deployment recorded against work still in
//!   Review does not close it -- that work has not been accepted yet -- and a
//!   sweep reaching into Review would be asserting acceptance nobody gave.
//! - **Only when the tag carries EVERY recorded commit.** Half-shipped work
//!   stays where a reader expects to find it.
//! - **Only when the tagged commit is on a remote branch.** A tag that exists
//!   solely on this machine is not a release; it is somebody's afternoon.
//! - **It never says "running".** A tag proves the work was released. Whether a
//!   given machine is running it is a different claim needing a different check,
//!   and the reference says so in as many words.

use std::path::Path;

use swarm_application::{release_carrying_every_commit, release_deployment_reference};
use swarm_domain::TaskCommitReport;

use crate::AppState;
use crate::runtime::git_output;

/// Where an automatic record says the work is.
///
/// DELIBERATELY NOT "production". Every deployment on this board written by a
/// person names a place they looked at and a version they read back. This one
/// looked at a tag. Reusing their word would make the two indistinguishable in
/// a list, and the weaker evidence would inherit the stronger reputation.
const RELEASE_ENVIRONMENT: &str = "release";

impl AppState {
    /// Records deployments for parked work that a tag now carries.
    ///
    /// Safe to run repeatedly: `record_task_deployment` is keyed on
    /// (task, environment, reference) and returns the earlier receipt rather
    /// than writing a second one. That matters more than it looks -- a task
    /// whose prerequisites are unresolved stays in Awaiting Release after a
    /// correct record, so this WILL see it again, and is supposed to.
    ///
    /// Synchronous on purpose: every call it makes blocks -- rusqlite and a git
    /// subprocess -- so it belongs on a blocking thread rather than pretending
    /// to yield. Its caller puts it on one.
    pub fn record_released_tasks(&self) {
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let reports = match store.awaiting_release_commit_reports() {
            Ok(reports) => reports,
            Err(error) => {
                tracing::warn!(%error, "could not read work parked in Awaiting Release");
                return;
            }
        };
        let now = chrono::Utc::now().timestamp();
        for report in reports {
            let Some(tag) = release_carrying(&report) else {
                continue;
            };
            let shas = report
                .commits
                .iter()
                .map(|commit| commit.sha.clone())
                .collect::<Vec<_>>();
            let reference = release_deployment_reference(&tag, &shas, &report.workspace);
            match store.record_task_deployment(report.task_id, RELEASE_ENVIRONMENT, &reference, now)
            {
                Ok(_) => {
                    tracing::info!(task = %report.task_id, %tag, "a release carried this task");
                    self.control_room_notify.notify_waiters();
                }
                Err(error) => {
                    tracing::warn!(task = %report.task_id, %tag, %error, "a released task could not be recorded");
                }
            }
        }
    }
}

/// The tag that carries all of this task's work, if one does.
///
/// Answers `None` the moment anything is unreadable. A workspace that has moved,
/// a commit that was rebased away, a git that failed -- none of those are
/// evidence of a release, and the cost of being wrong here is a ticket closed
/// against a release that does not contain it.
fn release_carrying(report: &TaskCommitReport) -> Option<String> {
    let workspace = Path::new(report.workspace.as_str());
    let mut tags_by_commit = Vec::with_capacity(report.commits.len());
    for commit in &report.commits {
        // Version order, so the first common tag is the release the work FIRST
        // reached people in rather than whichever one git happened to list.
        //
        // A RECORDED SHA NEEDS NO SHAPE CHECK, and this used to have one. The
        // worry was that `git tag --contains` has no `--` terminator, so a
        // stored value beginning with a dash would be read as an option. It is
        // not: `--contains` consumes the next argument as its VALUE. Measured
        // 2026-09-11 against git in this workspace -- `--sort=creatordate`,
        // `--merged`, `-n` and `-v1.8.0` each exit 129 with "malformed object
        // name", which arrives here as no output and therefore no release.
        // The guard was deleted rather than kept with a corrected comment: it
        // refused exactly what git refuses, and an ablation could not tell the
        // two apart.
        let listed = git_output(
            workspace,
            &["tag", "--sort=v:refname", "--contains", &commit.sha],
        )?;
        tags_by_commit.push(
            listed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
    }
    let tag = release_carrying_every_commit(&tags_by_commit)?;
    is_pushed(workspace, &tag).then_some(tag)
}

/// Whether the tagged commit has left this machine.
///
/// ANY remote branch, not origin/main, for the same reason
/// `development_source_status` asks it that way: the property is whether this
/// survives losing the machine. Remote-tracking refs can be stale, which can
/// only withhold a record that is due -- the safe direction.
///
/// The ref is spelled in full so a tag name can never be read as an option.
fn is_pushed(workspace: &Path, tag: &str) -> bool {
    git_output(
        workspace,
        &[
            "branch",
            "--remotes",
            "--contains",
            &format!("refs/tags/{tag}"),
        ],
    )
    .is_some_and(|branches| !branches.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, TaskCommit, TaskCommitReport, TaskId,
    };

    use super::release_carrying;

    /// A real repository, because the question is what git says.
    ///
    /// ⚠️ A MOCK WOULD BE WORTHLESS HERE. Everything this function decides is a
    /// fact about `git tag --contains` and `git branch --remotes --contains` --
    /// which tags they list, in what order, and what they do with a commit no
    /// ref reaches. A fixture returning strings would only be testing the
    /// strings this author expected git to produce.
    struct Repository {
        _root: tempfile::TempDir,
        checkout: std::path::PathBuf,
    }

    impl Repository {
        fn git(&self, arguments: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.checkout)
                .args(arguments)
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "git {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }

        fn commit(&self, message: &str) -> String {
            std::fs::write(self.checkout.join("file"), message).unwrap();
            self.git(&["add", "file"]);
            self.git(&["commit", "--quiet", "-m", message]);
            self.git(&["rev-parse", "--short=12", "HEAD"])
        }
    }

    /// A checkout with an `origin` it can actually push to, so "is this pushed"
    /// is answered by a remote-tracking ref rather than by a stub.
    fn repository() -> Repository {
        let root = tempfile::tempdir().unwrap();
        let origin = root.path().join("origin.git");
        let checkout = root.path().join("checkout");
        Command::new("git")
            .args(["init", "--quiet", "--bare", "--initial-branch=main"])
            .arg(&origin)
            .status()
            .expect("git runs");
        Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .arg(&checkout)
            .status()
            .expect("git runs");
        let repository = Repository {
            _root: root,
            checkout,
        };
        repository.git(&["config", "user.email", "test@example.invalid"]);
        repository.git(&["config", "user.name", "test"]);
        repository.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
        repository
    }

    fn report(checkout: &Path, shas: &[&str]) -> TaskCommitReport {
        TaskCommitReport {
            task_id: TaskId::new(),
            workspace: checkout.to_string_lossy().into_owned(),
            repository_state: CommitRepositoryState::Read,
            reported_at: 1_000,
            commits: shas
                .iter()
                .map(|sha| TaskCommit {
                    sha: (*sha).to_owned(),
                    verdict: CommitVerdict::Present,
                    subject: String::new(),
                    changed_paths: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_release_that_first_carried_every_commit_is_the_one_reported() {
        let repository = repository();
        let first = repository.commit("first");
        let second = repository.commit("second");
        repository.git(&["tag", "v1.7.1"]);
        repository.commit("third");
        repository.git(&["tag", "v1.8.0"]);
        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);

        assert_eq!(
            release_carrying(&report(&repository.checkout, &[&first, &second])).as_deref(),
            Some("v1.7.1"),
            "v1.7.1 carries both, and it shipped first"
        );
    }

    #[test]
    fn work_split_across_two_releases_waits_for_the_one_carrying_all_of_it() {
        let repository = repository();
        let early = repository.commit("early");
        repository.git(&["tag", "v1.7.1"]);
        let late = repository.commit("late");
        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);

        // The later commit is in no tag at all yet, so nothing carries the task.
        assert_eq!(
            release_carrying(&report(&repository.checkout, &[&early, &late])),
            None
        );

        repository.git(&["tag", "v1.8.0"]);
        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);
        assert_eq!(
            release_carrying(&report(&repository.checkout, &[&early, &late])).as_deref(),
            Some("v1.8.0")
        );
    }

    #[test]
    fn a_tag_that_never_left_this_machine_is_not_a_release() {
        // THE FAILURE THIS PREVENTS: somebody tags locally to try something, and
        // the board reports work as released that nobody outside this room can
        // install. Tagged is not shipped.
        let repository = repository();
        let only = repository.commit("only");
        repository.git(&["tag", "v9.9.9"]);

        assert_eq!(
            release_carrying(&report(&repository.checkout, &[&only])),
            None,
            "an unpushed tag must not close anything"
        );

        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);
        assert_eq!(
            release_carrying(&report(&repository.checkout, &[&only])).as_deref(),
            Some("v9.9.9"),
            "and pushing it is exactly what changes the answer"
        );
    }

    #[test]
    fn a_task_that_built_nothing_is_never_carried_by_a_release() {
        let repository = repository();
        repository.commit("only");
        repository.git(&["tag", "v1.0.0"]);
        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);

        assert_eq!(release_carrying(&report(&repository.checkout, &[])), None);
    }

    /// A stored SHA git cannot resolve reports no release, not every release.
    ///
    /// Covers the dash-leading case specifically, which is the one that would
    /// matter if `--contains` ever took its argument as an option instead of a
    /// value. It does not -- git exits 129 with "malformed object name" -- so
    /// this asserts the behaviour rather than a guard of ours, and it is here to
    /// notice if that ever changes.
    #[test]
    fn a_stored_sha_git_cannot_resolve_carries_nothing() {
        let repository = repository();
        repository.commit("only");
        repository.git(&["tag", "v1.0.0"]);
        repository.git(&["push", "--quiet", "origin", "main", "--tags"]);
        repository.git(&["fetch", "--quiet", "origin"]);

        for stored in ["--sort=creatordate", "-v1.0.0", "not-a-sha", ""] {
            assert_eq!(
                release_carrying(&report(&repository.checkout, &[stored])),
                None,
                "{stored:?} must carry nothing"
            );
        }
    }

    #[test]
    fn a_workspace_that_is_not_a_repository_carries_nothing() {
        let elsewhere = tempfile::tempdir().unwrap();
        assert_eq!(
            release_carrying(&report(elsewhere.path(), &["246e98ba"])),
            None
        );
    }
}
