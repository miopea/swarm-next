//! Deriving what this Hive can do, for publication to its Apiary.
//!
//! Lives here rather than behind the persistence boundary because deriving it
//! means reading git and the running build — neither of which is database
//! state. Persistence owns identity, the monotonic revision and the signature;
//! this owns the observation.

use std::collections::HashMap;
use std::process::Command;

use swarm_domain::{HiveCapabilityWorker, MAX_CAPABILITY_WORKERS, valid_repository_remote};
use swarm_persistence::TaskStore;

/// The repository a workspace belongs to, as a remote rather than a location.
///
/// ⚠️ RETURNS `None` RATHER THAN FALLING BACK TO THE PATH, and that is the whole
/// point of the function. The tempting fallback — "no remote, so report where it
/// lives" — produces a string that identifies nothing on another machine and
/// hands every member a map of one operator's disk. `valid_repository_remote`
/// refuses it at the domain boundary too, so a fallback here would simply make
/// the report invalid rather than useful.
///
/// A workspace with no remote is ORDINARY, not an error. Scout works across a
/// parent directory and Queen's workspace holds no checkout at all.
fn repository_remote(workspace: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    valid_repository_remote(&remote).then_some(remote)
}

/// This Hive's current capability: every worker it holds and the repository each
/// one works in.
///
/// Returns the workers and whether the list was truncated.
///
/// ⚠️ SLEEPING WORKERS ARE INCLUDED, which is the point rather than an
/// oversight. The operator asked for the repo list "including sleeping workers"
/// because the question Keeper is answering is what this Hive COULD do. A
/// sleeping worker can be woken; an absent one cannot.
pub(crate) fn derive_hive_capability(store: &TaskStore) -> (Vec<HiveCapabilityWorker>, bool) {
    let Ok(profiles) = store.list_worker_profiles() else {
        return (Vec::new(), false);
    };
    // Several workers commonly share one workspace, and each git call is a
    // process. Resolve each distinct workspace once.
    let mut remotes: HashMap<String, Option<String>> = HashMap::new();
    let mut workers = Vec::new();
    let mut truncated = false;
    for profile in profiles {
        // A temporary worker is "spawned beside another to try a second
        // provider, and not yet adopted into the Hive". It is not a durable
        // capability and would appear and vanish from the fleet picture.
        if profile.ephemeral {
            continue;
        }
        if workers.len() >= MAX_CAPABILITY_WORKERS {
            truncated = true;
            break;
        }
        let repository = remotes
            .entry(profile.workspace.clone())
            .or_insert_with(|| repository_remote(&profile.workspace))
            .clone();
        workers.push(HiveCapabilityWorker {
            name: profile.name,
            provider: profile.provider,
            repository,
            awake: profile.active_session_id.is_some(),
        });
    }
    (workers, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(directory: &std::path::Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {arguments:?} failed");
    }

    #[test]
    fn a_checkout_reports_its_remote_and_never_its_location() {
        let directory = TempDir::new().unwrap();
        let path = directory.path();
        git(path, &["init", "--quiet"]);
        git(
            path,
            &["remote", "add", "origin", "git@github.com:rcg/platform.git"],
        );
        assert_eq!(
            repository_remote(path.to_str().unwrap()).as_deref(),
            Some("git@github.com:rcg/platform.git")
        );
    }

    /// ⚠️ THE FALLBACK THAT MUST NOT EXIST. A checkout with no remote, and a
    /// directory that is not a checkout at all, both report nothing — rather
    /// than reporting where they are.
    #[test]
    fn a_workspace_with_no_remote_reports_nothing_rather_than_a_path() {
        let directory = TempDir::new().unwrap();
        let path = directory.path();
        assert_eq!(repository_remote(path.to_str().unwrap()), None);
        git(path, &["init", "--quiet"]);
        assert_eq!(
            repository_remote(path.to_str().unwrap()),
            None,
            "an initialised repository with no origin still has no remote to publish"
        );
    }

    /// A remote that is itself a local path is refused, because it identifies
    /// nothing on another machine. git will happily produce one.
    #[test]
    fn a_local_path_remote_is_refused_even_though_git_reports_it() {
        let origin = TempDir::new().unwrap();
        git(origin.path(), &["init", "--quiet", "--bare"]);
        let directory = TempDir::new().unwrap();
        let path = directory.path();
        git(path, &["init", "--quiet"]);
        git(
            path,
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        assert_eq!(
            repository_remote(path.to_str().unwrap()),
            None,
            "a filesystem path is not a publishable remote, however git reports it"
        );
    }
}
