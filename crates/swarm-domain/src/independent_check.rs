//! A worker's independent check of its own task's commits (ADR 0171).
//!
//! The worker runs the check. Swarm builds what the checker is given, so the
//! worker cannot steer it, and records what came back against the exact set of
//! commits it saw. Queen is shown the result. Review is never refused for want
//! of one: the operator chose "show it, don't refuse".

use std::{collections::BTreeSet, fmt, fmt::Write as _, str::FromStr};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    CommitSettlement, ParseTaskStateError, TaskCommitReport, commit_settlement, commits_digest,
};

/// How long "what ran" may be. It names commands, not their output.
pub const MAX_INDEPENDENT_CHECK_RAN_CHARS: usize = 1_000;
/// How long the findings may be. Queen reads them beside the task, so they are
/// a summary with file and line, not a transcript.
pub const MAX_INDEPENDENT_CHECK_FINDINGS_CHARS: usize = 4_000;

/// What the checker concluded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndependentCheckVerdict {
    /// The change does what the task asks, and nothing the checker ran failed.
    Pass,
    /// Some of it holds and some does not, or the checker could not run all of
    /// it.
    Partial,
    /// The change does not do what the task asks, or a check failed.
    Fail,
}

impl fmt::Display for IndependentCheckVerdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pass => "pass",
            Self::Partial => "partial",
            Self::Fail => "fail",
        })
    }
}

impl FromStr for IndependentCheckVerdict {
    type Err = ParseTaskStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pass" => Ok(Self::Pass),
            "partial" => Ok(Self::Partial),
            "fail" => Ok(Self::Fail),
            _ => Err(ParseTaskStateError),
        }
    }
}

/// One recorded check, as stored.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IndependentCheck {
    /// [`commits_digest`] of the task's recorded commits when the checker's
    /// input was built. This, not the head, is what decides staleness: a task's
    /// work can have more than one tip.
    pub commits: String,
    /// The newest of those commits. Shown to people; decides nothing.
    pub head: String,
    pub verdict: IndependentCheckVerdict,
    /// What the checker ran, in its own words.
    pub ran: String,
    /// What it found, with file and line. Empty only on a pass.
    pub findings: String,
    pub recorded_at: i64,
}

/// What Queen and the task card are shown about a task's check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum IndependentCheckState {
    /// The task built code and nobody has recorded a check of it.
    NotChecked,
    /// A check covers every commit the task has recorded.
    Checked {
        head: String,
        verdict: IndependentCheckVerdict,
        ran: String,
        findings: String,
        recorded_at: i64,
    },
    /// The task recorded commits after its last check. The old verdict is
    /// kept, because "it failed, and then more work landed" is worth seeing,
    /// but it no longer speaks for the work as it stands.
    Stale {
        head: String,
        verdict: IndependentCheckVerdict,
        recorded_at: i64,
        /// The commits the check never saw.
        since: Vec<String>,
    },
}

/// The short token a worker carries from building a check's input to
/// recording its result.
///
/// It binds the result to the commits the checker was actually given. A worker
/// that commits again in between gets a different basis, and the record is
/// refused rather than pinned to work the checker never saw.
#[must_use]
pub fn independent_check_basis(shas: &[String]) -> String {
    let digest = Sha256::digest(commits_digest(shas).as_bytes());
    digest
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The check state of a task, or `None` when there is nothing to show.
///
/// ⚠️ NOTHING HERE REFUSES ANYTHING. This only says what to show. A task with
/// no check still enters Review, by the operator's choice.
///
/// `None` covers a task that has recorded no commits, built nothing, or
/// changed documentation only. Those carry no code for a checker to read, and
/// a "not checked" on every paperwork task would teach Queen to skip the line.
/// A recorded check is always shown, whatever the commits turned out to be.
#[must_use]
pub fn independent_check_state(
    report: Option<&TaskCommitReport>,
    latest: Option<&IndependentCheck>,
) -> Option<IndependentCheckState> {
    let shas: Vec<String> = report
        .map(|report| {
            report
                .commits
                .iter()
                .map(|commit| commit.sha.clone())
                .collect()
        })
        .unwrap_or_default();
    if let Some(check) = latest {
        if commits_digest(&shas) == check.commits {
            return Some(IndependentCheckState::Checked {
                head: check.head.clone(),
                verdict: check.verdict,
                ran: check.ran.clone(),
                findings: check.findings.clone(),
                recorded_at: check.recorded_at,
            });
        }
        let seen: BTreeSet<&str> = check.commits.split(',').collect();
        let mut since: Vec<String> = shas
            .into_iter()
            .filter(|sha| !seen.contains(sha.as_str()))
            .collect();
        since.sort_unstable();
        since.dedup();
        return Some(IndependentCheckState::Stale {
            head: check.head.clone(),
            verdict: check.verdict,
            recorded_at: check.recorded_at,
            since,
        });
    }
    match commit_settlement(report) {
        CommitSettlement::BuiltCode => Some(IndependentCheckState::NotChecked),
        // Commits nobody could settle are still commits: a merge with no
        // listed paths may carry the whole change.
        CommitSettlement::Unestablished if !shas.is_empty() => {
            Some(IndependentCheckState::NotChecked)
        }
        _ => None,
    }
}

/// Why a check's result cannot be recorded as given, in the worker's words.
///
/// # Errors
/// Returns the reason when "what ran" is empty or too long, the findings are
/// too long, or a verdict other than a pass arrives with no findings.
pub fn validate_independent_check(
    verdict: IndependentCheckVerdict,
    ran: &str,
    findings: &str,
) -> Result<(), String> {
    if ran.trim().is_empty() {
        return Err("Say what the checker ran, even if it only read the diff.".to_owned());
    }
    if ran.chars().count() > MAX_INDEPENDENT_CHECK_RAN_CHARS {
        return Err(format!(
            "\"ran\" is over {MAX_INDEPENDENT_CHECK_RAN_CHARS} characters. Name the commands; leave out their output."
        ));
    }
    if findings.chars().count() > MAX_INDEPENDENT_CHECK_FINDINGS_CHARS {
        return Err(format!(
            "The findings are over {MAX_INDEPENDENT_CHECK_FINDINGS_CHARS} characters. Give each one with its file and line, and leave out the reasoning."
        ));
    }
    if verdict != IndependentCheckVerdict::Pass && findings.trim().is_empty() {
        return Err(format!(
            "A {verdict} needs its findings, so Queen can see what was wrong."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommitRepositoryState, CommitVerdict, TaskCommit, TaskId};

    fn commit(sha: &str, paths: &[&str]) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Present,
            subject: format!("subject {sha}"),
            changed_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            found_in: None,
        }
    }

    fn report(commits: Vec<TaskCommit>) -> TaskCommitReport {
        TaskCommitReport {
            task_id: TaskId::new(),
            workspace: "/work".to_owned(),
            repository_state: CommitRepositoryState::Read,
            reported_at: 10,
            commits,
        }
    }

    fn check_of(shas: &[&str], verdict: IndependentCheckVerdict) -> IndependentCheck {
        let shas: Vec<String> = shas.iter().map(|sha| (*sha).to_owned()).collect();
        IndependentCheck {
            commits: commits_digest(&shas),
            head: shas.last().cloned().unwrap_or_default(),
            verdict,
            ran: "cargo test".to_owned(),
            findings: String::new(),
            recorded_at: 20,
        }
    }

    #[test]
    fn nothing_is_shown_without_code_to_check() {
        assert_eq!(independent_check_state(None, None), None);
        assert_eq!(independent_check_state(Some(&report(vec![])), None), None);
        let docs = report(vec![commit("aaa", &["docs/adr.md", "README.md"])]);
        assert_eq!(independent_check_state(Some(&docs), None), None);
    }

    #[test]
    fn code_with_no_check_reads_not_checked() {
        let code = report(vec![commit("aaa", &["src/lib.rs"])]);
        assert_eq!(
            independent_check_state(Some(&code), None),
            Some(IndependentCheckState::NotChecked)
        );
    }

    #[test]
    fn a_merge_with_no_listed_paths_still_reads_not_checked() {
        let merge = report(vec![commit("aaa", &[])]);
        assert_eq!(
            independent_check_state(Some(&merge), None),
            Some(IndependentCheckState::NotChecked)
        );
    }

    #[test]
    fn a_check_covering_every_commit_reads_checked() {
        let code = report(vec![
            commit("bbb", &["src/a.rs"]),
            commit("aaa", &["src/b.rs"]),
        ]);
        let check = check_of(&["aaa", "bbb"], IndependentCheckVerdict::Pass);
        assert_eq!(
            independent_check_state(Some(&code), Some(&check)),
            Some(IndependentCheckState::Checked {
                head: "bbb".to_owned(),
                verdict: IndependentCheckVerdict::Pass,
                ran: "cargo test".to_owned(),
                findings: String::new(),
                recorded_at: 20,
            })
        );
    }

    #[test]
    fn a_commit_recorded_after_the_check_makes_it_stale_and_names_it() {
        let code = report(vec![
            commit("aaa", &["src/a.rs"]),
            commit("ccc", &["src/c.rs"]),
        ]);
        let check = check_of(&["aaa"], IndependentCheckVerdict::Fail);
        assert_eq!(
            independent_check_state(Some(&code), Some(&check)),
            Some(IndependentCheckState::Stale {
                head: "aaa".to_owned(),
                verdict: IndependentCheckVerdict::Fail,
                recorded_at: 20,
                since: vec!["ccc".to_owned()],
            })
        );
    }

    #[test]
    fn a_recorded_check_is_shown_even_on_documentation() {
        let docs = report(vec![commit("aaa", &["docs/a.md"])]);
        let check = check_of(&["aaa"], IndependentCheckVerdict::Pass);
        assert!(matches!(
            independent_check_state(Some(&docs), Some(&check)),
            Some(IndependentCheckState::Checked { .. })
        ));
    }

    #[test]
    fn the_basis_ignores_order_and_repeats_and_moves_with_a_new_commit() {
        let one = independent_check_basis(&["b".to_owned(), "a".to_owned()]);
        let same = independent_check_basis(&["a".to_owned(), "b".to_owned(), "a".to_owned()]);
        let more = independent_check_basis(&["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        assert_eq!(one, same);
        assert_ne!(one, more);
        assert_eq!(one.len(), 16);
        assert!(one.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn verdicts_round_trip_through_their_stored_words() {
        for verdict in [
            IndependentCheckVerdict::Pass,
            IndependentCheckVerdict::Partial,
            IndependentCheckVerdict::Fail,
        ] {
            assert_eq!(
                verdict.to_string().parse::<IndependentCheckVerdict>(),
                Ok(verdict)
            );
        }
        assert!("passed".parse::<IndependentCheckVerdict>().is_err());
    }

    #[test]
    fn a_result_needs_what_ran_and_findings_unless_it_passed() {
        use IndependentCheckVerdict::{Fail, Partial, Pass};
        assert!(validate_independent_check(Pass, "cargo test", "").is_ok());
        assert!(validate_independent_check(Pass, "  ", "").is_err());
        assert!(validate_independent_check(Fail, "cargo test", " ").is_err());
        assert!(validate_independent_check(Partial, "read the diff", "a.rs:3 unused").is_ok());
        let long = "x".repeat(MAX_INDEPENDENT_CHECK_RAN_CHARS + 1);
        assert!(validate_independent_check(Pass, &long, "").is_err());
        let long = "x".repeat(MAX_INDEPENDENT_CHECK_FINDINGS_CHARS + 1);
        assert!(validate_independent_check(Fail, "cargo test", &long).is_err());
    }
}
