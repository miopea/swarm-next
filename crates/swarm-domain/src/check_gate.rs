//! The check gate: work does not count as done until CI passes on the commits
//! it recorded (ADR 0115).
//!
//! Pure rules only. Reading CI belongs to an adapter, which hands these rules
//! what it read; storing what it read and refusing a close belong to the store.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// How strictly a Hive holds work to its CI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckGateLevel {
    /// No gate: CI plays no part in closing work. Every Hive's starting point.
    #[default]
    Off,
    /// CI is read and shown in review, but work can still close.
    Warn,
    /// Work with recorded commits cannot close while their CI fails or is
    /// still running. CI that cannot be read does not hold it, and says so.
    Require,
}

impl CheckGateLevel {
    pub const ALL: [Self; 3] = [Self::Off, Self::Warn, Self::Require];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Require => "require",
        }
    }
}

impl fmt::Display for CheckGateLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CheckGateLevel {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str() == value)
            .ok_or_else(|| format!("unknown check gate level {value:?}"))
    }
}

/// One GitHub check run on a commit, as read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckRunReading {
    pub name: String,
    /// `queued`, `in_progress`, `completed`, and so on.
    pub status: String,
    /// Set once `status` is `completed`.
    pub conclusion: Option<String>,
}

/// One commit status context, as read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StatusReading {
    pub context: String,
    /// `success`, `pending`, `failure` or `error`.
    pub state: String,
}

/// What CI says about a task's commits.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CiOutcome {
    Passed,
    /// At least one check ended badly. Names the checks.
    Failed {
        failing: Vec<String>,
    },
    /// Nothing failed yet, but something is still running or has not run.
    Pending {
        waiting: Vec<String>,
    },
    /// CI could not be read, with the reason in the operator's words.
    Unreadable {
        why: String,
    },
}

/// The conclusions a finished check run may have and still count as passing.
const PASSING_CONCLUSIONS: [&str; 3] = ["success", "neutral", "skipped"];

/// Judges one commit's check runs and statuses, or `None` when it has none.
///
/// ⚠️ A RUN THAT FINISHED ANY OTHER WAY THAN PASSING IS A FAILURE: `cancelled`,
/// `timed_out`, `action_required` and `stale` included. A gate that let an
/// unfamiliar conclusion through would pass exactly the runs nobody looked at.
#[must_use]
pub fn classify_checks(runs: &[CheckRunReading], statuses: &[StatusReading]) -> Option<CiOutcome> {
    if runs.is_empty() && statuses.is_empty() {
        return None;
    }
    let mut failing = Vec::new();
    let mut waiting = Vec::new();
    for run in runs {
        if run.status != "completed" {
            waiting.push(run.name.clone());
        } else if !run
            .conclusion
            .as_deref()
            .is_some_and(|conclusion| PASSING_CONCLUSIONS.contains(&conclusion))
        {
            failing.push(run.name.clone());
        }
    }
    for status in statuses {
        match status.state.as_str() {
            "success" => {}
            "pending" => waiting.push(status.context.clone()),
            _ => failing.push(status.context.clone()),
        }
    }
    Some(outcome_of(failing, waiting))
}

fn outcome_of(mut failing: Vec<String>, mut waiting: Vec<String>) -> CiOutcome {
    if !failing.is_empty() {
        failing.sort();
        failing.dedup();
        CiOutcome::Failed { failing }
    } else if !waiting.is_empty() {
        waiting.sort();
        waiting.dedup();
        CiOutcome::Pending { waiting }
    } else {
        CiOutcome::Passed
    }
}

/// One outcome for several lines of work: any failure, then anything still
/// running, then anything unreadable, and only then a pass.
#[must_use]
pub fn combine_outcomes(outcomes: Vec<CiOutcome>) -> CiOutcome {
    let mut failing = Vec::new();
    let mut waiting = Vec::new();
    let mut unreadable = Vec::new();
    for outcome in outcomes {
        match outcome {
            CiOutcome::Passed => {}
            CiOutcome::Failed { failing: names } => failing.extend(names),
            CiOutcome::Pending { waiting: names } => waiting.extend(names),
            CiOutcome::Unreadable { why } => unreadable.push(why),
        }
    }
    if failing.is_empty() && waiting.is_empty() && !unreadable.is_empty() {
        unreadable.sort();
        unreadable.dedup();
        return CiOutcome::Unreadable {
            why: unreadable.join("; "),
        };
    }
    outcome_of(failing, waiting)
}

/// CI cannot be read on this machine at all: these two are the Hive's to fix,
/// not one repository's, so the member tells its Keeper about them.
pub const CI_GH_MISSING: &str = "gh is not installed on this machine";
pub const CI_GH_SIGNED_OUT: &str = "gh is not logged in on this machine";

/// Why this Hive cannot read CI at all, if that is what a reading says.
#[must_use]
pub fn hive_wide_ci_problem(observation: &CiObservation) -> Option<&str> {
    match &observation.outcome {
        CiOutcome::Unreadable { why } if why == CI_GH_MISSING || why == CI_GH_SIGNED_OUT => {
            Some(why.as_str())
        }
        _ => None,
    }
}

/// How long after a commit was made its CI may still be starting. GitHub
/// creates check runs moments after a push, so a commit this recent with none
/// yet is waiting for them, not a repository without CI.
pub const CI_START_GRACE_SECONDS: i64 = 15 * 60;

/// What a commit with no checks at all means: CI that has not started yet on
/// a fresh commit, or, after the grace period, a repository with no CI on it.
#[must_use]
pub fn no_checks_outcome(commit_age_seconds: i64, looked_at: usize) -> CiOutcome {
    if commit_age_seconds < CI_START_GRACE_SECONDS {
        CiOutcome::Pending {
            waiting: vec!["CI to start".to_owned()],
        }
    } else {
        CiOutcome::Unreadable {
            why: format!(
                "no CI ran on this work or the {looked_at} pushed commits after it, so the repository may have none set up"
            ),
        }
    }
}

/// Identifies the set of commits an observation was taken for, so a reading
/// taken before a task recorded more commits is never mistaken for one after.
#[must_use]
pub fn commits_digest(shas: &[String]) -> String {
    let mut sorted: Vec<&str> = shas.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.join(",")
}

/// CI as last read for a task.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CiObservation {
    /// [`commits_digest`] of the commits this was read for.
    pub commits: String,
    /// The commits CI was judged on: the tips of the task's work, or the first
    /// pushed commit after each tip that CI ran on.
    pub judged: Vec<String>,
    #[serde(flatten)]
    pub outcome: CiOutcome,
    pub observed_at: i64,
}

/// Whether work may close, as the gate sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateVerdict {
    Allow,
    /// It may close, and the record says CI was not checked, and why.
    AllowUnchecked {
        why: String,
    },
    /// It may not close yet, and why.
    Hold {
        why: String,
    },
}

/// The gate itself.
///
/// ⚠️ WORK WITH NO RECORDED COMMITS IS NEVER HELD: there is nothing for CI to
/// have run on. That is also what leaves Jira-mirrored and paperwork tasks
/// alone.
#[must_use]
pub fn gate_verdict(
    level: CheckGateLevel,
    recorded_commits: &[String],
    observation: Option<&CiObservation>,
) -> GateVerdict {
    if level != CheckGateLevel::Require || recorded_commits.is_empty() {
        return GateVerdict::Allow;
    }
    let Some(observation) =
        observation.filter(|observed| observed.commits == commits_digest(recorded_commits))
    else {
        return GateVerdict::Hold {
            why: "CI has not been read for this task's latest commits yet".to_owned(),
        };
    };
    let on = short_shas(&observation.judged);
    match &observation.outcome {
        CiOutcome::Passed => GateVerdict::Allow,
        CiOutcome::Failed { failing } => GateVerdict::Hold {
            why: format!("CI failed on {on}: {}", failing.join(", ")),
        },
        CiOutcome::Pending { waiting } => GateVerdict::Hold {
            why: format!(
                "CI hasn't finished on {on}: waiting for {}",
                waiting.join(", ")
            ),
        },
        CiOutcome::Unreadable { why } => GateVerdict::AllowUnchecked { why: why.clone() },
    }
}

fn short_shas(shas: &[String]) -> String {
    if shas.is_empty() {
        return "its commits".to_owned();
    }
    shas.iter()
        .map(|sha| sha.get(..8).unwrap_or(sha))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, status: &str, conclusion: Option<&str>) -> CheckRunReading {
        CheckRunReading {
            name: name.to_owned(),
            status: status.to_owned(),
            conclusion: conclusion.map(str::to_owned),
        }
    }

    fn status(context: &str, state: &str) -> StatusReading {
        StatusReading {
            context: context.to_owned(),
            state: state.to_owned(),
        }
    }

    /// ⚠️ ONLY THREE WAYS TO PASS. Anything else that finished is a failure, so a
    /// conclusion this code has never seen cannot slip through.
    #[test]
    fn a_check_passes_only_by_succeeding_or_being_skipped() {
        assert_eq!(classify_checks(&[], &[]), None, "no checks is not a pass");
        assert_eq!(
            classify_checks(
                &[
                    run("rust", "completed", Some("success")),
                    run("docs", "completed", Some("skipped")),
                    run("lint", "completed", Some("neutral")),
                ],
                &[status("ci/legacy", "success")],
            ),
            Some(CiOutcome::Passed)
        );
        for bad in [
            "failure",
            "cancelled",
            "timed_out",
            "action_required",
            "stale",
            "novel",
        ] {
            assert_eq!(
                classify_checks(&[run("rust", "completed", Some(bad))], &[]),
                Some(CiOutcome::Failed {
                    failing: vec!["rust".to_owned()]
                }),
                "{bad} is not a pass"
            );
        }
        assert_eq!(
            classify_checks(
                &[
                    run("rust", "in_progress", None),
                    run("web", "completed", Some("success"))
                ],
                &[status("deploy", "pending")],
            ),
            Some(CiOutcome::Pending {
                waiting: vec!["deploy".to_owned(), "rust".to_owned()]
            })
        );
        assert_eq!(
            classify_checks(
                &[run("rust", "in_progress", None)],
                &[status("ci/legacy", "error")],
            ),
            Some(CiOutcome::Failed {
                failing: vec!["ci/legacy".to_owned()]
            }),
            "a failure decides even while something else runs"
        );
    }

    #[test]
    fn several_lines_combine_worst_first() {
        let failed = CiOutcome::Failed {
            failing: vec!["rust".to_owned()],
        };
        let pending = CiOutcome::Pending {
            waiting: vec!["web".to_owned()],
        };
        let unreadable = CiOutcome::Unreadable {
            why: "no CI is set up".to_owned(),
        };
        assert_eq!(
            combine_outcomes(vec![CiOutcome::Passed, pending.clone(), failed.clone()]),
            failed
        );
        assert_eq!(
            combine_outcomes(vec![unreadable.clone(), pending.clone()]),
            pending
        );
        assert_eq!(
            combine_outcomes(vec![CiOutcome::Passed, unreadable.clone()]),
            unreadable
        );
        assert_eq!(combine_outcomes(vec![CiOutcome::Passed]), CiOutcome::Passed);
    }

    /// The operator's rulings, decision 01a1378c: require holds on failed or
    /// running CI; CI that cannot be read lets the work close and says so.
    #[test]
    fn require_holds_on_failing_or_running_ci_and_lets_unreadable_through_with_a_reason() {
        let commits = vec!["bbbb2222cccc".to_owned(), "aaaa1111dddd".to_owned()];
        let observed = |outcome| CiObservation {
            commits: commits_digest(&commits),
            judged: vec!["bbbb2222cccc".to_owned()],
            outcome,
            observed_at: 10,
        };
        let require = CheckGateLevel::Require;

        assert_eq!(
            gate_verdict(CheckGateLevel::Off, &commits, None),
            GateVerdict::Allow
        );
        assert_eq!(
            gate_verdict(CheckGateLevel::Warn, &commits, None),
            GateVerdict::Allow,
            "warn shows CI and never holds"
        );
        assert_eq!(
            gate_verdict(require, &[], None),
            GateVerdict::Allow,
            "no recorded commits, nothing for CI to have run on"
        );
        assert_eq!(
            gate_verdict(require, &commits, None),
            GateVerdict::Hold {
                why: "CI has not been read for this task's latest commits yet".to_owned()
            }
        );
        assert_eq!(
            gate_verdict(require, &commits, Some(&observed(CiOutcome::Passed))),
            GateVerdict::Allow
        );
        assert_eq!(
            gate_verdict(
                require,
                &commits,
                Some(&observed(CiOutcome::Failed {
                    failing: vec!["rust".to_owned(), "web".to_owned()]
                }))
            ),
            GateVerdict::Hold {
                why: "CI failed on bbbb2222: rust, web".to_owned()
            }
        );
        assert_eq!(
            gate_verdict(
                require,
                &commits,
                Some(&observed(CiOutcome::Pending {
                    waiting: vec!["rust".to_owned()]
                }))
            ),
            GateVerdict::Hold {
                why: "CI hasn't finished on bbbb2222: waiting for rust".to_owned()
            }
        );
        assert_eq!(
            gate_verdict(
                require,
                &commits,
                Some(&observed(CiOutcome::Unreadable {
                    why: "no CI is set up for this repository".to_owned()
                }))
            ),
            GateVerdict::AllowUnchecked {
                why: "no CI is set up for this repository".to_owned()
            }
        );

        // A reading taken before the task recorded another commit is stale.
        let more = vec![
            "bbbb2222cccc".to_owned(),
            "aaaa1111dddd".to_owned(),
            "eeee3333ffff".to_owned(),
        ];
        assert!(matches!(
            gate_verdict(require, &more, Some(&observed(CiOutcome::Passed))),
            GateVerdict::Hold { .. }
        ));
    }

    /// ⚠️ A FRESH PUSH HAS NO CHECKS FOR A MOMENT. Reading that as "no CI here"
    /// would let require close the work unchecked seconds before CI started.
    #[test]
    fn a_commit_with_no_checks_waits_for_ci_to_start_before_counting_as_unchecked() {
        assert_eq!(
            no_checks_outcome(60, 0),
            CiOutcome::Pending {
                waiting: vec!["CI to start".to_owned()]
            }
        );
        assert!(matches!(
            no_checks_outcome(CI_START_GRACE_SECONDS, 3),
            CiOutcome::Unreadable { why } if why.contains("the 3 pushed commits after it")
        ));
    }

    #[test]
    fn levels_have_one_spelling_each() {
        for level in CheckGateLevel::ALL {
            assert_eq!(level.to_string().parse::<CheckGateLevel>(), Ok(level));
        }
        assert!("strict".parse::<CheckGateLevel>().is_err());
        assert_eq!(CheckGateLevel::default(), CheckGateLevel::Off);
    }
}
