//! What each member last said about its health, and when it reached the Keeper
//! over the last day (ADR 0114). Both live on the Keeper.

use rusqlite::{OptionalExtension, params};
use swarm_domain::{CONTACT_HISTORY_SECONDS, HiveHealthReport, HiveId, contact_slot};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

/// One member's latest health report, as the Keeper holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredHiveHealth {
    pub hive_id: HiveId,
    /// When the Keeper received it, on the Keeper's clock.
    pub received_at: i64,
    pub report: HiveHealthReport,
}

/// Adds the latest-report table and the contact-history slots.
///
/// The slots are bounded by construction: one row per Hive per five minutes,
/// and anything older than a day is deleted as each new one is written.
pub(crate) fn migrate_hive_health(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_hive_health (
             apiary_id TEXT NOT NULL,
             hive_id TEXT NOT NULL,
             received_at INTEGER NOT NULL,
             report_json TEXT NOT NULL,
             PRIMARY KEY (apiary_id, hive_id)
         );
         CREATE TABLE IF NOT EXISTS apiary_hive_fault_episodes (
             hive_id TEXT NOT NULL,
             fault TEXT NOT NULL,
             since INTEGER NOT NULL,
             PRIMARY KEY (hive_id, fault)
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS apiary_member_contact_slots (
             apiary_id TEXT NOT NULL,
             hive_id TEXT NOT NULL,
             slot_start INTEGER NOT NULL,
             PRIMARY KEY (apiary_id, hive_id, slot_start)
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", crate::HIVE_HEALTH_SCHEMA_MARKER)
}

/// Marks the five minutes `now` falls in as a time `hive` reached the Keeper,
/// and forgets anything older than a day.
///
/// Called where contact is already recorded, so it is written at most once per
/// half minute per member, whatever the member asks for.
pub(crate) fn record_contact_slot(
    connection: &rusqlite::Connection,
    apiary: &str,
    hive: &str,
    now: i64,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT OR IGNORE INTO apiary_member_contact_slots (apiary_id, hive_id, slot_start)
         VALUES (?1, ?2, ?3)",
        params![apiary, hive, contact_slot(now)],
    )?;
    connection.execute(
        "DELETE FROM apiary_member_contact_slots
         WHERE apiary_id = ?1 AND hive_id = ?2 AND slot_start < ?3",
        params![apiary, hive, contact_slot(now) - CONTACT_HISTORY_SECONDS],
    )?;
    Ok(())
}

impl TaskStore {
    /// Keeps what one member says about itself, replacing what it said before.
    ///
    /// # Errors
    /// Rejects an invalid or expired credential, a report outside its bounds,
    /// and unavailable persistence.
    pub fn accept_hive_health(
        &self,
        credential: &str,
        report: &HiveHealthReport,
        now: i64,
    ) -> Result<HiveId, TaskStoreError> {
        report
            .validate()
            .map_err(|_| TaskStoreError::InvalidFederationSync)?;
        let credential = crate::federation::decode_node_credential(credential)?;
        let identity = self.local_hive_identity()?;
        let report_json = serde_json::to_string(report)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = crate::federation::authenticate_member_credential(
            &transaction,
            &identity,
            &credential,
            now,
        )?;
        transaction.execute(
            "INSERT INTO apiary_hive_health (apiary_id, hive_id, received_at, report_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (apiary_id, hive_id) DO UPDATE SET
                 received_at = excluded.received_at,
                 report_json = excluded.report_json",
            params![
                member.apiary.to_string(),
                member.hive.to_string(),
                now,
                report_json
            ],
        )?;
        transaction.commit()?;
        Ok(member.hive)
    }

    /// The latest report from each current member of this Keeper's Apiary.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a partial fleet.
    pub fn apiary_hive_health(&self) -> Result<Vec<StoredHiveHealth>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT h.hive_id, h.received_at, h.report_json
             FROM apiary_hive_health h
             JOIN apiary_federation_memberships m
               ON m.apiary_id = h.apiary_id AND m.member_hive_id = h.hive_id
             WHERE m.state = 'active'
             ORDER BY h.hive_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut found = Vec::new();
        for row in rows {
            let (hive, received_at, report) = row?;
            found.push(StoredHiveHealth {
                hive_id: parse_domain_id(&hive)?,
                received_at,
                report: serde_json::from_str(&report)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
            });
        }
        Ok(found)
    }

    /// When `fault` began on `hive`, starting it now if it was not already
    /// under way. A fault that clears and comes back is a new episode, which
    /// is what lets an answered alert hold until something new happens.
    ///
    /// # Errors
    /// Returns unavailable persistence.
    pub fn open_hive_fault_episode(
        &self,
        hive: HiveId,
        fault: &str,
        now: i64,
    ) -> Result<i64, TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO apiary_hive_fault_episodes (hive_id, fault, since)
             VALUES (?1, ?2, ?3)",
            params![hive.to_string(), fault, now],
        )?;
        Ok(connection.query_row(
            "SELECT since FROM apiary_hive_fault_episodes WHERE hive_id = ?1 AND fault = ?2",
            params![hive.to_string(), fault],
            |row| row.get(0),
        )?)
    }

    /// Every fault episode under way, as (Hive, fault, since).
    ///
    /// # Errors
    /// Returns unavailable persistence or a corrupt Hive id.
    pub fn hive_fault_episodes(&self) -> Result<Vec<(HiveId, String, i64)>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT hive_id, fault, since FROM apiary_hive_fault_episodes ORDER BY hive_id, fault",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                parse_domain_id::<HiveId>(&row.get::<_, String>(0)?)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Ends a fault episode: the fault has cleared.
    ///
    /// # Errors
    /// Returns unavailable persistence.
    pub fn close_hive_fault_episode(
        &self,
        hive: HiveId,
        fault: &str,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "DELETE FROM apiary_hive_fault_episodes WHERE hive_id = ?1 AND fault = ?2",
            params![hive.to_string(), fault],
        )?;
        Ok(())
    }

    /// The starts of the five-minute slots in the last day when `hive` reached
    /// this Keeper, oldest first.
    ///
    /// # Errors
    /// Returns unavailable persistence.
    pub fn member_contact_history(
        &self,
        hive: HiveId,
        now: i64,
    ) -> Result<Vec<i64>, TaskStoreError> {
        let connection = self.connection()?;
        let apiary: Option<String> = connection
            .query_row(
                "SELECT apiary_id FROM apiary_federation_memberships
                 WHERE member_hive_id = ?1 AND state = 'active'",
                [hive.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(apiary) = apiary else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT slot_start FROM apiary_member_contact_slots
             WHERE apiary_id = ?1 AND hive_id = ?2 AND slot_start >= ?3
             ORDER BY slot_start",
        )?;
        let slots = statement
            .query_map(
                params![
                    apiary,
                    hive.to_string(),
                    contact_slot(now) - CONTACT_HISTORY_SECONDS
                ],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        CONTACT_SLOT_SECONDS, EngineAttemptOutcome, FederationSyncCondition,
        HIVE_HEALTH_SCHEMA_VERSION, ReportedDisk, ReportedEngine, ReportedSync, ResourcePressure,
    };

    use super::*;

    fn report(stuck_workers: u32) -> HiveHealthReport {
        HiveHealthReport {
            schema_version: HIVE_HEALTH_SCHEMA_VERSION,
            observed_at: 1,
            sync: ReportedSync {
                condition: FederationSyncCondition::Incompatible,
                failed_step: Some("project catalog".to_owned()),
                last_success_at: None,
                consecutive_failures: 3,
            },
            engine: ReportedEngine {
                update_pending: true,
                last_attempt: Some(EngineAttemptOutcome::Failed),
            },
            disk: ReportedDisk {
                pressure: ResourcePressure::Advisory,
                lowest_available_bytes: Some(1 << 30),
            },
            memory: ResourcePressure::Normal,
            stuck_workers,
            ci_unreadable: None,
        }
    }

    #[test]
    fn a_member_report_replaces_its_last_and_only_a_member_may_send_one() {
        let now = 140_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let hive = member.local_hive_identity().unwrap().hive.id;

        assert_eq!(
            keeper
                .accept_hive_health(&credential, &report(1), now + 10)
                .unwrap(),
            hive
        );
        keeper
            .accept_hive_health(&credential, &report(2), now + 20)
            .unwrap();
        assert_eq!(
            keeper.apiary_hive_health().unwrap(),
            [StoredHiveHealth {
                hive_id: hive,
                received_at: now + 20,
                report: report(2),
            }]
        );

        assert!(matches!(
            keeper.accept_hive_health("not-a-credential", &report(1), now + 30),
            Err(TaskStoreError::InvalidFederationCredential)
        ));
        let mut unknown = report(1);
        unknown.schema_version = HIVE_HEALTH_SCHEMA_VERSION + 1;
        assert!(
            keeper
                .accept_hive_health(&credential, &unknown, now + 30)
                .is_err()
        );
    }

    /// ⚠️ BOUNDED BY CONSTRUCTION. A member reaches its Keeper every few
    /// seconds for as long as it lives; a history that only grew would be the
    /// "invisible infinity" the charter forbids.
    #[test]
    fn contact_history_keeps_one_day_of_five_minute_slots_and_shows_the_gaps() {
        let start = 2_000_000 - 2_000_000 % CONTACT_SLOT_SECONDS;
        let (keeper, member) = crate::federation::tests::joined_member(start);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let hive = member.local_hive_identity().unwrap().hive.id;
        let contact = |at: i64| {
            keeper.authenticated_member_hive(&credential, at).unwrap();
        };
        // Every half minute for 26 hours, except a three-hour outage.
        let outage = (start + 10 * 3_600)..(start + 13 * 3_600);
        let end = start + 26 * 3_600;
        for at in (start + 6..end).step_by(30) {
            if !outage.contains(&at) {
                contact(at);
            }
        }

        let slots = keeper.member_contact_history(hive, end).unwrap();
        let day = usize::try_from(CONTACT_HISTORY_SECONDS / CONTACT_SLOT_SECONDS).unwrap();
        assert!(slots.len() <= day + 1, "{} slots", slots.len());
        assert!(slots.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(slots[0] >= contact_slot(end) - CONTACT_HISTORY_SECONDS);
        // The outage is the gap, and only it.
        let missing = (slots[0]..=*slots.last().unwrap())
            .step_by(usize::try_from(CONTACT_SLOT_SECONDS).unwrap())
            .filter(|slot| !slots.contains(slot))
            .collect::<Vec<_>>();
        assert_eq!(missing.first(), Some(&outage.start));
        assert_eq!(
            i64::try_from(missing.len()).unwrap(),
            3 * 3_600 / CONTACT_SLOT_SECONDS
        );
        // And nothing older than a day is kept in the table at all.
        let kept: i64 = keeper
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM apiary_member_contact_slots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(usize::try_from(kept).unwrap() <= day + 1, "{kept} rows");
    }
}
