//! One `gh api` call, as the machine's logged-in GitHub user.
//!
//! The check gate (ADR 0115) and GitHub's asks of the operator (ADR 0170) both
//! read GitHub this way, so the ways a call goes wrong are named once here and
//! each reader says them in its own words.

use std::io::Write as _;
use std::process::{Command, Stdio};

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

/// Why a write to GitHub did not clearly succeed.
///
/// The split is the whole point. A refused write did not happen, so the
/// operator may simply try again. An unknown one may have happened, so it is
/// settled by reading GitHub back, never by sending it again (ADR 0176).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GhWriteFailure {
    /// It did not happen: gh could not send it, or GitHub answered no. In
    /// GitHub's own words when it gave them.
    Refused(String),
    /// It may have happened: no answer in time, a server error, or no answer
    /// at all.
    Unknown(String),
}

/// Sends one write through `gh api`, with `body` as JSON on stdin so nothing
/// the operator wrote reaches the process list, and parses what `jq` printed.
pub(crate) fn gh_api_write<T: serde::de::DeserializeOwned>(
    method: &str,
    path: &str,
    body: &serde_json::Value,
    jq: &str,
) -> Result<T, GhWriteFailure> {
    let mut child = Command::new("timeout")
        .args([
            GH_TIMEOUT_SECONDS,
            "gh",
            "api",
            "--method",
            method,
            path,
            "--input",
            "-",
            "--jq",
            jq,
        ])
        .env_remove("CLICOLOR_FORCE")
        .env_remove("FORCE_COLOR")
        .env_remove("GH_FORCE_TTY")
        .env("NO_COLOR", "1")
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| GhWriteFailure::Refused(GhFailure::NoTimeoutCommand.describe()))?;
    if let Some(mut stdin) = child.stdin.take() {
        // Nothing can have reached GitHub if the body never reached gh.
        stdin
            .write_all(body.to_string().as_bytes())
            .map_err(|_| GhWriteFailure::Refused("the request could not be handed to gh".into()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| GhWriteFailure::Unknown(format!("gh did not finish: {error}")))?;
    if output.status.success() {
        // It happened. A reply in a shape Swarm did not expect is settled by
        // reading back, like any other unknown.
        return serde_json::from_slice(&output.stdout)
            .map_err(|_| GhWriteFailure::Unknown(GhFailure::UnexpectedShape.describe()));
    }
    Err(write_failure(
        output.status.code(),
        &String::from_utf8_lossy(&output.stderr),
    ))
}

/// Whether a failed write happened, from gh's exit code and error output.
fn write_failure(code: Option<i32>, stderr: &str) -> GhWriteFailure {
    match failure(code, stderr) {
        failure @ (GhFailure::NoTimeoutCommand | GhFailure::Missing | GhFailure::SignedOut) => {
            GhWriteFailure::Refused(failure.describe())
        }
        GhFailure::NotFound => GhWriteFailure::Refused(GhFailure::NotFound.describe()),
        GhFailure::RateLimited(line) => GhWriteFailure::Refused(githubs_words(&line)),
        failure @ (GhFailure::TimedOut | GhFailure::UnexpectedShape) => {
            GhWriteFailure::Unknown(failure.describe())
        }
        // GitHub answered 4xx: it read the request and said no.
        GhFailure::Other(line) if line.contains("(HTTP 4") => {
            GhWriteFailure::Refused(githubs_words(&line))
        }
        // A server error, or no HTTP answer at all: it may have landed.
        GhFailure::Other(line) => GhWriteFailure::Unknown(githubs_words(&line)),
    }
}

/// `gh: Pull Request is not mergeable (HTTP 405)` as GitHub said it.
fn githubs_words(line: &str) -> String {
    let line = line.strip_prefix("gh: ").unwrap_or(line);
    match line.rfind(" (HTTP ") {
        Some(at) if line.ends_with(')') => line[..at].to_owned(),
        _ => line.to_owned(),
    }
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
    fn a_write_github_refused_did_not_happen_and_one_unanswered_may_have() {
        assert_eq!(
            write_failure(
                Some(1),
                "gh: Can not approve your own pull request (HTTP 422)"
            ),
            GhWriteFailure::Refused("Can not approve your own pull request".into())
        );
        assert_eq!(
            write_failure(Some(1), "gh: Pull Request is not mergeable (HTTP 405)"),
            GhWriteFailure::Refused("Pull Request is not mergeable".into())
        );
        assert_eq!(
            write_failure(Some(1), "gh: Head branch was modified. (HTTP 409)"),
            GhWriteFailure::Refused("Head branch was modified.".into())
        );
        assert!(matches!(
            write_failure(
                Some(4),
                "To get started with GitHub CLI, please run:  gh auth login"
            ),
            GhWriteFailure::Refused(_)
        ));
        assert!(matches!(
            write_failure(Some(127), ""),
            GhWriteFailure::Refused(_)
        ));
        assert!(matches!(
            write_failure(Some(1), "gh: Not Found (HTTP 404)"),
            GhWriteFailure::Refused(_)
        ));
        // These may have landed, so they are never simply sent again.
        assert_eq!(
            write_failure(Some(124), ""),
            GhWriteFailure::Unknown("GitHub did not answer within 20 seconds".into())
        );
        assert_eq!(
            write_failure(Some(1), "gh: Server Error (HTTP 502)"),
            GhWriteFailure::Unknown("Server Error".into())
        );
        assert_eq!(
            write_failure(Some(1), "error connecting to api.github.com"),
            GhWriteFailure::Unknown("error connecting to api.github.com".into())
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
