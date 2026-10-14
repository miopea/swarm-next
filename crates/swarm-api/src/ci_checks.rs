//! Reads CI for the check gate (ADR 0115), through git and the machine's own
//! `gh` login.
//!
//! The operator ruled on 2026-10-13 (decision 01a1378c) that a Hive reads CI
//! with "Its gh CLI login": the login that already reaches the operator's
//! private repositories, with nothing new for Swarm to hold. So this asks `gh
//! api` and stores nothing it was given.
//!
//! What counts as passing, and what the gate does with it, are domain rules
//! (`swarm_domain::classify_checks`, `gate_verdict`). This only finds the commits
//! CI ran on and reads what it said.

use std::path::Path;
use std::process::Command;

use swarm_domain::{
    CheckRunReading, CiObservation, CiOutcome, StatusReading, classify_checks, combine_outcomes,
    commits_digest, no_checks_outcome,
};

use crate::AppState;
use crate::runtime::git_output;

/// How many pushed commits after a tip are tried when the tip itself has no
/// checks. CI usually runs only on the last commit of a push.
const DESCENDANTS_TRIED: usize = 20;
/// How many tasks one pass reads CI for.
const TASKS_PER_PASS: usize = 10;
/// How long one pass may keep starting tasks, so it ends soon after, whatever
/// GitHub is doing.
const PASS_SECONDS: u64 = 60;
/// How long one `gh api` call may take.
const GH_TIMEOUT_SECONDS: &str = "20";

/// CI as GitHub reports it for one commit.
pub(crate) trait CheckReader {
    /// The commit's check runs and statuses, or why they could not be read,
    /// in the operator's words.
    fn read(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<(Vec<CheckRunReading>, Vec<StatusReading>), String>;
}

/// Reads through `gh api`, as the machine's logged-in GitHub user.
pub(crate) struct GhCli;

impl GhCli {
    fn api<T: serde::de::DeserializeOwned>(path: &str, jq: &str) -> Result<T, String> {
        // `timeout` bounds a hung call; std's Command has no deadline of its own.
        // ⚠️ PLAIN JSON, WHATEVER THE ENVIRONMENT FORCES. A forced-colour
        // variable makes gh paint its JSON with escape codes even into a pipe,
        // and NO_COLOR alone does not beat CLICOLOR_FORCE. Seen on this box.
        let output = Command::new("timeout")
            .args([GH_TIMEOUT_SECONDS, "gh", "api", path, "--jq", jq])
            .env_remove("CLICOLOR_FORCE")
            .env_remove("FORCE_COLOR")
            .env_remove("GH_FORCE_TTY")
            .env("NO_COLOR", "1")
            .env("GH_PROMPT_DISABLED", "1")
            .output()
            .map_err(|_| "the timeout command is not available on this machine".to_owned())?;
        if output.status.success() {
            return serde_json::from_slice(&output.stdout)
                .map_err(|_| "GitHub answered in a shape Swarm did not expect".to_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(match output.status.code() {
            Some(124) => "GitHub did not answer within 20 seconds".to_owned(),
            Some(127) => swarm_domain::CI_GH_MISSING.to_owned(),
            _ if stderr.contains("gh auth login") || stderr.contains("not logged") => {
                swarm_domain::CI_GH_SIGNED_OUT.to_owned()
            }
            _ if stderr.contains("HTTP 404") => {
                "this machine's gh login can't see that repository".to_owned()
            }
            _ => format!(
                "GitHub could not be read: {}",
                stderr.lines().next().unwrap_or("no reason given").trim()
            ),
        })
    }
}

impl CheckReader for GhCli {
    fn read(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<(Vec<CheckRunReading>, Vec<StatusReading>), String> {
        let runs = Self::api(
            &format!("repos/{repository}/commits/{sha}/check-runs?per_page=100"),
            "[.check_runs[] | {name, status, conclusion}]",
        )?;
        let statuses = Self::api(
            &format!("repos/{repository}/commits/{sha}/status"),
            "[.statuses[] | {context, state}]",
        )?;
        Ok((runs, statuses))
    }
}

/// `owner/name` for a GitHub remote, or `None` for anything else.
fn github_repository(remote: &str) -> Option<String> {
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .or_else(|| remote.strip_prefix("https://github.com/"))?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{owner}/{name}"))
}

/// Reads CI for one task's recorded commits in `workspace`.
pub(crate) fn observe_ci(
    workspace: &Path,
    commits: &[String],
    reader: &dyn CheckReader,
    now: i64,
) -> CiObservation {
    let mut judged = Vec::new();
    let outcome = judge(workspace, commits, reader, now, &mut judged);
    CiObservation {
        commits: commits_digest(commits),
        judged,
        outcome,
        observed_at: now,
    }
}

fn judge(
    workspace: &Path,
    commits: &[String],
    reader: &dyn CheckReader,
    now: i64,
    judged: &mut Vec<String>,
) -> CiOutcome {
    let unreadable = |why: &str| CiOutcome::Unreadable {
        why: why.to_owned(),
    };
    let Some(repository) = git_output(workspace, &["remote", "get-url", "origin"])
        .and_then(|remote| github_repository(&remote))
    else {
        return unreadable("this repository has no GitHub remote named origin");
    };
    let mut full = Vec::new();
    for sha in commits {
        let Some(resolved) = git_output(
            workspace,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{sha}^{{commit}}"),
            ],
        ) else {
            return unreadable(&format!("commit {sha} is not in this repository"));
        };
        full.push(resolved);
    }
    // The tips of the work: commits no other recorded commit builds on.
    let tips: Vec<&String> = full
        .iter()
        .filter(|candidate| {
            !full.iter().any(|other| {
                other != *candidate
                    && git_output(
                        workspace,
                        &["merge-base", "--is-ancestor", candidate, other],
                    )
                    .is_some()
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for tip in tips {
        outcomes.push(judge_tip(workspace, &repository, tip, reader, now, judged));
    }
    combine_outcomes(outcomes)
}

fn judge_tip(
    workspace: &Path,
    repository: &str,
    tip: &str,
    reader: &dyn CheckReader,
    now: i64,
    judged: &mut Vec<String>,
) -> CiOutcome {
    let Some(branch) = pushed_branch(workspace, tip) else {
        judged.push(tip.to_owned());
        return CiOutcome::Pending {
            waiting: vec!["the push to GitHub".to_owned()],
        };
    };
    let after = git_output(
        workspace,
        &[
            "rev-list",
            "--reverse",
            "--ancestry-path",
            &format!("{tip}..{branch}"),
        ],
    )
    .unwrap_or_default();
    let candidates = std::iter::once(tip.to_owned())
        .chain(after.lines().map(str::to_owned).take(DESCENDANTS_TRIED));
    let mut looked_at: usize = 0;
    for candidate in candidates {
        match reader.read(repository, &candidate) {
            Err(why) => {
                judged.push(candidate);
                return CiOutcome::Unreadable { why };
            }
            Ok((runs, statuses)) => {
                if let Some(outcome) = classify_checks(&runs, &statuses) {
                    judged.push(candidate);
                    return outcome;
                }
            }
        }
        looked_at += 1;
    }
    judged.push(tip.to_owned());
    let made_at = git_output(workspace, &["show", "-s", "--format=%ct", tip])
        .and_then(|seconds| seconds.parse::<i64>().ok())
        .unwrap_or(0);
    no_checks_outcome(now - made_at, looked_at.saturating_sub(1))
}

/// The remote-tracking branch carrying `sha`, preferring origin's default.
fn pushed_branch(workspace: &Path, sha: &str) -> Option<String> {
    let carrying = git_output(
        workspace,
        &[
            "branch",
            "--remotes",
            "--contains",
            sha,
            "--format=%(refname:short)",
        ],
    )?;
    let carrying: Vec<&str> = carrying
        .lines()
        .map(str::trim)
        .filter(|branch| {
            branch.starts_with("origin/") && *branch != "origin/HEAD" && *branch != "origin"
        })
        .collect();
    let default = git_output(
        workspace,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    );
    default
        .filter(|default| carrying.contains(&default.as_str()))
        .or_else(|| carrying.first().map(|branch| (*branch).to_owned()))
}

impl AppState {
    /// Reads CI for the tasks the check gate is waiting on, a bounded number
    /// per pass, and stores what it says.
    ///
    /// Synchronous on purpose, like the release sweep: git and `gh` are
    /// subprocesses and the store is rusqlite. Its caller puts it on a blocking
    /// thread.
    pub fn observe_task_ci(&self) {
        let Some(store) = self.task_store.as_ref() else {
            return;
        };
        let now = crate::unix_timestamp();
        let candidates = match store.ci_observation_candidates(now, TASKS_PER_PASS) {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(error = %error, "the check gate could not list work to read CI for");
                return;
            }
        };
        let started = std::time::Instant::now();
        for candidate in candidates {
            if started.elapsed() >= std::time::Duration::from_secs(PASS_SECONDS) {
                break;
            }
            let observed = observe_ci(
                Path::new(&candidate.workspace),
                &candidate.commits,
                &GhCli,
                now,
            );
            if let Err(error) = store.record_ci_observation(candidate.task_id, &observed) {
                tracing::warn!(error = %error, task_id = %candidate.task_id, "the check gate could not store a CI reading");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;

    struct Repository {
        _root: tempfile::TempDir,
        checkout: PathBuf,
    }

    impl Repository {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let checkout = root.path().join("checkout");
            Command::new("git")
                .args(["init", "--quiet", "--initial-branch=main"])
                .arg(&checkout)
                .status()
                .expect("git runs");
            let repository = Self {
                _root: root,
                checkout,
            };
            repository.git(&["config", "user.email", "test@example.invalid"]);
            repository.git(&["config", "user.name", "test"]);
            // Never contacted: a remote-tracking ref stands in for the push.
            repository.git(&["remote", "add", "origin", "git@github.com:acme/petal.git"]);
            repository
        }

        fn git(&self, arguments: &[&str]) -> String {
            git_output(&self.checkout, arguments).unwrap_or_default()
        }

        fn commit(&self, message: &str) -> String {
            std::fs::write(self.checkout.join("file"), message).unwrap();
            self.git(&["add", "file"]);
            self.git(&["commit", "--quiet", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }

        fn pushed_to(&self, sha: &str) {
            self.git(&["update-ref", "refs/remotes/origin/main", sha]);
        }
    }

    /// GitHub as a table: each commit's checks, or a reason it can't be read.
    #[derive(Default)]
    struct FakeGitHub {
        checks: HashMap<String, Vec<CheckRunReading>>,
        failure: Option<String>,
        asked: RefCell<Vec<(String, String)>>,
    }

    impl CheckReader for FakeGitHub {
        fn read(
            &self,
            repository: &str,
            sha: &str,
        ) -> Result<(Vec<CheckRunReading>, Vec<StatusReading>), String> {
            self.asked
                .borrow_mut()
                .push((repository.to_owned(), sha.to_owned()));
            if let Some(why) = &self.failure {
                return Err(why.clone());
            }
            Ok((
                self.checks.get(sha).cloned().unwrap_or_default(),
                Vec::new(),
            ))
        }
    }

    fn passed(name: &str) -> CheckRunReading {
        CheckRunReading {
            name: name.to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
        }
    }

    /// The real thing, through this machine's gh login. Ignored by default:
    /// CI has no gh login, and a test that needs the network is not a unit
    /// test. Run with `--ignored` on a machine that has one.
    #[test]
    #[ignore = "reads real GitHub through this machine's gh login"]
    fn gh_cli_reads_this_repositorys_real_ci() {
        let (runs, _statuses) = GhCli
            .read(
                "miopea/swarm-next",
                "d9e716058daf6414266ae0e06857b9868fd7ce2d",
            )
            .expect("this machine's gh login reads swarm-next");
        assert_eq!(
            classify_checks(&runs, &[]),
            Some(CiOutcome::Passed),
            "release 1.17.2's manifest commit passed all four checks: {runs:?}"
        );
        assert_eq!(
            GhCli
                .read(
                    "miopea/no-such-repository-here",
                    "d9e716058daf6414266ae0e06857b9868fd7ce2d"
                )
                .err()
                .as_deref(),
            Some("this machine's gh login can't see that repository")
        );
    }

    #[test]
    fn only_github_remotes_name_a_repository() {
        for remote in [
            "git@github.com:acme/petal.git",
            "ssh://git@github.com/acme/petal.git",
            "https://github.com/acme/petal",
            "https://github.com/acme/petal.git/",
        ] {
            assert_eq!(
                github_repository(remote).as_deref(),
                Some("acme/petal"),
                "{remote}"
            );
        }
        for remote in [
            "git@gitlab.com:acme/petal.git",
            "/srv/git/petal.git",
            "https://github.com/acme",
        ] {
            assert_eq!(github_repository(remote), None, "{remote}");
        }
    }

    /// ⚠️ CI USUALLY RUNS ONLY ON THE LAST COMMIT OF A PUSH. A task whose own
    /// commit was not that one is judged by the first pushed commit after it
    /// that CI did run on, never by an empty list read as "no CI".
    #[test]
    fn a_commit_ci_skipped_is_judged_by_the_pushed_commit_after_it() {
        let repository = Repository::new();
        let first = repository.commit("first");
        let task_tip = repository.commit("the task's tip");
        let later = repository.commit("someone else's, pushed with it");
        repository.pushed_to(&later);
        let github = FakeGitHub {
            checks: HashMap::from([(later.clone(), vec![passed("rust")])]),
            ..FakeGitHub::default()
        };

        let observed = observe_ci(
            &repository.checkout,
            &[first.clone(), task_tip.clone()],
            &github,
            2_000_000_000,
        );

        assert_eq!(observed.outcome, CiOutcome::Passed);
        assert_eq!(observed.judged, vec![later.clone()]);
        assert_eq!(observed.commits, commits_digest(&[first, task_tip.clone()]));
        assert_eq!(
            github.asked.borrow().as_slice(),
            &[
                ("acme/petal".to_owned(), task_tip),
                ("acme/petal".to_owned(), later)
            ],
            "only the tip and what came after it, never the commit beneath the tip"
        );
    }

    #[test]
    fn unpushed_work_waits_and_unreadable_ci_says_why() {
        let repository = Repository::new();
        let base = repository.commit("base");
        repository.pushed_to(&base);
        let unpushed = repository.commit("not pushed yet");
        let github = FakeGitHub::default();
        let observed = observe_ci(
            &repository.checkout,
            std::slice::from_ref(&unpushed),
            &github,
            2_000_000_000,
        );
        assert_eq!(
            observed.outcome,
            CiOutcome::Pending {
                waiting: vec!["the push to GitHub".to_owned()]
            }
        );
        assert!(
            github.asked.borrow().is_empty(),
            "nothing to ask GitHub about yet"
        );

        repository.pushed_to(&unpushed);
        let signed_out = FakeGitHub {
            failure: Some("gh is not logged in on this machine".to_owned()),
            ..FakeGitHub::default()
        };
        assert_eq!(
            observe_ci(
                &repository.checkout,
                std::slice::from_ref(&unpushed),
                &signed_out,
                2_000_000_000
            )
            .outcome,
            CiOutcome::Unreadable {
                why: "gh is not logged in on this machine".to_owned()
            }
        );

        // Pushed long ago with no checks anywhere: a repository without CI.
        let quiet = observe_ci(
            &repository.checkout,
            &[unpushed],
            &FakeGitHub::default(),
            2_000_000_000,
        );
        assert!(
            matches!(quiet.outcome, CiOutcome::Unreadable { why } if why.contains("no CI ran"))
        );

        let elsewhere = observe_ci(
            &repository.checkout,
            &["0123456789abcdef0123456789abcdef01234567".to_owned()],
            &FakeGitHub::default(),
            2_000_000_000,
        );
        assert!(
            matches!(elsewhere.outcome, CiOutcome::Unreadable { why } if why.contains("is not in this repository"))
        );
    }
}
