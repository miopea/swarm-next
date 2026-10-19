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

use crate::{MEMBER_QUIET_SECONDS, SwarmVersion};

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
    expected_schema: Option<i64>,
    release_cut_at: i64,
    now: i64,
) -> VersionStanding {
    // `None` when Keeper runs a development build: its schema is not any
    // release's schema, and judging members against it raised a Hive on the
    // newest published release as behind minutes after Keeper was rebuilt.
    let schema_behind = expected_schema.is_some_and(|schema| reported_schema < schema);
    let Some(reported) = SwarmVersion::parse(reported_version) else {
        return VersionStanding::Unreadable;
    };
    if reported.is_development() {
        return if schema_behind {
            VersionStanding::SchemaBehind
        } else {
            VersionStanding::Development
        };
    }
    let Some(expected) = expected else {
        return if schema_behind {
            VersionStanding::SchemaBehind
        } else {
            VersionStanding::Unknown
        };
    };
    if expected.supersedes(&reported) {
        // ⚠️ THE SAME GRACE WHETHER OR NOT THE SCHEMA IS BEHIND TOO. A release
        // that migrates is still an ordinary release, and a member that
        // installs it by itself (ADR 0110) needs the window to do so before
        // anyone is asked about it. Raising at once asked the operator about
        // every migrating release across the whole fleet.
        if now.saturating_sub(release_cut_at) < VERSION_GRACE_SECONDS {
            return VersionStanding::BehindWithinGrace;
        }
        return if schema_behind {
            VersionStanding::SchemaBehind
        } else {
            VersionStanding::Behind
        };
    }
    // On the expected release with an older schema is a migration that did not
    // run, which no amount of waiting fixes.
    if schema_behind {
        VersionStanding::SchemaBehind
    } else {
        VersionStanding::Current
    }
}

/// When a Hive's current spell in touch with its Keeper began, from the starts
/// of the five-minute slots it reached the Keeper in (oldest first) and the
/// last time it did. `None` while it is quiet.
///
/// ⚠️ A QUIET HIVE IS NEVER PUSHED ABOUT, BEHIND OR NOT. The 2026-09-30 ruling
/// is to warn about silence where the operator goes to use the Hive, and the
/// roster already says Quiet. Every "Hives are behind" raise for 1.17.4 named
/// only Hives that had been silent for days, and none of them could have done
/// anything until it came back (ADR 0110 §5).
///
/// A spell breaks where two slots are further apart than a Hive may go unheard
/// before it reads as quiet, so one missed slot is not a return. A spell older
/// than the slots kept reaches back to the oldest of them, which is a day.
#[must_use]
pub fn in_touch_since(slots: &[i64], last_contact_at: Option<i64>, now: i64) -> Option<i64> {
    let last = last_contact_at.filter(|at| now.saturating_sub(*at) < MEMBER_QUIET_SECONDS)?;
    let mut since = slots.last().map_or(last, |slot| (*slot).min(last));
    for pair in slots.windows(2).rev() {
        if pair[1].saturating_sub(pair[0]) > MEMBER_QUIET_SECONDS {
            break;
        }
        since = pair[0];
    }
    Some(since)
}

/// When a Hive's grace for a release starts: when the release appeared, or
/// when the Hive came back into touch after that, whichever is later.
///
/// ⚠️ A HIVE CANNOT INSTALL WHAT IT HAS NOT SEEN. One that was switched off
/// when the release appeared was past the window the moment it came back, and
/// was raised before it had checked once. Honeysuckle came back at 14:05 on
/// 2026-10-19 and was on 1.17.4 ten minutes later, by itself. A Hive that stays
/// in touch keeps the spell it started with, so phoning home does not restart
/// the clock.
#[must_use]
pub fn grace_starts_at(release_cut_at: i64, in_touch_since: Option<i64>) -> i64 {
    in_touch_since.map_or(release_cut_at, |since| since.max(release_cut_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected() -> SwarmVersion {
        SwarmVersion::parse("1.12.0").unwrap()
    }

    #[test]
    fn a_hive_on_the_expected_release_is_current_and_raises_nothing() {
        let standing = version_standing("1.12.0", 186, Some(&expected()), Some(186), 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Current);
        assert!(!standing.raises());
    }

    #[test]
    fn a_hive_ahead_of_the_apiary_is_not_behind() {
        assert_eq!(
            version_standing("1.13.0", 186, Some(&expected()), Some(186), 0, 10_000_000),
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
            Some(186),
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
            Some(186),
            cut_at,
            cut_at + VERSION_GRACE_SECONDS,
        );
        assert_eq!(outside, VersionStanding::Behind);
        assert!(outside.raises(), "and it must eventually say so");
    }

    /// ⚠️ A MEMBER ON THE EXPECTED RELEASE WITH AN OLDER SCHEMA IS RAISED AT
    /// ONCE, and so is a development build: that is a migration that did not
    /// run, which waiting does not fix.
    #[test]
    fn schema_drift_is_raised_immediately_and_even_for_a_dev_build() {
        let cut_at = 1_000_000;
        assert_eq!(
            version_standing(
                "1.12.0",
                185,
                Some(&expected()),
                Some(186),
                cut_at,
                cut_at + 1
            ),
            VersionStanding::SchemaBehind,
            "no grace for a schema mismatch on the expected release"
        );
        assert_eq!(
            version_standing(
                "1.12.0-dev-a597c3ad7f3f-20260921195509-1986653",
                185,
                Some(&expected()),
                Some(186),
                cut_at,
                cut_at + 1
            ),
            VersionStanding::SchemaBehind,
            "a development build is exempt from version drift, not from schema drift"
        );
    }

    /// A release that migrates is still an ordinary release. A member behind on
    /// it waits out the same window as any other, so it can install the
    /// release by itself before anyone is asked (ADR 0110).
    #[test]
    fn a_member_behind_a_migrating_release_gets_the_same_grace() {
        let cut_at = 1_000_000;
        assert_eq!(
            version_standing(
                "1.11.0",
                185,
                Some(&expected()),
                Some(186),
                cut_at,
                cut_at + 1
            ),
            VersionStanding::BehindWithinGrace
        );
        assert_eq!(
            version_standing(
                "1.11.0",
                185,
                Some(&expected()),
                Some(186),
                cut_at,
                cut_at + VERSION_GRACE_SECONDS
            ),
            VersionStanding::SchemaBehind,
            "and says it is the schema once the window has passed"
        );
    }

    /// ⚠️ THE 2026-09-25 FALSE ALARM. Keeper was rebuilt from a working copy with
    /// a migration no release carried yet, and a member on the newest published
    /// release was raised as `schema_behind` minutes later.
    #[test]
    fn a_development_keeper_does_not_judge_members_by_its_own_schema() {
        let standing = version_standing("1.12.0", 185, Some(&expected()), None, 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Current);
        assert!(!standing.raises());
    }

    #[test]
    fn a_development_build_is_exempt_from_the_version_raise() {
        let standing = version_standing(
            "1.12.0-dev-a597c3ad7f3f-20260921195509-1986653",
            186,
            Some(&SwarmVersion::parse("1.13.0").unwrap()),
            Some(186),
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
        let standing = version_standing("1.11.0", 186, None, Some(186), 0, 10_000_000);
        assert_eq!(standing, VersionStanding::Unknown);
        assert!(
            !standing.raises(),
            "one missing check is a fact about the Apiary, not a fault in every member"
        );
        assert_eq!(
            version_standing("1.11.0", 185, None, Some(186), 0, 10_000_000),
            VersionStanding::SchemaBehind,
            "schema is measured against Keeper directly, so it survives a missing release check"
        );
    }

    #[test]
    fn an_unreadable_version_is_raised_rather_than_assumed_current() {
        let standing = version_standing(
            "not-a-version",
            186,
            Some(&expected()),
            Some(186),
            0,
            10_000_000,
        );
        assert_eq!(standing, VersionStanding::Unreadable);
        assert!(standing.raises());
    }

    /// ⚠️ THE 1.17.4 RAISES. Every one named only Hives silent for days.
    #[test]
    fn a_quiet_hive_is_not_in_touch_whatever_its_history() {
        let now = 1_000_000;
        let slots = (0..12)
            .map(|slot| now - 7_200 + slot * 300)
            .collect::<Vec<_>>();
        assert_eq!(
            in_touch_since(&slots, Some(now - MEMBER_QUIET_SECONDS), now),
            None,
            "ten minutes unheard is quiet, as the roster says"
        );
        assert_eq!(in_touch_since(&[], None, now), None, "never heard from");
        assert_eq!(
            in_touch_since(&slots, Some(now - MEMBER_QUIET_SECONDS + 1), now),
            Some(now - 7_200)
        );
    }

    #[test]
    fn a_spell_in_touch_starts_after_the_last_quiet_gap_and_survives_one_missed_slot() {
        let now = 1_000_000;
        let back = now - 3_000;
        // Two hours online, an hour away, then back: the spell is the return.
        let mut slots = (0..24)
            .map(|slot| now - 12_000 + slot * 300)
            .collect::<Vec<_>>();
        slots.extend((0..=10).map(|slot| back + slot * 300));
        assert_eq!(in_touch_since(&slots, Some(now - 5), now), Some(back));

        // One missed five-minute slot is not a return.
        let steady = [now - 1_500, now - 1_200, now - 600, now - 300];
        assert_eq!(
            in_touch_since(&steady, Some(now - 5), now),
            Some(now - 1_500)
        );
        // Two are: that is longer than a Hive may go unheard.
        let away = [now - 1_500, now - 1_200, now - 300];
        assert_eq!(in_touch_since(&away, Some(now - 5), now), Some(now - 300));
    }

    #[test]
    fn grace_runs_from_the_release_or_the_hives_return_whichever_is_later() {
        assert_eq!(
            grace_starts_at(1_000, None),
            1_000,
            "a quiet Hive keeps the release's clock"
        );
        assert_eq!(
            grace_starts_at(1_000, Some(500)),
            1_000,
            "in touch since before the release: phoning home does not restart it"
        );
        assert_eq!(
            grace_starts_at(1_000, Some(9_000)),
            9_000,
            "back after it: from the return"
        );
    }
}
