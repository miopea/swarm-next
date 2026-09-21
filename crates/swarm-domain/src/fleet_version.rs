//! Where each Hive stands against the release the Apiary expects.
//!
//! The Apiary view shows this, and a Hive that has fallen behind is RAISED
//! rather than merely listed.
//!
//! ⚠️ THE RAISE IS THE POINT AND A COLUMN WOULD HAVE CHANGED NOTHING. This Hive
//! sat wedged on a stale build for roughly a day on 2026-09-20 — reloaded onto
//! an unmerged branch commit, then a rebase orphaned it and every reload was
//! refused. A version column would have displayed that the whole time and been
//! read by nobody. What was missing was something that SAID SO.

use serde::{Deserialize, Serialize};

use crate::SwarmVersion;

/// How long after a release is cut before a Hive still on the old one is
/// raised.
///
/// ⚠️ GENEROUS ON PURPOSE. Raising the instant a release is cut would fire
/// across the whole fleet every time and train everyone to dismiss it — and an
/// alarm that is always on is one nobody reads, which is the failure this
/// exists to prevent, wearing a different coat. The 2026-09-20 wedge ran a full
/// day, so hours rather than minutes still catches the case that motivated it.
pub const VERSION_GRACE_SECONDS: i64 = 6 * 60 * 60;

/// What the Apiary should say about one Hive's build.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionStanding {
    /// Running the expected release, or newer.
    Current,
    /// A development build.
    ///
    /// ⚠️ EXEMPT, AND THIS IS NOT A COVERAGE GAP even though it exempts exactly
    /// the Hive that fell behind on 2026-09-20. A dev Hive behind its own
    /// checkout is already reported locally by the development reload card, so
    /// dev drift moves from a fleet signal to a local one rather than
    /// disappearing. If that card is ever weakened, this exemption becomes a
    /// real hole.
    Development,
    /// Behind the expected release, but still inside the grace window.
    BehindWithinGrace,
    /// Behind the expected release for longer than the grace window.
    Behind,
    /// Behind on DATABASE SCHEMA, which is louder and has no grace.
    ///
    /// ⚠️ A DIFFERENT KIND OF PROBLEM, NOT A WORSE DEGREE OF THE SAME ONE. An
    /// app version behind is stale; a schema behind can mean a member cannot
    /// read what Keeper sends. That is correctness rather than freshness, so it
    /// is raised immediately and must read differently on screen.
    SchemaBehind,
    /// The reported version could not be parsed.
    ///
    /// Reported rather than treated as current: a Hive whose version is
    /// unreadable is not a Hive known to be fine.
    Unreadable,
    /// The Apiary does not know what the newest release is, so this Hive cannot
    /// be judged against it.
    ///
    /// ⚠️ NOT `Current`, AND THIS IS THE WHOLE REASON THE VARIANT EXISTS. A
    /// release check that never succeeded would otherwise make every Hive in
    /// the fleet read as up to date — the check failing OPEN, which is the
    /// silent negative this ticket was written to avoid. It does not raise
    /// per-Hive, because one missing check is one fact about the Apiary rather
    /// than a fault in every member; the report carries that fact instead.
    Unknown,
}

impl VersionStanding {
    /// Whether this standing should raise, rather than only display.
    #[must_use]
    pub fn raises(self) -> bool {
        matches!(self, Self::Behind | Self::SchemaBehind | Self::Unreadable)
    }
}

/// Where one Hive stands.
///
/// `release_cut_at` is when the expected release was issued, which is what the
/// grace window is measured from — NOT when the member last reported. Measuring
/// from the report would restart the clock every time a stale Hive checked in,
/// so a Hive that phoned home regularly could stay behind forever without ever
/// being raised.
#[must_use]
pub fn version_standing(
    reported_version: &str,
    reported_schema: i64,
    expected: Option<&SwarmVersion>,
    expected_schema: i64,
    release_cut_at: i64,
    now: i64,
) -> VersionStanding {
    // Schema first: it is the failure that breaks things rather than merely
    // dates them, and it applies even to a development build.
    if reported_schema < expected_schema {
        return VersionStanding::SchemaBehind;
    }
    let Some(reported) = SwarmVersion::parse(reported_version) else {
        return VersionStanding::Unreadable;
    };
    if reported.is_development() {
        return VersionStanding::Development;
    }
    let Some(expected) = expected else {
        return VersionStanding::Unknown;
    };
    if expected.supersedes(&reported) {
        if now.saturating_sub(release_cut_at) < VERSION_GRACE_SECONDS {
            return VersionStanding::BehindWithinGrace;
        }
        return VersionStanding::Behind;
    }
    VersionStanding::Current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected() -> SwarmVersion {
        SwarmVersion::parse("1.12.0").unwrap()
    }

    #[test]
    fn a_hive_on_the_expected_release_is_current_and_raises_nothing() {
        let standing = version_standing("1.12.0", 186, Some(&expected()), 186, 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Current);
        assert!(!standing.raises());
    }

    #[test]
    fn a_hive_ahead_of_the_apiary_is_not_behind() {
        assert_eq!(
            version_standing("1.13.0", 186, Some(&expected()), 186, 0, 10_000_000),
            VersionStanding::Current,
            "running newer than the Apiary expects is not a fault"
        );
    }

    /// The window is measured from when the RELEASE was cut, not from when the
    /// member last reported — otherwise a stale Hive that phones home regularly
    /// restarts its own clock and is never raised.
    #[test]
    fn being_behind_is_tolerated_briefly_and_then_raised() {
        let cut_at = 1_000_000;
        let inside = version_standing(
            "1.11.0",
            186,
            Some(&expected()),
            186,
            cut_at,
            cut_at + VERSION_GRACE_SECONDS - 1,
        );
        assert_eq!(inside, VersionStanding::BehindWithinGrace);
        assert!(
            !inside.raises(),
            "a fresh release must not fire across the fleet"
        );

        let outside = version_standing(
            "1.11.0",
            186,
            Some(&expected()),
            186,
            cut_at,
            cut_at + VERSION_GRACE_SECONDS,
        );
        assert_eq!(outside, VersionStanding::Behind);
        assert!(outside.raises(), "and it must eventually say so");
    }

    /// ⚠️ SCHEMA BEATS EVERYTHING, INCLUDING THE GRACE WINDOW AND THE DEV
    /// EXEMPTION. A schema behind can mean a member cannot read what Keeper
    /// sends, which is correctness rather than freshness.
    #[test]
    fn schema_drift_is_raised_immediately_and_even_for_a_dev_build() {
        let cut_at = 1_000_000;
        assert_eq!(
            version_standing("1.12.0", 185, Some(&expected()), 186, cut_at, cut_at + 1),
            VersionStanding::SchemaBehind,
            "no grace for a schema mismatch"
        );
        assert_eq!(
            version_standing(
                "1.12.0-dev-a597c3ad7f3f-20260921195509-1986653",
                185,
                Some(&expected()),
                186,
                cut_at,
                cut_at + 1
            ),
            VersionStanding::SchemaBehind,
            "a development build is exempt from version drift, not from schema drift"
        );
    }

    #[test]
    fn a_development_build_is_exempt_from_the_version_raise() {
        let standing = version_standing(
            "1.12.0-dev-a597c3ad7f3f-20260921195509-1986653",
            186,
            Some(&SwarmVersion::parse("1.13.0").unwrap()),
            186,
            0,
            10_000_000,
        );
        assert_eq!(standing, VersionStanding::Development);
        assert!(
            !standing.raises(),
            "a dev Hive behind its checkout is reported by its own reload card, locally"
        );
    }

    /// A Hive whose version cannot be read is not a Hive known to be fine.
    /// ⚠️ THE FAIL-OPEN CASE. If a Hive that cannot be compared read as
    /// `Current`, a release check that never succeeded would report the entire
    /// fleet as up to date while every member sat behind.
    #[test]
    fn with_no_known_release_a_hive_is_unknown_rather_than_current() {
        let standing = version_standing("1.11.0", 186, None, 186, 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Unknown);
        assert!(
            !standing.raises(),
            "one missing check is a fact about the Apiary, not a fault in every member"
        );
        assert_eq!(
            version_standing("1.11.0", 185, None, 186, 0, 10_000_000),
            VersionStanding::SchemaBehind,
            "schema is measured against Keeper directly, so it survives a missing release check"
        );
    }

    #[test]
    fn an_unreadable_version_is_raised_rather_than_assumed_current() {
        let standing =
            version_standing("not-a-version", 186, Some(&expected()), 186, 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Unreadable);
        assert!(standing.raises());
    }
}
