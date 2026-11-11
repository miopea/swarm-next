//! One `gh api` call, as the machine's logged-in GitHub user.
//!
//! The check gate (ADR 0115) and GitHub's asks of the operator (ADR 0170) both
//! read GitHub this way, so the ways a call goes wrong are named once here and
//! each reader says them in its own words.

use std::process::Command;

/// How long one call may take. `timeout` bounds a hung call; std's Command has
/// no deadline of its own.
const GH_TIMEOUT_SECONDS: &str = "20";

/// Why a `gh api` call gave no answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GhFailure {
    /// The `timeout` command itself is missing, so no call can be bounded.
    NoTimeoutCommand,
    /// GitHub did not answer in time.
    TimedOut,
    /// gh is not installed.
    Missing,
    /// gh is installed and not logged in.
    SignedOut,
    /// GitHub answered 404: the login cannot see it, or it does not exist.
    /// GitHub gives a private repository the login cannot read the same answer
    /// as one that is not there.
    NotFound,
    /// The login's rate limit is used up. Carries gh's own first line.
    RateLimited(String),
    /// GitHub answered in a shape the caller did not expect.
    UnexpectedShape,
    /// Anything else. Carries gh's own first line.
    Other(String),
}

impl GhFailure {
    /// The operator's words for it.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::NoTimeoutCommand => {
                "the timeout command is not available on this machine".to_owned()
            }
            Self::TimedOut => "GitHub did not answer within 20 seconds".to_owned(),
            Self::Missing => swarm_domain::CI_GH_MISSING.to_owned(),
            Self::SignedOut => swarm_domain::CI_GH_SIGNED_OUT.to_owned(),
            Self::NotFound => "this machine's gh login can't see that repository".to_owned(),
            Self::UnexpectedShape => "GitHub answered in a shape Swarm did not expect".to_owned(),
            Self::RateLimited(line) | Self::Other(line) => {
                format!("GitHub could not be read: {line}")
            }
        }
    }
}

/// Calls `gh api <path> --jq <jq>` and parses what it printed.
pub(crate) fn gh_api<T: serde::de::DeserializeOwned>(path: &str, jq: &str) -> Result<T, GhFailure> {
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
        .map_err(|_| GhFailure::NoTimeoutCommand)?;
    if output.status.success() {
        return serde_json::from_slice(&output.stdout).map_err(|_| GhFailure::UnexpectedShape);
    }
    Err(failure(
        output.status.code(),
        &String::from_utf8_lossy(&output.stderr),
    ))
}

/// What a failed call's exit code and error output mean.
fn failure(code: Option<i32>, stderr: &str) -> GhFailure {
    let line = || {
        stderr
            .lines()
            .next()
            .unwrap_or("no reason given")
            .trim()
            .to_owned()
    };
    match code {
        Some(124) => GhFailure::TimedOut,
        Some(127) => GhFailure::Missing,
        _ if stderr.contains("gh auth login") || stderr.contains("not logged") => {
            GhFailure::SignedOut
        }
        _ if stderr.contains("HTTP 404") => GhFailure::NotFound,
        _ if stderr.contains("HTTP 429") || stderr.to_ascii_lowercase().contains("rate limit") => {
            GhFailure::RateLimited(line())
        }
        _ => GhFailure::Other(line()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_call_is_named_by_what_gh_said() {
        assert_eq!(failure(Some(124), ""), GhFailure::TimedOut);
        assert_eq!(failure(Some(127), ""), GhFailure::Missing);
        assert_eq!(
            failure(
                Some(4),
                "To get started with GitHub CLI, please run:  gh auth login"
            ),
            GhFailure::SignedOut
        );
        assert_eq!(
            failure(Some(1), "gh: Not Found (HTTP 404)"),
            GhFailure::NotFound
        );
        assert_eq!(
            failure(
                Some(1),
                "gh: API rate limit exceeded for user ID 1. (HTTP 403)\nmore"
            ),
            GhFailure::RateLimited("gh: API rate limit exceeded for user ID 1. (HTTP 403)".into())
        );
        assert_eq!(
            failure(
                Some(1),
                "gh: You have exceeded a secondary rate limit (HTTP 403)"
            ),
            GhFailure::RateLimited(
                "gh: You have exceeded a secondary rate limit (HTTP 403)".into()
            )
        );
        assert_eq!(
            failure(Some(1), "gh: Too Many Requests (HTTP 429)"),
            GhFailure::RateLimited("gh: Too Many Requests (HTTP 429)".into())
        );
        assert_eq!(
            failure(Some(1), "gh: Server Error (HTTP 502)"),
            GhFailure::Other("gh: Server Error (HTTP 502)".into())
        );
        assert_eq!(
            failure(None, ""),
            GhFailure::Other("no reason given".into())
        );
    }

    #[test]
    fn the_check_gate_keeps_its_words() {
        assert_eq!(GhFailure::Missing.describe(), swarm_domain::CI_GH_MISSING);
        assert_eq!(
            GhFailure::SignedOut.describe(),
            swarm_domain::CI_GH_SIGNED_OUT
        );
        assert_eq!(
            GhFailure::TimedOut.describe(),
            "GitHub did not answer within 20 seconds"
        );
        assert_eq!(
            GhFailure::NotFound.describe(),
            "this machine's gh login can't see that repository"
        );
        assert_eq!(
            GhFailure::RateLimited("gh: API rate limit exceeded".into()).describe(),
            "GitHub could not be read: gh: API rate limit exceeded"
        );
    }
}
