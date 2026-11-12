//! What a worker's independent checker is given (ADR 0171).
//!
//! Swarm builds the checker's input from the task and its recorded commits.
//! The worker hands it to a fresh agent and records the answer, and adds
//! nothing of its own. A worker who writes the checker's brief can steer it
//! ("already run, do NOT re-run"), and a check steered by the work's author
//! reads the work the way the author does.
//!
//! Every git read here is bounded in time and in bytes. A change too large to
//! include goes to the checker as its commits and paths, with the command to
//! read each one, rather than as a diff cut off part way.

use std::fmt::Write as _;
use std::process::Stdio;
use std::time::Duration;

use swarm_domain::{CommitVerdict, Task, TaskAmendment, TaskCommitReport};
use tokio::io::AsyncReadExt as _;
use tokio::time::Instant;

/// The most patch text one input carries. Past it the checker is given the
/// commits and their paths and reads the changes itself.
const PATCH_BUDGET_BYTES: usize = 48 * 1024;
/// The most a single commit's summary line read may return.
const META_CAP_BYTES: usize = 4 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Every git read for one call, together. A task's reports can name hundreds
/// of commits, and ten seconds each in a repository git is slow to read would
/// hold the worker's call for minutes. Past it, commits go unread and the
/// checker is sent to git for the changes.
const ALL_READS_DEADLINE: Duration = Duration::from_secs(30);

/// What one bounded git read came back with.
#[derive(Debug, PartialEq, Eq)]
enum GitRead {
    Text(String),
    /// More than the cap. The process was stopped, not waited for.
    TooLarge,
    /// Not a repository, an unknown commit, git missing, or too slow.
    Failed,
}

async fn bounded_git(workspace: &str, args: &[&str], cap: usize, deadline: Instant) -> GitRead {
    if Instant::now() >= deadline {
        return GitRead::Failed;
    }
    let Ok(mut child) = tokio::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return GitRead::Failed;
    };
    let Some(stdout) = child.stdout.take() else {
        return GitRead::Failed;
    };
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    let read = async {
        let mut buffer = Vec::new();
        stdout.take(limit).read_to_end(&mut buffer).await.ok()?;
        if buffer.len() > cap {
            // Dropping the child stops it; there is no point waiting for the
            // rest of a read that is already too large.
            return Some((buffer, None));
        }
        let status = child.wait().await.ok()?;
        Some((buffer, Some(status)))
    };
    let stop = deadline.min(Instant::now() + GIT_TIMEOUT);
    match tokio::time::timeout_at(stop, read).await {
        Ok(Some((buffer, None))) if buffer.len() > cap => GitRead::TooLarge,
        Ok(Some((buffer, Some(status)))) if status.success() => {
            GitRead::Text(String::from_utf8_lossy(&buffer).into_owned())
        }
        _ => GitRead::Failed,
    }
}

/// One recorded commit as the checker is shown it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckedCommit {
    /// As recorded: the token the basis is built from.
    pub recorded: String,
    /// The full SHA, when git could read it.
    pub full: Option<String>,
    pub subject: String,
    pub paths: Vec<String>,
    /// The repository it was read in.
    pub workspace: String,
    committed_at: i64,
    /// Full parent SHAs, as git read them.
    parents: Vec<String>,
}

impl CheckedCommit {
    fn shown(&self) -> &str {
        self.full.as_deref().unwrap_or(&self.recorded)
    }

    fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// The task's recorded commits, oldest first, as far as git could order them.
/// A commit git could not read keeps its recorded place at the end.
pub(crate) async fn ordered_commits(report: &TaskCommitReport) -> Vec<CheckedCommit> {
    ordered_commits_until(report, Instant::now() + ALL_READS_DEADLINE).await
}

async fn ordered_commits_until(report: &TaskCommitReport, deadline: Instant) -> Vec<CheckedCommit> {
    let mut read = Vec::new();
    let mut unread = Vec::new();
    for commit in &report.commits {
        let workspace = commit
            .found_in
            .clone()
            .unwrap_or_else(|| report.workspace.clone());
        let mut checked = CheckedCommit {
            recorded: commit.sha.clone(),
            full: None,
            subject: commit.subject.clone(),
            paths: commit.changed_paths.clone(),
            workspace,
            committed_at: 0,
            parents: Vec::new(),
        };
        let readable = matches!(
            commit.verdict,
            CommitVerdict::Present | CommitVerdict::Unreachable | CommitVerdict::InAnotherWorkspace
        );
        if readable
            && let GitRead::Text(meta) = bounded_git(
                &checked.workspace,
                &["show", "-s", "--format=%H%x1f%ct%x1f%P", &commit.sha],
                META_CAP_BYTES,
                deadline,
            )
            .await
        {
            let mut fields = meta.trim().split('\u{1f}');
            let full = fields.next().unwrap_or_default();
            if !full.is_empty() {
                checked.full = Some(full.to_owned());
                checked.committed_at = fields
                    .next()
                    .and_then(|time| time.parse().ok())
                    .unwrap_or_default();
                checked.parents = fields
                    .next()
                    .map(|parents| parents.split_whitespace().map(str::to_owned).collect())
                    .unwrap_or_default();
                read.push(checked);
                continue;
            }
        }
        unread.push(checked);
    }
    read.sort_by(|left, right| {
        left.committed_at
            .cmp(&right.committed_at)
            .then_with(|| left.recorded.cmp(&right.recorded))
    });
    let mut ordered = parents_first_within_a_second(read);
    ordered.extend(unread);
    ordered
}

/// Commit times are whole seconds, so commits made in quick succession tie.
/// Within a tie, a commit whose parent is also in the tie goes after it.
/// Ancestry through a commit that was not recorded is not seen, and those
/// keep the order they were sorted in.
fn parents_first_within_a_second(sorted: Vec<CheckedCommit>) -> Vec<CheckedCommit> {
    let mut ordered = Vec::with_capacity(sorted.len());
    let mut tie: Vec<CheckedCommit> = Vec::new();
    for commit in sorted {
        if tie
            .first()
            .is_some_and(|first| first.committed_at != commit.committed_at)
        {
            place_parents_first(&mut tie, &mut ordered);
        }
        tie.push(commit);
    }
    place_parents_first(&mut tie, &mut ordered);
    ordered
}

fn place_parents_first(tie: &mut Vec<CheckedCommit>, ordered: &mut Vec<CheckedCommit>) {
    while !tie.is_empty() {
        let in_tie = |sha: &String| tie.iter().any(|other| other.full.as_ref() == Some(sha));
        // Git history has no cycles, so one always qualifies; the fallback
        // only keeps this total if a repository were ever corrupt.
        let next = tie
            .iter()
            .position(|commit| !commit.parents.iter().any(in_tie))
            .unwrap_or(0);
        ordered.push(tie.remove(next));
    }
}

/// The newest commit the check covers, for people to read. Decides nothing.
pub(crate) fn head_of(commits: &[CheckedCommit]) -> String {
    commits
        .iter()
        .rev()
        .find(|commit| commit.full.is_some())
        .or_else(|| commits.last())
        .map(|commit| commit.shown().to_owned())
        .unwrap_or_default()
}

/// The changes, or `None` when they are over the budget or the reads ran out
/// of time.
///
/// A merge is left out when the task also recorded ordinary commits, because
/// its first-parent diff repeats them. A task that recorded only a merge gets
/// the merge's diff, which is then the whole change.
async fn read_patches(
    commits: &[CheckedCommit],
    deadline: Instant,
) -> Option<Vec<(String, String)>> {
    let any_ordinary = commits
        .iter()
        .any(|commit| commit.full.is_some() && !commit.is_merge());
    let mut remaining = PATCH_BUDGET_BYTES;
    let mut patches = Vec::new();
    for commit in commits {
        let Some(full) = &commit.full else {
            continue;
        };
        if commit.is_merge() && any_ordinary {
            continue;
        }
        match bounded_git(
            &commit.workspace,
            &[
                "show",
                "--no-color",
                "--no-ext-diff",
                "--format=",
                "--stat",
                "--patch",
                "-m",
                "--first-parent",
                full,
            ],
            remaining,
            deadline,
        )
        .await
        {
            GitRead::Text(patch) => {
                remaining = remaining.saturating_sub(patch.len());
                patches.push((full.clone(), patch));
            }
            GitRead::TooLarge => return None,
            // Some changes unread for want of time is not the whole change.
            GitRead::Failed if Instant::now() >= deadline => return None,
            GitRead::Failed => {}
        }
    }
    Some(patches)
}

/// The checker's input, built for one task.
pub(crate) struct CheckInput {
    pub head: String,
    pub commits: Vec<CheckedCommit>,
    pub diff_included: bool,
    pub prompt: String,
}

pub(crate) async fn build_check_input(
    task: &Task,
    amendments: &[TaskAmendment],
    report: &TaskCommitReport,
) -> CheckInput {
    let deadline = Instant::now() + ALL_READS_DEADLINE;
    let commits = ordered_commits_until(report, deadline).await;
    let patches = read_patches(&commits, deadline).await;
    CheckInput {
        head: head_of(&commits),
        diff_included: patches.is_some(),
        prompt: checker_prompt(task, amendments, &commits, patches.as_deref()),
        commits,
    }
}

/// The words the checker is given. Pure, so what a checker reads can be
/// tested without a repository.
///
/// ⚠️ THERE IS NO PARAMETER FOR THE WORKER'S VIEW, and that is the design. The
/// operator chose "diff and task brief only": the worker's summary, its list
/// of what it already ran and what to skip, never reaches the checker.
fn checker_prompt(
    task: &Task,
    amendments: &[TaskAmendment],
    commits: &[CheckedCommit],
    patches: Option<&[(String, String)]>,
) -> String {
    let mut prompt = String::from(
        "You are checking another agent's work on a task. You have not seen its reasoning \
         or its own summary, only what it was asked to do and what it changed. Form your own view.\n\
         \n\
         RULES\n\
         - Read only. Do not edit files, commit, push, or change anything in the repository or anywhere else.\n\
         - Judge what is in front of you. Do not ask the author what they meant.\n\
         \n\
         THE TASK\n",
    );
    let _ = writeln!(prompt, "Title: {}\n", task.title.trim());
    let description = task.description.trim();
    if description.is_empty() {
        prompt.push_str("(The task has no description beyond its title.)\n");
    } else {
        prompt.push_str(description);
        prompt.push('\n');
    }
    if !amendments.is_empty() {
        prompt.push_str(
            "\nCorrections made to the description later. Where they disagree with it, they are right:\n",
        );
        for amendment in amendments {
            let _ = writeln!(prompt, "- {}", amendment.body.trim());
        }
    }
    prompt.push_str("\nTHE WORK\nCommits, oldest first:\n");
    for commit in commits {
        let short = commit.shown().get(..12).unwrap_or(commit.shown());
        let subject = commit.subject.trim();
        let _ = write!(prompt, "- {short} {subject}");
        if commit.full.is_none() {
            prompt.push_str("  (git could not read this commit)");
        }
        let _ = writeln!(prompt, "\n  in {}", commit.workspace);
        for path in &commit.paths {
            let _ = writeln!(prompt, "    {path}");
        }
    }
    match patches {
        Some(patches) if !patches.is_empty() => {
            prompt.push_str("\nThe changes:\n");
            for (sha, patch) in patches {
                let _ = writeln!(prompt, "\n=== {sha}\n{}", patch.trim_end());
            }
        }
        Some(_) => {
            prompt.push_str(
                "\nNone of the changes could be read here. Read each commit with git show in its repository.\n",
            );
        }
        None => {
            prompt.push_str(
                "\nThe changes are not included here: they are too large, or git took too long to \
                 read them. Read each commit yourself with `git -C <repository> show <commit>`, \
                 using the repository named under it.\n",
            );
        }
    }
    prompt.push_str(
        "\nWHAT TO CHECK\n\
         1. Does the change do what the task asks? Name anything the task asks for that is missing.\n\
         2. Does it change anything the task did not ask for?\n\
         3. Were any tests removed, skipped or weakened: deleted cases, loosened assertions, wider mocks?\n\
         4. Run the repository's own checks, such as its test or verify script, and give each command's exit code. If you cannot run them, say why.\n\
         \n\
         ANSWER WITH\n\
         - verdict: pass, partial or fail\n\
         - ran: the commands you ran, or \"read only\" if you ran none\n\
         - findings: each problem with its file and line, or none\n",
    );
    prompt
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, TaskAmendment, TaskCommit, TaskCommitReport, TaskId,
        WorkerId,
    };
    use tokio::time::Instant;

    use super::{
        CheckedCommit, GitRead, PATCH_BUDGET_BYTES, bounded_git, build_check_input, checker_prompt,
        head_of, ordered_commits, ordered_commits_until, read_patches,
    };

    pub(crate) fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn later() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    pub(crate) fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        root
    }

    pub(crate) fn commit_file(repo: &Path, name: &str, body: &str, message: &str) -> String {
        std::fs::write(repo.join(name), body).unwrap();
        git(repo, &["add", name]);
        git(repo, &["commit", "-q", "-m", message]);
        git(repo, &["rev-parse", "HEAD"])
    }

    fn task(description: &str) -> swarm_domain::Task {
        let mut task = swarm_persistence::TaskStore::in_memory()
            .unwrap()
            .create_task("Make the petals bloom", "/work")
            .unwrap();
        task.description = description.to_owned();
        task
    }

    fn present(sha: &str, path: &str) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Present,
            subject: format!("feat: {path}"),
            changed_paths: vec![path.to_owned()],
            found_in: None,
        }
    }

    fn report(workspace: &Path, commits: Vec<TaskCommit>) -> TaskCommitReport {
        TaskCommitReport {
            task_id: TaskId::new(),
            workspace: workspace.display().to_string(),
            repository_state: CommitRepositoryState::Read,
            reported_at: 1,
            commits,
        }
    }

    #[tokio::test]
    async fn the_input_carries_the_brief_the_commits_and_their_changes_oldest_first() {
        let repo = repository();
        let first = commit_file(repo.path(), "stem.rs", "fn stem() {}\n", "feat: stem");
        let second = commit_file(repo.path(), "petal.rs", "fn petal() {}\n", "feat: petal");
        // Recorded newest first, abbreviated: git puts them in order.
        let report = report(
            repo.path(),
            vec![
                present(&second[..10], "petal.rs"),
                present(&first[..10], "stem.rs"),
            ],
        );
        let amendment = TaskAmendment {
            id: "a".to_owned(),
            author_worker_id: WorkerId::new(),
            author_name: "Queen".to_owned(),
            body: "Petals are blue, not red.".to_owned(),
            created_at: 1,
        };

        let input = build_check_input(
            &task("Petals should bloom in spring."),
            &[amendment],
            &report,
        )
        .await;

        assert!(input.diff_included);
        assert_eq!(input.head, second);
        assert_eq!(input.commits[0].full.as_deref(), Some(first.as_str()));
        let prompt = &input.prompt;
        assert!(prompt.contains("Petals should bloom in spring."));
        assert!(prompt.contains("Petals are blue, not red."));
        assert!(prompt.contains("+fn stem() {}"));
        assert!(prompt.contains("+fn petal() {}"));
        assert!(
            prompt.find("+fn stem() {}") < prompt.find("+fn petal() {}"),
            "oldest change first"
        );
        assert!(prompt.contains("Read only."));
    }

    #[tokio::test]
    async fn changes_over_the_budget_are_left_out_whole_and_the_checker_told_how_to_read_them() {
        let repo = repository();
        let big = "x".repeat(PATCH_BUDGET_BYTES + 1024);
        let sha = commit_file(repo.path(), "big.txt", &big, "chore: big");
        let report = report(repo.path(), vec![present(&sha, "big.txt")]);

        let input = build_check_input(&task("Add the big file."), &[], &report).await;

        assert!(!input.diff_included);
        assert!(!input.prompt.contains("xxxxxxxxxx"), "no part of the diff");
        assert!(input.prompt.contains("they are too large"));
        assert!(
            input.prompt.contains("big.txt"),
            "the paths are still listed"
        );
        assert_eq!(input.head, sha);
    }

    #[tokio::test]
    async fn a_merge_is_left_out_when_its_commits_are_recorded_and_used_when_alone() {
        let repo = repository();
        commit_file(repo.path(), "base.rs", "base\n", "base");
        git(repo.path(), &["checkout", "-q", "-b", "work"]);
        let work = commit_file(repo.path(), "leaf.rs", "fn leaf() {}\n", "feat: leaf");
        git(repo.path(), &["checkout", "-q", "main"]);
        commit_file(repo.path(), "other.rs", "other\n", "other");
        git(
            repo.path(),
            &["merge", "-q", "--no-ff", "-m", "merge work", "work"],
        );
        let merge = git(repo.path(), &["rev-parse", "HEAD"]);

        let both = report(
            repo.path(),
            vec![present(&work, "leaf.rs"), present(&merge, "leaf.rs")],
        );
        let input = build_check_input(&task("Add a leaf."), &[], &both).await;
        assert_eq!(
            input.prompt.matches("+fn leaf() {}").count(),
            1,
            "not twice"
        );

        let alone = report(repo.path(), vec![present(&merge, "leaf.rs")]);
        let input = build_check_input(&task("Add a leaf."), &[], &alone).await;
        assert!(
            input.prompt.contains("+fn leaf() {}"),
            "the merge carries it"
        );
    }

    #[tokio::test]
    async fn commits_git_cannot_read_are_listed_and_say_so() {
        let not_a_repo = tempfile::tempdir().unwrap();
        let mut gone = present("deadbeef", "src/lib.rs");
        gone.verdict = CommitVerdict::Missing;
        let report = report(
            not_a_repo.path(),
            vec![present("cafef00d", "src/a.rs"), gone],
        );

        let input = build_check_input(&task("Fix it."), &[], &report).await;

        assert!(input.prompt.contains("cafef00d"));
        assert!(input.prompt.contains("deadbeef"));
        assert_eq!(
            input
                .prompt
                .matches("git could not read this commit")
                .count(),
            2
        );
        assert!(
            input
                .prompt
                .contains("None of the changes could be read here")
        );
        assert_eq!(
            input.head, "deadbeef",
            "the last recorded when git read none"
        );
    }

    #[tokio::test]
    async fn a_read_past_its_cap_stops_and_says_too_large() {
        let repo = repository();
        let sha = commit_file(repo.path(), "a.txt", &"y".repeat(10_000), "chore: a");
        assert_eq!(
            bounded_git(
                &repo.path().display().to_string(),
                &["show", &sha],
                100,
                later()
            )
            .await,
            GitRead::TooLarge
        );
        assert_eq!(
            bounded_git(
                &repo.path().display().to_string(),
                &["show", "nope"],
                100,
                later()
            )
            .await,
            GitRead::Failed
        );
    }

    #[tokio::test]
    async fn ordering_falls_back_to_the_recorded_order_for_unreadable_commits() {
        let not_a_repo = tempfile::tempdir().unwrap();
        let report = report(
            not_a_repo.path(),
            vec![present("aaaaaaaa", "a.rs"), present("bbbbbbbb", "b.rs")],
        );
        let ordered = ordered_commits(&report).await;
        let recorded: Vec<&str> = ordered
            .iter()
            .map(|commit| commit.recorded.as_str())
            .collect();
        assert_eq!(recorded, vec!["aaaaaaaa", "bbbbbbbb"]);
        assert_eq!(head_of(&ordered), "bbbbbbbb");
    }

    /// Commit times are whole seconds. Six commits made in one second and
    /// recorded newest first still read oldest first, because each one's
    /// parent goes before it. Six, so a SHA order cannot pass by chance.
    #[tokio::test]
    async fn commits_made_in_the_same_second_follow_their_parents() {
        let repo = repository();
        let at_one_second = |name: &str| {
            std::fs::write(repo.path().join(name), name).unwrap();
            git(repo.path(), &["add", name]);
            let output = Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                ])
                .args(["commit", "-q", "-m", name])
                .env("GIT_AUTHOR_DATE", "1767225600 +0000")
                .env("GIT_COMMITTER_DATE", "1767225600 +0000")
                .output()
                .unwrap();
            assert!(output.status.success(), "commit {name} failed");
            git(repo.path(), &["rev-parse", "HEAD"])
        };
        let made: Vec<String> = ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs"]
            .into_iter()
            .map(at_one_second)
            .collect();
        let recorded = made.iter().rev().map(|sha| present(sha, "x.rs")).collect();

        let commits = ordered_commits(&report(repo.path(), recorded)).await;

        let order: Vec<&str> = commits
            .iter()
            .map(|commit| commit.full.as_deref().unwrap())
            .collect();
        assert_eq!(order, made.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(head_of(&commits), made[5]);
    }

    /// Out of time, nothing more is read. Commits keep their recorded order
    /// unread, and changes already read are not passed off as the whole.
    #[tokio::test]
    async fn reads_stop_at_the_deadline_for_the_whole_call() {
        let repo = repository();
        let first = commit_file(repo.path(), "a.rs", "a", "feat: a");
        let second = commit_file(repo.path(), "b.rs", "b", "feat: b");
        let report = report(
            repo.path(),
            vec![present(&second, "b.rs"), present(&first, "a.rs")],
        );
        let spent = Instant::now();

        let unread = ordered_commits_until(&report, spent).await;
        assert!(unread.iter().all(|commit| commit.full.is_none()));
        assert_eq!(unread[0].recorded, second);

        let read = ordered_commits_until(&report, later()).await;
        assert!(read.iter().all(|commit| commit.full.is_some()));
        assert_eq!(read_patches(&read, spent).await, None);
        assert_eq!(
            read_patches(&read, later())
                .await
                .map(|patches| patches.len()),
            Some(2)
        );
    }

    #[test]
    fn a_task_with_no_description_says_so_rather_than_leaving_a_blank() {
        let commit = CheckedCommit {
            recorded: "aaaaaaaa".to_owned(),
            full: None,
            subject: "feat: a".to_owned(),
            paths: vec![],
            workspace: "/work".to_owned(),
            committed_at: 0,
            parents: vec![],
        };
        let prompt = checker_prompt(&task(""), &[], &[commit], Some(&[]));
        assert!(prompt.contains("no description beyond its title"));
        assert!(prompt.contains("Title: Make the petals bloom"));
    }
}
