//! What a Hive can do, published to its Apiary.
//!
//! Keeper needs to know which Hives could take which work. That is a question
//! about CAPABILITY — which repositories this Hive holds and which workers it
//! has — and deliberately not about liveness. A sleeping worker still counts,
//! because the Hive can still wake it; routing decides nothing from `awake`.

use serde::{Deserialize, Serialize};

use crate::apiary_directory::valid_revision;
use crate::{ApiaryId, ProviderKind, PublicHiveIdentity};

/// The report format, so a Keeper can refuse one it cannot read.
pub const HIVE_CAPABILITY_SCHEMA_VERSION: u16 = 1;

/// Bounded like every other federation read here. A Hive with more workers than
/// this reports the first `MAX` and says plainly that it truncated, rather than
/// silently shortening the fleet's picture of itself.
pub const MAX_CAPABILITY_WORKERS: usize = 256;

const MAX_NAME_BYTES: usize = 200;
const MAX_REPOSITORY_BYTES: usize = 512;
const MAX_VERSION_BYTES: usize = 128;

/// One worker this Hive could run, and the repository it works in.
///
/// ⚠️ THERE IS NO PATH FIELD, AND ITS ABSENCE IS THE ENFORCEMENT. The obvious
/// identifier for a repository here is the worker's workspace, because that is
/// what `swarm_create_task` already routes on — and it is the wrong one twice
/// over. An absolute path cannot identify the same repository on another
/// machine, and publishing it hands every member a map of one operator's disk
/// for no gain. A field that does not exist cannot be populated in a hurry by
/// someone who needs a routing key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveCapabilityWorker {
    pub name: String,
    pub provider: ProviderKind,
    /// The repository's canonical remote, or `None` when this worker's
    /// workspace is not a git checkout at all — which is ordinary. Scout works
    /// across a parent directory, and Queen's workspace holds no remote.
    pub repository: Option<String>,
    /// Whether a session is open right now.
    ///
    /// ⚠️ REPORTED, NEVER ROUTED ON. Availability was settled separately: a
    /// sleeping Hive simply never reserves, and expiry recovery reclaims
    /// anything it reserved and then slept on. Liveness is the fastest-rotting
    /// fact in the system, so anything that routes on this field is routing on
    /// where a Hive was, not where it is.
    pub awake: bool,
}

impl HiveCapabilityWorker {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.len() <= MAX_NAME_BYTES
            && self
                .repository
                .as_deref()
                .is_none_or(valid_repository_remote)
    }
}

/// A repository remote, and specifically NOT a filesystem path.
///
/// ⚠️ THIS IS THE GUARD THE TASK'S ACCEPTANCE RESTS ON: "no absolute filesystem
/// path appears anywhere in it". Refusing at the type boundary is the only place
/// that holds, because the caller derives this by shelling out to git and a
/// misconfigured checkout can hand back anything.
///
/// `file://` is refused for the same reason a bare path is: it is a local disk
/// location wearing a URL, it identifies nothing on another machine, and it is
/// exactly what a well-meaning fallback would produce.
#[must_use]
pub fn valid_repository_remote(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_REPOSITORY_BYTES {
        return false;
    }
    if trimmed != value || trimmed.contains(char::is_whitespace) {
        return false;
    }
    // A bare absolute path, a home-relative path, or a Windows drive letter.
    if trimmed.starts_with('/') || trimmed.starts_with('~') {
        return false;
    }
    let windows_drive = {
        let mut characters = trimmed.chars();
        matches!(characters.next(), Some(letter) if letter.is_ascii_alphabetic())
            && matches!(characters.next(), Some(':'))
            && matches!(characters.next(), Some('\\' | '/'))
    };
    if windows_drive {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("file:") {
        return false;
    }
    // What remains must look like a remote: a scheme, or git's scp-like form.
    lowered.starts_with("https://")
        || lowered.starts_with("http://")
        || lowered.starts_with("ssh://")
        || lowered.starts_with("git://")
        || scp_like(trimmed)
}

/// git's `user@host:path` form, which has no scheme and is not a path.
fn scp_like(value: &str) -> bool {
    let Some((host, path)) = value.split_once(':') else {
        return false;
    };
    if path.is_empty() || host.is_empty() {
        return false;
    }
    let host = host.rsplit('@').next().unwrap_or(host);
    !host.is_empty() && host.contains('.') && !host.contains('/')
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveCapabilityPayload {
    pub schema_version: u16,
    pub apiary_id: ApiaryId,
    pub identity: PublicHiveIdentity,
    pub revision: u64,
    /// When the reporting Hive derived this, by its own clock.
    ///
    /// ⚠️ THIS IS WHAT MAKES A STALE REPORT DISTINGUISHABLE FROM A MISSING ONE,
    /// which the acceptance asks for. A reader that only knows "there is a
    /// report" cannot tell a Hive that went quiet last week from one that
    /// answered a minute ago, and those demand opposite reactions.
    pub observed_at: i64,
    /// The running build, as the Hive reports itself.
    pub swarm_version: String,
    /// The database schema this Hive is actually on. Carried separately from
    /// the build because a schema mismatch is a correctness problem rather than
    /// a freshness one, and is surfaced differently.
    pub database_schema_version: i64,
    pub workers: Vec<HiveCapabilityWorker>,
    /// True when this Hive holds more workers than `MAX_CAPABILITY_WORKERS`.
    pub workers_truncated: bool,
}

impl HiveCapabilityPayload {
    /// Structural scope validation only. Persistence must still verify the
    /// pinned signature, the active credential, and a monotonic revision inside
    /// its own transaction — exactly as the profile update does.
    #[must_use]
    pub fn matches_member(&self, apiary_id: ApiaryId, member: PublicHiveIdentity) -> bool {
        self.schema_version == HIVE_CAPABILITY_SCHEMA_VERSION
            && self.apiary_id == apiary_id
            && self.identity == member
            && valid_revision(self.revision)
            && self.is_valid()
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.observed_at >= 0
            && !self.swarm_version.trim().is_empty()
            && self.swarm_version.len() <= MAX_VERSION_BYTES
            && self.database_schema_version >= 0
            && self.workers.len() <= MAX_CAPABILITY_WORKERS
            && self.workers.iter().all(HiveCapabilityWorker::is_valid)
    }

    /// Every distinct repository this Hive can work in, for routing.
    #[must_use]
    pub fn repositories(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = self
            .workers
            .iter()
            .filter_map(|worker| worker.repository.as_deref())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HiveCapabilityUpdate {
    pub payload: HiveCapabilityPayload,
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ THE ACCEPTANCE CRITERION, ASSERTED DIRECTLY: "no absolute filesystem
    /// path appears anywhere in it". The caller derives these by shelling out to
    /// git, so the type boundary is the only place that can hold the line.
    #[test]
    fn a_filesystem_path_is_never_a_repository_however_it_is_dressed() {
        for rejected in [
            // The obvious one, and the one a routing key would reach for first:
            // swarm_create_task already routes on exactly this string.
            "/home/bschleifer/projects/personal/swarm-next",
            "/home/bschleifer/projects",
            "~/projects/swarm-next",
            // A local path wearing a URL. This is what a well-meaning fallback
            // produces, and it identifies nothing on another machine.
            "file:///home/bschleifer/projects/swarm-next",
            "FILE:///home/bschleifer/projects",
            "C:\\Users\\op\\swarm-next",
            "c:/Users/op/swarm-next",
            // Not a remote at all.
            "",
            "   ",
            "swarm-next",
            "git@github.com:miopea/swarm-next.git extra",
        ] {
            assert!(
                !valid_repository_remote(rejected),
                "{rejected:?} must not pass as a repository remote"
            );
        }
    }

    #[test]
    fn real_remotes_are_accepted_in_every_form_git_actually_emits() {
        for accepted in [
            "https://github.com/miopea/swarm-next.git",
            "https://github.com/miopea/swarm-next",
            "http://internal.example.com/git/thing.git",
            "ssh://git@github.com/miopea/swarm-next.git",
            "git://example.com/thing.git",
            // scp-like, which has no scheme and is the default for SSH remotes.
            "git@github.com:miopea/swarm-next.git",
            "org-1234@github.com:miopea/swarm-next.git",
        ] {
            assert!(
                valid_repository_remote(accepted),
                "{accepted:?} is a remote git emits and must be accepted"
            );
        }
    }

    fn worker(name: &str, repository: Option<&str>) -> HiveCapabilityWorker {
        HiveCapabilityWorker {
            name: name.to_owned(),
            provider: ProviderKind::ClaudeCode,
            repository: repository.map(str::to_owned),
            awake: false,
        }
    }

    /// A worker whose workspace is not a checkout is ORDINARY, not an error.
    /// Scout works across a parent directory and Queen's workspace holds no
    /// remote; rejecting them would drop two real workers from the fleet's
    /// picture of itself.
    #[test]
    fn a_worker_without_a_repository_is_still_reportable() {
        assert!(worker("Scout", None).is_valid());
        assert!(!worker("  ", None).is_valid());
        assert!(!worker("Platform", Some("/home/op/projects/rcg-platform")).is_valid());
    }

    fn payload(workers: Vec<HiveCapabilityWorker>) -> HiveCapabilityPayload {
        HiveCapabilityPayload {
            schema_version: HIVE_CAPABILITY_SCHEMA_VERSION,
            apiary_id: ApiaryId::new(),
            identity: PublicHiveIdentity {
                node_id: crate::FederationNodeId::new(),
                hive_id: crate::HiveId::new(),
                operator_id: crate::OperatorId::new(),
            },
            revision: 1,
            observed_at: 1_790_000_000,
            swarm_version: "1.12.0-dev-f8c39fc2ed12".to_owned(),
            database_schema_version: 184,
            workers,
            workers_truncated: false,
        }
    }

    #[test]
    fn a_payload_is_only_valid_when_every_worker_is() {
        assert!(
            payload(vec![worker(
                "Platform",
                Some("git@github.com:rcg/platform.git")
            )])
            .is_valid()
        );
        assert!(!payload(vec![worker("Platform", Some("/home/op/platform"))]).is_valid());

        let mut missing_version = payload(Vec::new());
        missing_version.swarm_version = " ".to_owned();
        assert!(
            !missing_version.is_valid(),
            "a Hive must say what it is running"
        );

        let mut over_bound = payload(
            (0..=MAX_CAPABILITY_WORKERS)
                .map(|index| worker(&format!("Worker {index}"), None))
                .collect(),
        );
        assert!(!over_bound.is_valid(), "the worker list is bounded");
        over_bound.workers.truncate(MAX_CAPABILITY_WORKERS);
        over_bound.workers_truncated = true;
        assert!(
            over_bound.is_valid(),
            "truncating and saying so is the way through"
        );
    }

    /// Routing asks "which Hive can work in this repository", so several workers
    /// in one repository must not read as several capabilities.
    #[test]
    fn repositories_are_deduplicated_and_pathless_workers_contribute_none() {
        let report = payload(vec![
            worker("Platform", Some("git@github.com:rcg/platform.git")),
            worker("Platform relief", Some("git@github.com:rcg/platform.git")),
            worker("Admin", Some("https://github.com/rcg/admin.git")),
            worker("Scout", None),
        ]);
        assert_eq!(
            report.repositories(),
            vec![
                "git@github.com:rcg/platform.git",
                "https://github.com/rcg/admin.git"
            ]
        );
    }
}
