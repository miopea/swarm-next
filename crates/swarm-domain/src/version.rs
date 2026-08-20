//! What a Swarm version is, and how two of them compare.
//!
//! A release carries a plain semantic version — `0.2.0` — and nothing else. A
//! development build carries the same base plus the revision it was built from
//! and when: `0.1.0-dev-5394d9a6b872-20260820201150-900012`.
//!
//! The distinction is the point. Until now every build, release or not, carried
//! a revision suffix, so two releases could not be ordered and an updater had
//! nothing to compare. A build from someone's working copy must also never be
//! mistaken for a release that others can be offered.

use std::cmp::Ordering;
use std::fmt;

/// A parsed Swarm version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwarmVersion {
    major: u64,
    minor: u64,
    patch: u64,
    /// Present only for a build made from a working copy.
    development: Option<DevelopmentBuild>,
}

/// What a development build carries beyond its base version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevelopmentBuild {
    /// The revision the working copy was at.
    pub revision: String,
    /// Build stamp, `YYYYMMDDHHMMSS`, which orders two builds of one revision.
    pub built_at: String,
}

impl SwarmVersion {
    /// Reads a version string as produced by the packaging scripts.
    ///
    /// # Errors
    /// Returns `None` when the string is not a version this product produces.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (base, rest) = match text.split_once("-dev-") {
            Some((base, rest)) => (base, Some(rest)),
            None => (text, None),
        };
        let mut parts = base.split('.');
        let mut number = || parts.next()?.parse::<u64>().ok();
        let (major, minor, patch) = (number()?, number()?, number()?);
        if parts.next().is_some() {
            return None;
        }
        let development = match rest {
            None => None,
            Some(rest) => {
                let mut fields = rest.split('-');
                let revision = fields.next()?.to_owned();
                let built_at = fields.next()?.to_owned();
                if revision.is_empty() || built_at.is_empty() {
                    return None;
                }
                Some(DevelopmentBuild { revision, built_at })
            }
        };
        Some(Self {
            major,
            minor,
            patch,
            development,
        })
    }

    /// Whether this build came from a working copy rather than a release.
    #[must_use]
    pub fn is_development(&self) -> bool {
        self.development.is_some()
    }

    /// The development detail, absent on a release.
    #[must_use]
    pub fn development(&self) -> Option<&DevelopmentBuild> {
        self.development.as_ref()
    }

    /// Whether `self` is a release that supersedes `other`.
    ///
    /// Only releases are offered as updates. A development build is not an
    /// upgrade for anyone — it exists on one machine and its contents are
    /// whatever was in that checkout — and nothing is an upgrade over a
    /// development build, because there is no telling what it already carries.
    #[must_use]
    pub fn supersedes(&self, other: &Self) -> bool {
        if self.is_development() || other.is_development() {
            return false;
        }
        self.release_order().gt(&other.release_order())
    }

    fn release_order(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

/// Releases order by number. A development build sorts beneath the release it
/// was built from, because it is unfinished work on the way to one, and two of
/// them order by when they were built.
impl Ord for SwarmVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.release_order()
            .cmp(&other.release_order())
            .then_with(|| match (&self.development, &other.development) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left
                    .built_at
                    .cmp(&right.built_at)
                    .then_with(|| left.revision.cmp(&right.revision)),
            })
    }
}

impl PartialOrd for SwarmVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for SwarmVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(build) = &self.development {
            write!(formatter, "-dev-{}-{}", build.revision, build.built_at)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_a_plain_semantic_version() {
        let release = SwarmVersion::parse("0.2.0").unwrap();
        assert!(!release.is_development());
        assert_eq!(release.to_string(), "0.2.0");
    }

    #[test]
    fn a_development_build_carries_where_and_when_it_came_from() {
        let build = SwarmVersion::parse("0.1.0-dev-5394d9a6b872-20260820201150-900012").unwrap();
        assert!(build.is_development());
        let detail = build.development().unwrap();
        assert_eq!(detail.revision, "5394d9a6b872");
        assert_eq!(detail.built_at, "20260820201150");
    }

    /// The reason the scheme changed: two releases have to be comparable.
    #[test]
    fn releases_order_by_number() {
        let older = SwarmVersion::parse("0.2.0").unwrap();
        let newer = SwarmVersion::parse("0.10.0").unwrap();
        assert!(newer.supersedes(&older));
        assert!(!older.supersedes(&newer));
        // Not string order: "0.10.0" sorts before "0.2.0" as text.
        assert!(newer > older);
    }

    /// A build from someone's working copy is not an update for anyone, and
    /// nothing is an update over one — there is no telling what it contains.
    #[test]
    fn development_builds_are_never_offered_as_updates() {
        let release = SwarmVersion::parse("0.9.0").unwrap();
        let build = SwarmVersion::parse("0.1.0-dev-5394d9a6b872-20260820201150-1").unwrap();
        assert!(!release.supersedes(&build));
        assert!(!build.supersedes(&release));
    }

    #[test]
    fn two_builds_of_one_revision_order_by_when_they_were_built() {
        let earlier = SwarmVersion::parse("0.1.0-dev-aaaaaaaaaaaa-20260820201150-1").unwrap();
        let later = SwarmVersion::parse("0.1.0-dev-aaaaaaaaaaaa-20260820201151-2").unwrap();
        assert!(later > earlier);
    }

    #[test]
    fn a_development_build_sits_beneath_the_release_it_was_built_from() {
        let release = SwarmVersion::parse("0.1.0").unwrap();
        let build = SwarmVersion::parse("0.1.0-dev-aaaaaaaaaaaa-20260820201150-1").unwrap();
        assert!(release > build);
    }

    #[test]
    fn anything_that_is_not_one_of_ours_is_refused() {
        for text in [
            "",
            "0.1",
            "0.1.0.1",
            "v0.1.0",
            "0.1.x",
            "0.1.0-dev-",
            "0.1.0-dev-abc",
        ] {
            assert!(SwarmVersion::parse(text).is_none(), "{text} parsed");
        }
    }
}
