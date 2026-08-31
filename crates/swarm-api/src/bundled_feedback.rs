//! The credential Swarm ships so a fresh install can report a bug about Swarm.
//!
//! WHY THIS EXISTS. Anonymous feedback shipped in 1.0.0 as a first-class
//! feature and worked only when the person installing had personally obtained a
//! GitHub token and written it into `swarm.env`. The operator's words:
//! "so you are telling me that devs need to install settings to make it work?
//! that is stupid ... instead you want me to give out a token?!?". Two things
//! were wrong at once — every installer had to do credential setup before a
//! core feature worked at all, and every stranger's report went out under the
//! operator's own account.
//!
//! THIS IS AN EMBEDDED SECRET AND IT IS NOT HIDDEN. The token is a string
//! literal in a distributed binary. `strings` finds it. Anyone holding the
//! artefact holds the credential, and nothing here obfuscates or encodes it,
//! because an encoding would imply a protection that does not exist and would
//! only mislead the next reader. That is the trade the operator accepted, with
//! the blast radius stated: decision 01a05973-0855-71b0-8708-d57dfbecf86d,
//! "Ship a token scoped to issues-write on the Swarm repo only".
//!
//! THE SCOPING IS THE DECISION, NOT A DETAIL OF IT. A fine-grained token with
//! `issues: write` on ONE repository and no other permission. What makes this
//! shape acceptable is that the worst an extracted copy can do is open issues
//! on the Swarm repository. Widen the scope — a classic PAT, `repo`, "issues
//! plus a bit more because it was easier" — and the operator agreed to
//! something else. If a change here appears to need a broader permission, that
//! is a question to take back to them, not a detail to settle in a commit.
//!
//! ROTATION IS A RELEASE. Replacing the credential means revoking it at GitHub,
//! putting the new value in 1Password, and building again. That is the floor
//! and it is inherent to shipping a secret in an artefact. What is avoidable is
//! making it worse, so the value is named ONCE, here, and reaches the build
//! through a single environment variable.

/// The only repository the shipped credential may be used against.
///
/// Pinned rather than configurable, and that is a safety property rather than a
/// convenience: the token is scoped to `issues: write` on this repository
/// alone, so pairing it with any other destination could only produce a
/// confusing failure — or, worse, quietly work if the scope were ever widened.
/// An operator who wants their own destination sets `SWARM_GITHUB_REPOSITORY`
/// and `SWARM_GITHUB_TOKEN`, which takes precedence over everything here.
pub const BUNDLED_FEEDBACK_REPOSITORY: &str = "miopea/swarm-next";

/// The name of the single environment variable that carries the token into a
/// build, stated once so a reader and the packaging script cannot disagree.
pub const BUNDLED_FEEDBACK_TOKEN_VAR: &str = "SWARM_BUNDLED_FEEDBACK_TOKEN";

/// Where a Hive with no operator configuration files feedback, if this build
/// carries a credential at all.
///
/// `None` is a legitimate build, not a broken one: a developer's `cargo build`
/// carries no token and should not, and a release built without the variable
/// set is a Hive that cannot file. What must not happen is that state being
/// silent, which is why `github_readiness` reports it and the dialog says so.
///
/// An empty or whitespace-only value is treated as absent rather than passed
/// on. It would be rejected downstream — `GithubFeedback::new` refuses an empty
/// token — but "the variable was set to nothing" is the shape a shell produces
/// when a secret lookup failed, and it should read as no credential rather than
/// as a malformed one.
#[must_use]
pub fn bundled_feedback_destination() -> Option<(&'static str, &'static str)> {
    match option_env!("SWARM_BUNDLED_FEEDBACK_TOKEN") {
        Some(token) if !token.trim().is_empty() => {
            Some((BUNDLED_FEEDBACK_REPOSITORY, token.trim()))
        }
        _ => None,
    }
}

/// Where a Hive files feedback, decided from what the operator set.
///
/// A PURE FUNCTION IN THE LIBRARY rather than a `match` inside `main.rs`,
/// because the precedence is the part of this change that carries the
/// acceptance criterion — "no anonymous submission is attributed to a
/// credential belonging to a person who did not write it" — and a rule living
/// only in a binary's private function cannot be asserted by anything.
#[derive(Debug, Eq, PartialEq)]
pub enum FeedbackDestination {
    /// The operator named their own repository and token. Theirs wins, always.
    Operator { repository: String, token: String },
    /// Neither variable set, and this build ships a credential.
    Bundled {
        repository: &'static str,
        token: &'static str,
    },
    /// One variable without the other: a mistake worth naming, and NOT a case
    /// the shipped credential quietly rescues. An operator who named their own
    /// repository is telling us where their reports go; filing them into the
    /// Swarm repository instead would send their words somewhere they did not
    /// ask for, which is a worse failure than declining to file.
    HalfCredential,
    /// Nothing set and nothing shipped. Reports stay on this Hive.
    Nowhere,
}

/// Applies the precedence. `repository` and `token` are the two environment
/// variables, already read.
#[must_use]
pub fn feedback_destination(
    repository: Option<String>,
    token: Option<String>,
) -> FeedbackDestination {
    match (repository, token) {
        (Some(repository), Some(token)) => FeedbackDestination::Operator { repository, token },
        (Some(_), None) | (None, Some(_)) => FeedbackDestination::HalfCredential,
        (None, None) => match bundled_feedback_destination() {
            Some((repository, token)) => FeedbackDestination::Bundled { repository, token },
            None => FeedbackDestination::Nowhere,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE SCOPE IS THE DECISION, so the pairing that enforces it is asserted
    /// rather than left to a reader's care. The token is valid for exactly one
    /// repository; a future edit that let a caller choose the destination while
    /// keeping the shipped credential would hand a repo-scoped token to an
    /// arbitrary repository, which is the one thing this shape may not do.
    #[test]
    fn the_shipped_credential_names_its_own_repository_and_takes_no_other() {
        // The function returns the repository; it does not accept one.
        if let Some((repository, _)) = bundled_feedback_destination() {
            assert_eq!(repository, BUNDLED_FEEDBACK_REPOSITORY);
        }
        assert_eq!(BUNDLED_FEEDBACK_REPOSITORY, "miopea/swarm-next");
    }

    /// A build with no variable set carries no credential, and that is a
    /// legitimate configuration rather than a failure.
    #[test]
    fn a_build_without_the_variable_carries_no_credential() {
        if option_env!("SWARM_BUNDLED_FEEDBACK_TOKEN").is_none() {
            assert!(bundled_feedback_destination().is_none());
        }
    }

    /// The mirror of the case above, and the one that proves the embedding
    /// path works at all rather than merely compiling. Run it with the variable
    /// set to a placeholder:
    ///
    /// ```text
    /// SWARM_BUNDLED_FEEDBACK_TOKEN=placeholder cargo test -p swarm-api bundled_feedback
    /// ```
    ///
    /// Conditional on purpose. An unconditional assertion here would fail every
    /// ordinary `cargo test`, and a test that must be skipped to get a green run
    /// is a test nobody keeps.
    #[test]
    fn a_build_with_the_variable_set_carries_exactly_that_credential() {
        if let Some(compiled) = option_env!("SWARM_BUNDLED_FEEDBACK_TOKEN") {
            if compiled.trim().is_empty() {
                assert!(bundled_feedback_destination().is_none());
            } else {
                let (repository, token) =
                    bundled_feedback_destination().expect("a non-empty compiled token");
                assert_eq!(repository, BUNDLED_FEEDBACK_REPOSITORY);
                // Compared against the compiled value, never printed.
                assert_eq!(token, compiled.trim());
            }
        }
    }

    /// The variable is named once. A packaging script that sets a different
    /// name ships a token-less artefact that looks fine, which is exactly the
    /// silent failure this task exists to remove.
    #[test]
    fn the_build_variable_is_named_once_and_matches_what_the_code_reads() {
        assert_eq!(BUNDLED_FEEDBACK_TOKEN_VAR, "SWARM_BUNDLED_FEEDBACK_TOKEN");
    }
}

#[cfg(test)]
mod precedence_tests {
    use super::*;

    /// THE OPERATOR'S OWN DESTINATION WINS. A Hive whose owner triages their own
    /// issues is what the environment variables were built for, and shipping a
    /// credential must not quietly redirect their reports to the Swarm repo.
    #[test]
    fn an_operator_who_names_a_repository_and_token_files_there() {
        assert_eq!(
            feedback_destination(
                Some("acme/widgets".to_owned()),
                Some("their-own-token".to_owned())
            ),
            FeedbackDestination::Operator {
                repository: "acme/widgets".to_owned(),
                token: "their-own-token".to_owned(),
            }
        );
    }

    /// THE ACCEPTANCE CRITERION, AS AN ASSERTION. The shipped credential is
    /// scoped to `issues: write` on ONE repository. If half a credential fell
    /// through to the bundled arm, an operator who set only
    /// `SWARM_GITHUB_REPOSITORY` would have their reports filed into the Swarm
    /// repository under a credential they never chose — the exact confusion
    /// this task exists to remove, running in the other direction.
    #[test]
    fn half_a_credential_is_never_rescued_by_the_shipped_one() {
        assert_eq!(
            feedback_destination(Some("acme/widgets".to_owned()), None),
            FeedbackDestination::HalfCredential
        );
        assert_eq!(
            feedback_destination(None, Some("a-token".to_owned())),
            FeedbackDestination::HalfCredential
        );
    }

    /// The ordinary case, and the one the operator was complaining about: a
    /// fresh install with nothing set. It files, with no setup, and it files
    /// into the repository the shipped credential is scoped to and no other.
    #[test]
    fn a_fresh_install_files_into_the_repository_the_credential_is_scoped_to() {
        match feedback_destination(None, None) {
            FeedbackDestination::Bundled { repository, .. } => {
                assert_eq!(repository, BUNDLED_FEEDBACK_REPOSITORY);
            }
            // A build carrying no credential — every ordinary `cargo test` —
            // keeps reports local. Legitimate, and the dialog says so.
            FeedbackDestination::Nowhere => {
                assert!(bundled_feedback_destination().is_none());
            }
            other => panic!("nothing set should not produce {other:?}"),
        }
    }

    /// The shipped credential can only ever be paired with its own repository,
    /// because the only arm that produces it supplies both halves itself. This
    /// asserts the type does not offer a way to combine them wrongly.
    #[test]
    fn the_shipped_credential_cannot_be_pointed_at_another_repository() {
        let elsewhere = feedback_destination(Some("acme/widgets".to_owned()), Some("t".to_owned()));
        match elsewhere {
            FeedbackDestination::Operator { repository, token } => {
                assert_eq!(repository, "acme/widgets");
                // Their token, not the shipped one.
                assert_eq!(token, "t");
                assert_ne!(
                    Some(token.as_str()),
                    bundled_feedback_destination().map(|(_, t)| t)
                );
            }
            other => panic!("expected the operator's own destination, got {other:?}"),
        }
    }
}
