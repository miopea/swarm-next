//! One git index per worker in a checkout, by giving the second worker its own
//! worktree.
//!
//! WHY THIS EXISTS. Two concurrent sessions wrote the same rcg-platform
//! checkout. `.git/index` belongs to the WORKTREE, not to the process, so one
//! session's commit absorbed the other's staged deletions: a commit now on
//! origin/main carries a message describing one change and a diff doing
//! another. `git status` was clean throughout, and every check Swarm runs
//! passed. Operator decision 01a0a2b5-b7e3-7a71-88e0-84a04fa939a6 -- "Both:
//! atomic commits now, worktrees as a follow-up" -- authorised this half.
//!
//! ⚠️ ONLY THE SECOND AND LATER WORKER IN A CHECKOUT GETS ONE. That is a
//! deliberate limit, not a half-measure. Measured on this Hive: 1 workspace in
//! 29 holds more than one worker. And a worktree CANNOT sit on the same branch
//! as its checkout -- git refuses outright, `fatal: 'master' is already used by
//! worktree at ...` -- so every worktree means a worker whose commits land on a
//! branch rather than on main, and whose push becomes `git push origin
//! HEAD:main` instead of the workflow every skill, habit and CLAUDE.md rule
//! here assumes. Imposing that on 28 single-worker repositories, to prevent a
//! collision none of them can have, would be a bad trade. The worker that would
//! otherwise have SHARED an index is the one that pays for the fix.
//!
//! ⚠️ IT DEGRADES TO TODAY'S BEHAVIOUR, NOT TO A STOPPED WORKER. If git refuses
//! for any reason, the session starts in the checkout exactly as it does now and
//! the reason is logged. That leaves the hazard in place for that start, which
//! is worse than a worktree and much better than a worker that will not run --
//! an unstartable worker is a failure the operator cannot route around, and this
//! is a fix for a rare collision, not a safety interlock.

use std::path::{Path, PathBuf};
use std::process::Command;

use swarm_domain::{WorkerId, WorkerProfile};

/// Where one worker's session should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionWorkspace {
    /// The checkout itself: what a worker gets when nothing else shares it.
    Checkout,
    /// A worktree beside it, for a worker that would otherwise share an index.
    Worktree { path: PathBuf, branch: String },
}

/// Which worker keeps the checkout, and which needs a worktree of its own.
///
/// ⚠️ THE ANSWER MUST NOT DEPEND ON WHO ASKS FIRST. An election that reads
/// "whoever is already running" would hand the checkout to a different worker
/// after every restart, and a worker whose workspace moves between starts loses
/// its uncommitted work without being told. The oldest profile in the checkout
/// holds it, by creation time and then by id, so the answer is the same
/// whatever order the roster arrives in and whatever is running at the time.
pub(crate) fn decide(
    profiles: &[WorkerProfile],
    worker_id: WorkerId,
    checkout: &Path,
) -> SessionWorkspace {
    let key = workspace_key(checkout);
    let mut sharers: Vec<&WorkerProfile> = profiles
        .iter()
        .filter(|profile| workspace_key(Path::new(&profile.workspace)) == key)
        .collect();
    // One worker in the checkout cannot collide with itself, and this is the
    // case for 28 of the 29 workspaces here. Nothing changes for them.
    if sharers.len() < 2 {
        return SessionWorkspace::Checkout;
    }
    sharers.sort_by_key(|profile| (profile.created_at, profile.id.to_string()));
    if sharers.first().is_some_and(|holder| holder.id == worker_id) {
        return SessionWorkspace::Checkout;
    }
    let name = sharers
        .iter()
        .find(|profile| profile.id == worker_id)
        .map(|profile| profile.name.as_str())
        .unwrap_or_default();
    SessionWorkspace::Worktree {
        // ⚠️ KEYED BY THE FULL WORKER ID, never by a prefix of it. Ids created
        // close together share leading characters, and two workers in ONE
        // checkout is exactly the population most likely to have been created
        // seconds apart -- a shortened id would collide precisely here, and a
        // collision means two workers sharing a worktree, which is the defect
        // this module exists to remove.
        path: checkout
            .join(".claude")
            .join("worktrees")
            .join(worker_id.to_string()),
        branch: format!("swarm/{}", slug(name)),
    }
}

/// Creates the worktree if it is not already there, and reports what went
/// wrong rather than panicking, because the caller's fallback is to carry on.
pub(crate) fn ensure(checkout: &Path, path: &Path, branch: &str) -> Result<(), String> {
    if already_a_worktree(path) {
        return Ok(());
    }
    // A worktree whose directory was deleted by hand leaves a record behind,
    // and git then refuses to create it again at the same path. Pruning first
    // makes provisioning idempotent across that, which is the state a machine
    // ends up in after someone tidies up a disk.
    let _ = git(checkout, &["worktree", "prune"]);
    keep_worktrees_out_of_status(checkout);
    let existing = git(
        checkout,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok();
    let display = path.to_string_lossy().into_owned();
    let arguments: Vec<&str> = if existing {
        vec!["worktree", "add", &display, branch]
    } else {
        vec!["worktree", "add", "-b", branch, &display, "HEAD"]
    };
    git(checkout, &arguments).map(|_| ())
}

/// Whether this path is already a working tree git will talk to.
///
/// ⚠️ ASKS GIT, rather than looking for a `.git` entry. A worktree's `.git` is a
/// FILE pointing into the parent's administrative directory, not a directory,
/// so a test for `path/.git/` is false for every healthy worktree -- it would
/// reprovision on every start, and `git worktree add` onto an occupied path
/// fails, so the worker would silently fall back to the checkout forever.
fn already_a_worktree(path: &Path) -> bool {
    path.exists()
        && git(path, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|out| out == "true")
}

/// Adds `.claude/worktrees/` to the checkout's local excludes when nothing
/// already ignores it.
///
/// The operator reads `git status` to decide whether their tree is clean. A
/// provisioned worktree that shows up as untracked turns that reading into
/// noise, and worse, invites someone to `git add` a whole second checkout.
/// This writes to `info/exclude`, which is local repository metadata and not a
/// tracked file, so it changes nothing for anyone else and nothing to review.
fn keep_worktrees_out_of_status(checkout: &Path) {
    const ENTRY: &str = ".claude/worktrees/";
    if git(checkout, &["check-ignore", "-q", ENTRY]).is_ok() {
        return;
    }
    let Ok(common) = git(checkout, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let common = if Path::new(&common).is_absolute() {
        PathBuf::from(common)
    } else {
        checkout.join(common)
    };
    let exclude = common.join("info").join("exclude");
    let current = std::fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|line| line.trim() == ENTRY) {
        return;
    }
    if let Some(parent) = exclude.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let separator = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let _ = std::fs::write(&exclude, format!("{current}{separator}{ENTRY}\n"));
}

fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The path as an identity, so two spellings of one checkout are one checkout.
///
/// Trailing slashes, `..` segments and a symlinked home all produce different
/// strings for the same directory, and a comparison that misses that decides
/// two workers do NOT share a checkout when they do -- which is the one wrong
/// answer this module must not give.
fn workspace_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path.to_string_lossy().trim_end_matches('/').to_owned()))
}

/// A worker name as a branch segment: lowercase, punctuation to dashes.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.extend(character.to_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').to_owned();
    if slug.is_empty() {
        "worker".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use swarm_domain::{HiveId, ProviderKind, WorkerId, WorkerProfile, WorkerRole};

    use super::{SessionWorkspace, decide, ensure};

    fn profile(name: &str, workspace: &str, created_at: i64) -> WorkerProfile {
        WorkerProfile {
            id: WorkerId::new(),
            hive_id: HiveId::new(),
            name: name.to_owned(),
            description: String::new(),
            role: WorkerRole::Worker,
            provider: ProviderKind::ClaudeCode,
            workspace: workspace.to_owned(),
            autostart: false,
            position: 0,
            active_session_id: None,
            provider_conversation_id: None,
            has_session_history: false,
            engagement_expires_at: None,
            created_at,
            updated_at: created_at,
            ephemeral: false,
            mark: None,
        }
    }

    fn git(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A repository with one commit, which is the least a worktree can be cut
    /// from.
    fn repository(root: &Path) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["-c", "init.defaultBranch=main", "init", "--quiet"])
                .status()
                .expect("git runs")
                .success()
        );
        git(root, &["config", "user.email", "worker@example.invalid"]);
        git(root, &["config", "user.name", "Worker"]);
        std::fs::write(root.join("first.txt"), "first\n").unwrap();
        git(root, &["add", "first.txt"]);
        git(root, &["commit", "--quiet", "-m", "first"]);
    }

    /// 28 of this Hive's 29 workspaces hold exactly one worker, and none of them
    /// should notice this module exists.
    #[test]
    fn the_only_worker_in_a_checkout_keeps_it() {
        let alone = profile("Platform", "/w/platform", 1);
        let elsewhere = profile("Admin", "/w/admin", 2);
        let id = alone.id;

        assert_eq!(
            decide(&[alone, elsewhere], id, Path::new("/w/platform")),
            SessionWorkspace::Checkout
        );
    }

    #[test]
    fn the_oldest_worker_keeps_the_checkout_and_the_newer_one_moves_out() {
        let older = profile("Platform", "/w/platform", 1);
        let newer = profile("Platform · Codex", "/w/platform", 2);
        let (older_id, newer_id) = (older.id, newer.id);
        let roster = [older, newer];

        assert_eq!(
            decide(&roster, older_id, Path::new("/w/platform")),
            SessionWorkspace::Checkout
        );
        let SessionWorkspace::Worktree { path, branch } =
            decide(&roster, newer_id, Path::new("/w/platform"))
        else {
            panic!("the second worker in a checkout needs its own index");
        };
        assert_eq!(
            path,
            Path::new("/w/platform/.claude/worktrees").join(newer_id.to_string()),
            "the directory is keyed by the FULL worker id -- ids made seconds \
             apart share a prefix, and two workers in one checkout is exactly \
             that population"
        );
        assert_eq!(branch, "swarm/platform-codex");
    }

    /// ⚠️ A WORKER WHOSE WORKSPACE MOVES BETWEEN STARTS LOSES UNCOMMITTED WORK
    /// without being told. The roster arrives in whatever order persistence
    /// returns it, so the election must not read that order.
    #[test]
    fn the_answer_does_not_depend_on_the_order_the_roster_arrives_in() {
        let older = profile("Platform", "/w/platform", 1);
        let newer = profile("Codex", "/w/platform", 2);
        let newer_id = newer.id;
        let forwards = [older.clone(), newer.clone()];
        let backwards = [newer, older];

        assert_eq!(
            decide(&forwards, newer_id, Path::new("/w/platform")),
            decide(&backwards, newer_id, Path::new("/w/platform"))
        );
    }

    /// The one wrong answer this module must not give is "these two workers do
    /// not share a checkout" when they do.
    #[test]
    fn two_spellings_of_one_checkout_are_one_checkout() {
        let older = profile("Platform", "/w/platform/", 1);
        let newer = profile("Codex", "/w/platform", 2);
        let newer_id = newer.id;

        assert!(matches!(
            decide(&[older, newer], newer_id, Path::new("/w/platform")),
            SessionWorkspace::Worktree { .. }
        ));
    }

    /// ⚠️ THE OUTCOME, NOT THE MECHANISM. The previous fix in this session
    /// passed every test of its own plumbing and delivered nothing, because no
    /// test crossed the boundary where the value was dropped. So this one does
    /// not assert that git was called: it stages a file in each tree and proves
    /// neither sees the other's staged change. That is the corruption -- one
    /// session's commit absorbing another's staged deletions -- reproduced as
    /// closely as a test can and shown to be impossible.
    #[test]
    fn the_two_trees_stage_into_separate_indexes() {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("repo");
        std::fs::create_dir(&checkout).unwrap();
        repository(&checkout);
        let worktree = checkout.join(".claude").join("worktrees").join("second");

        ensure(&checkout, &worktree, "swarm/second").expect("the worktree is created");

        std::fs::write(checkout.join("from-checkout.txt"), "a\n").unwrap();
        git(&checkout, &["add", "from-checkout.txt"]);
        std::fs::write(worktree.join("from-worktree.txt"), "b\n").unwrap();
        git(&worktree, &["add", "from-worktree.txt"]);

        let staged_in_checkout = git(&checkout, &["diff", "--cached", "--name-only"]);
        let staged_in_worktree = git(&worktree, &["diff", "--cached", "--name-only"]);
        assert_eq!(staged_in_checkout, "from-checkout.txt");
        assert_eq!(
            staged_in_worktree, "from-worktree.txt",
            "a shared index is what let one session's commit carry another's \
             staged changes; these two must not see each other"
        );
    }

    /// Provisioning runs on every start, not only the first.
    #[test]
    fn provisioning_an_existing_worktree_is_not_an_error() {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("repo");
        std::fs::create_dir(&checkout).unwrap();
        repository(&checkout);
        let worktree = checkout.join(".claude").join("worktrees").join("second");

        ensure(&checkout, &worktree, "swarm/second").expect("first provisioning");
        ensure(&checkout, &worktree, "swarm/second").expect("second provisioning");

        assert!(worktree.join("first.txt").exists());
    }

    /// The operator reads `git status` to decide whether their tree is clean.
    #[test]
    fn the_worktree_does_not_show_up_as_untracked_in_the_checkout() {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("repo");
        std::fs::create_dir(&checkout).unwrap();
        repository(&checkout);
        let worktree = checkout.join(".claude").join("worktrees").join("second");

        ensure(&checkout, &worktree, "swarm/second").expect("the worktree is created");

        // ⚠️ THE EXISTENCE ASSERTION IS NOT PADDING. Without it this test passes
        // when NOTHING is provisioned -- an empty status and an absent worktree
        // look identical from here, and an ablation proved it: neutering
        // `ensure` failed every other test in this file and left this one green.
        assert!(
            worktree.join("first.txt").exists(),
            "there must be a worktree before its invisibility means anything"
        );
        assert_eq!(
            git(&checkout, &["status", "--porcelain"]),
            "",
            "a provisioned worktree that reads as untracked invites somebody to \
             git add a whole second checkout"
        );
    }

    /// Git refuses to check one branch out twice, so the worktree cannot be on
    /// the checkout's branch. This records what the worker actually gets.
    #[test]
    fn the_worktree_is_on_its_own_branch_and_the_checkout_keeps_main() {
        let home = tempfile::tempdir().unwrap();
        let checkout = home.path().join("repo");
        std::fs::create_dir(&checkout).unwrap();
        repository(&checkout);
        let worktree = checkout.join(".claude").join("worktrees").join("second");

        ensure(&checkout, &worktree, "swarm/second").expect("the worktree is created");

        assert_eq!(
            git(&checkout, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "main"
        );
        assert_eq!(
            git(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "swarm/second",
            "a named branch rather than a detached HEAD: commits on a detached \
             HEAD are reachable from nothing and a later checkout drops them"
        );
    }
}
