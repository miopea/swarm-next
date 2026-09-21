//! What the Apiary expects everyone to be running, and where each Hive stands.
//!
//! The version each member reports already arrives on the capability report.
//! What was missing was the other half of the comparison: WHICH release the
//! Apiary considers current, and SINCE WHEN.

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    PublicHiveIdentity, ReleaseOffer, SwarmVersion, VersionStanding, version_standing,
};

use crate::{CURRENT_SCHEMA_VERSION, TaskStore, TaskStoreError};

pub(super) fn migrate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_expected_release (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            version TEXT NOT NULL,
            first_seen_at INTEGER NOT NULL CHECK(first_seen_at >= 0)
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::FLEET_VERSION_SCHEMA_MARKER)
}

/// Where one Hive stands against the release the Apiary expects.
#[derive(Clone, Debug)]
pub struct HiveVersionStanding {
    pub identity: PublicHiveIdentity,
    pub swarm_version: String,
    pub database_schema_version: i64,
    /// When the Hive derived this, by its own clock. A raise against a report
    /// from last week is a different thing from one against a fresh report, and
    /// the surface needs to be able to say which it has.
    pub observed_at: i64,
    pub standing: VersionStanding,
}

/// The whole fleet's standing, plus what it was judged against.
#[derive(Clone, Debug)]
pub struct FleetVersionReport {
    /// The release the Apiary expects, and when it first appeared.
    ///
    /// ⚠️ `None` IS A FACT THAT MUST REACH THE SURFACE. It means no release
    /// check has ever returned an offer, so nothing can be called behind. Every
    /// Hive then reads `Unknown` rather than `Current`, and a reader that
    /// silently drops this field would show a green fleet built entirely out of
    /// not having looked.
    pub expected_release: Option<(String, i64)>,
    /// The schema members are measured against: KEEPER'S OWN.
    ///
    /// Not the expected release's schema, which Keeper has no way to know from
    /// a release offer. Keeper's own is the right reference anyway, because the
    /// question schema drift actually asks is whether a member can read what
    /// Keeper sends — and that is Keeper's schema against theirs, not a proxy
    /// for it. The limit: if Keeper itself is behind, no member reads as schema
    /// behind, which is why the version comparison is kept separate.
    pub expected_schema_version: i64,
    pub hives: Vec<HiveVersionStanding>,
}

impl FleetVersionReport {
    /// The Hives this report RAISES, as opposed to merely lists.
    #[must_use]
    pub fn raised(&self) -> Vec<&HiveVersionStanding> {
        self.hives
            .iter()
            .filter(|hive| hive.standing.raises())
            .collect()
    }
}

impl TaskStore {
    /// Records the newest release the Apiary knows about.
    ///
    /// `first_seen_at` moves ONLY when the version actually changes. The grace
    /// window is measured from it, so stamping it on every check would restart
    /// the clock repeatedly and a fleet left behind would never be raised —
    /// the same self-restarting-clock bug that makes `observed_at` the wrong
    /// anchor on the member side.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn note_expected_release(&self, version: &str, now: i64) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO apiary_expected_release (singleton, version, first_seen_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE
                 SET version = excluded.version,
                     first_seen_at = excluded.first_seen_at
                 WHERE apiary_expected_release.version <> excluded.version",
            params![version, now],
        )?;
        Ok(())
    }

    /// The release the Apiary expects, and when it first appeared.
    ///
    /// Falls back to the last verified release offer when nothing has been
    /// recorded yet, so a Hive that checked before this table existed is not
    /// blind until its next check comes due — which could be a day away.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn expected_release(&self) -> Result<Option<(String, i64)>, TaskStoreError> {
        let connection = self.connection()?;
        let recorded = connection
            .query_row(
                "SELECT version, first_seen_at FROM apiary_expected_release WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        drop(connection);
        if let Some(recorded) = recorded {
            return Ok(Some(recorded));
        }
        let state = self.release_check_state()?;
        let Some(offer) = state.last_offer.as_deref() else {
            return Ok(None);
        };
        let Ok(offer) = serde_json::from_str::<ReleaseOffer>(offer) else {
            return Ok(None);
        };
        // The check time is the only stamp available for an offer recorded
        // before this table existed. It is later than the real cut, which makes
        // the grace window generous rather than premature — the safe direction.
        Ok(state
            .last_checked_at
            .map(|checked_at| (offer.version, checked_at)))
    }

    /// Where every Hive in the Apiary stands.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn fleet_version_report(&self, now: i64) -> Result<FleetVersionReport, TaskStoreError> {
        let expected_release = self.expected_release()?;
        let expected = expected_release
            .as_ref()
            .and_then(|(version, _)| SwarmVersion::parse(version));
        let cut_at = expected_release
            .as_ref()
            .map_or(0, |(_, first_seen_at)| *first_seen_at);
        let hives = self
            .federation_hive_capabilities()?
            .into_iter()
            .map(|report| HiveVersionStanding {
                standing: version_standing(
                    &report.payload.swarm_version,
                    report.payload.database_schema_version,
                    expected.as_ref(),
                    CURRENT_SCHEMA_VERSION,
                    cut_at,
                    now,
                ),
                identity: report.payload.identity,
                swarm_version: report.payload.swarm_version,
                database_schema_version: report.payload.database_schema_version,
                observed_at: report.observed_at,
            })
            .collect();
        Ok(FleetVersionReport {
            expected_release,
            expected_schema_version: CURRENT_SCHEMA_VERSION,
            hives,
        })
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{HiveCapabilityWorker, ProviderKind, VERSION_GRACE_SECONDS};

    use super::*;
    use crate::CURRENT_SCHEMA_VERSION;

    fn worker() -> HiveCapabilityWorker {
        HiveCapabilityWorker {
            name: "Platform".to_owned(),
            provider: ProviderKind::ClaudeCode,
            repository: Some("git@github.com:rcg/platform.git".to_owned()),
            awake: false,
        }
    }

    /// ⚠️ THE TICKET'S WHOLE POINT, END TO END. This Hive sat on a stale build
    /// for a day in September while every screen that could have said so showed
    /// nothing. The acceptance is not that the version is visible — it is that a
    /// Hive left behind past the grace window is RAISED.
    #[test]
    fn a_member_left_behind_past_the_grace_window_is_raised_at_keeper() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let report = member
            .seal_local_hive_capability(
                &[worker()],
                false,
                "1.11.0",
                CURRENT_SCHEMA_VERSION,
                now + 9,
            )
            .unwrap();
        keeper
            .accept_hive_capability(&credential, &report, now + 10)
            .unwrap();
        keeper.note_expected_release("1.12.0", now + 20).unwrap();

        let fresh = keeper.fleet_version_report(now + 20).unwrap();
        assert_eq!(fresh.hives.len(), 1);
        assert_eq!(fresh.hives[0].standing, VersionStanding::BehindWithinGrace);
        assert!(
            fresh.raised().is_empty(),
            "a release cut moments ago must not fire across the fleet"
        );

        let settled = keeper
            .fleet_version_report(now + 20 + VERSION_GRACE_SECONDS)
            .unwrap();
        assert_eq!(settled.hives[0].standing, VersionStanding::Behind);
        assert_eq!(settled.raised().len(), 1, "and then it must say so");
        assert_eq!(settled.raised()[0].swarm_version, "1.11.0");
        assert_eq!(
            settled.expected_release.as_ref().map(|(v, _)| v.as_str()),
            Some("1.12.0")
        );
    }

    /// ⚠️ THE CLOCK MUST NOT RESTART. The grace window is measured from when the
    /// release first appeared, so re-recording the same release on every daily
    /// check would push the deadline out by a day, every day, and a fleet left
    /// behind would never once be raised.
    #[test]
    fn re_recording_the_same_release_does_not_restart_the_grace_window() {
        let now = 120_000;
        let (keeper, _member) = crate::federation::tests::joined_member(now);
        keeper.note_expected_release("1.12.0", now).unwrap();
        keeper
            .note_expected_release("1.12.0", now + 10 * VERSION_GRACE_SECONDS)
            .unwrap();
        assert_eq!(
            keeper.expected_release().unwrap(),
            Some(("1.12.0".to_owned(), now)),
            "the same release keeps the stamp it first arrived with"
        );

        keeper
            .note_expected_release("1.13.0", now + 10 * VERSION_GRACE_SECONDS)
            .unwrap();
        assert_eq!(
            keeper.expected_release().unwrap(),
            Some(("1.13.0".to_owned(), now + 10 * VERSION_GRACE_SECONDS)),
            "a genuinely new release does start its own window"
        );
    }

    /// ⚠️ FAILING OPEN IS THE FAILURE THIS GUARDS. A Keeper that never managed a
    /// release check knows nothing, and "I do not know" must not be rendered as
    /// a fleet that is fine.
    #[test]
    fn with_no_release_ever_seen_the_fleet_reads_unknown_rather_than_current() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let report = member
            .seal_local_hive_capability(
                &[worker()],
                false,
                "1.11.0",
                CURRENT_SCHEMA_VERSION,
                now + 9,
            )
            .unwrap();
        keeper
            .accept_hive_capability(&credential, &report, now + 10)
            .unwrap();

        let fleet = keeper.fleet_version_report(now + 999_999).unwrap();
        assert_eq!(fleet.expected_release, None, "and the surface is told so");
        assert_eq!(fleet.hives[0].standing, VersionStanding::Unknown);
    }

    /// Schema drift is measured against Keeper's own, so it is caught even
    /// while the Apiary has no idea which release is current.
    #[test]
    fn a_member_behind_keepers_schema_is_raised_without_any_release_check() {
        let now = 120_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let report = member
            .seal_local_hive_capability(
                &[worker()],
                false,
                "1.12.0",
                CURRENT_SCHEMA_VERSION - 1,
                now + 9,
            )
            .unwrap();
        keeper
            .accept_hive_capability(&credential, &report, now + 10)
            .unwrap();

        let fleet = keeper.fleet_version_report(now + 11).unwrap();
        assert_eq!(fleet.expected_schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(fleet.hives[0].standing, VersionStanding::SchemaBehind);
        assert_eq!(
            fleet.raised().len(),
            1,
            "schema drift has no grace window to wait out"
        );
    }
}
