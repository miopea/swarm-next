//! Worker-busy time on this Hive, and, on a Keeper, what each member last
//! reported it spent (ADR 0114 addendum).

use rusqlite::params;
use swarm_domain::{HiveId, HiveUsageReport, MAX_ACTIVE_SECONDS_PER_DAY, UsageActive};

use crate::{TaskStore, TaskStoreError, parse_domain_id};

/// One member's latest usage report, as the Keeper holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredHiveUsage {
    pub hive_id: HiveId,
    /// When the Keeper received it, on the Keeper's clock.
    pub received_at: i64,
    pub report: HiveUsageReport,
}

/// Adds this Hive's own busy-time totals and the Keeper's copy of each
/// member's report. Both are bounded: busy time is one row per workspace per
/// day and is pruned to the usage window as it is written; a member's report
/// is replaced, never appended to.
pub(crate) fn migrate_hive_usage(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_active_daily (
             day TEXT NOT NULL,
             workspace TEXT NOT NULL,
             seconds INTEGER NOT NULL CHECK (seconds >= 0),
             PRIMARY KEY (day, workspace)
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS apiary_hive_usage (
             apiary_id TEXT NOT NULL,
             hive_id TEXT NOT NULL,
             received_at INTEGER NOT NULL,
             report_json TEXT NOT NULL,
             PRIMARY KEY (apiary_id, hive_id)
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::HIVE_USAGE_SCHEMA_MARKER)
}

impl TaskStore {
    /// Adds busy seconds to each workspace on `day`, and forgets days before
    /// `keep_from`. Bounded at `MAX_ACTIVE_SECONDS_PER_DAY`.
    ///
    /// # Errors
    /// Returns unavailable persistence.
    pub fn record_worker_active_seconds(
        &self,
        day: &str,
        busy: &[(String, i64)],
        keep_from: &str,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for (workspace, seconds) in busy.iter().filter(|(_, seconds)| *seconds > 0) {
            transaction.execute(
                "INSERT INTO worker_active_daily (day, workspace, seconds) VALUES (?1, ?2, ?3)
                 ON CONFLICT (day, workspace) DO UPDATE SET
                     seconds = MIN(?4, seconds + excluded.seconds)",
                params![
                    day,
                    workspace,
                    (*seconds).min(MAX_ACTIVE_SECONDS_PER_DAY),
                    MAX_ACTIVE_SECONDS_PER_DAY
                ],
            )?;
        }
        transaction.execute(
            "DELETE FROM worker_active_daily WHERE day < ?1",
            [keep_from],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Busy time per workspace per day from `earliest_day` onward, oldest first.
    ///
    /// # Errors
    /// Returns unavailable persistence.
    pub fn worker_active_since(
        &self,
        earliest_day: &str,
    ) -> Result<Vec<UsageActive>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT day, workspace, seconds FROM worker_active_daily
             WHERE day >= ?1 ORDER BY day, workspace",
        )?;
        let rows = statement
            .query_map([earliest_day], |row| {
                Ok(UsageActive {
                    day: row.get(0)?,
                    workspace: row.get(1)?,
                    seconds: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Keeps what one member reports it spent, replacing what it said before.
    ///
    /// # Errors
    /// Rejects an invalid or expired credential, a report outside its bounds,
    /// and unavailable persistence.
    pub fn accept_hive_usage(
        &self,
        credential: &str,
        report: &HiveUsageReport,
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
            "INSERT INTO apiary_hive_usage (apiary_id, hive_id, received_at, report_json)
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

    /// The latest usage report from each current member of this Keeper's Apiary.
    ///
    /// # Errors
    /// Returns corrupt stored evidence rather than a partial fleet.
    pub fn apiary_hive_usage(&self) -> Result<Vec<StoredHiveUsage>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT u.hive_id, u.received_at, u.report_json
             FROM apiary_hive_usage u
             JOIN apiary_federation_memberships m
               ON m.apiary_id = u.apiary_id AND m.member_hive_id = u.hive_id
             WHERE m.state = 'active'
             ORDER BY u.hive_id",
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
            found.push(StoredHiveUsage {
                hive_id: parse_domain_id(&hive)?,
                received_at,
                report: serde_json::from_str(&report)
                    .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?,
            });
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{HIVE_USAGE_SCHEMA_VERSION, UsageTokens};

    use super::*;

    #[test]
    fn busy_time_adds_up_per_day_within_its_bound_and_old_days_go() {
        let store = TaskStore::in_memory().unwrap();
        let busy = |workspace: &str, seconds| vec![(workspace.to_owned(), seconds)];
        store
            .record_worker_active_seconds("2026-09-01", &busy("/w/a", 600), "2026-09-01")
            .unwrap();
        store
            .record_worker_active_seconds("2026-10-12", &busy("/w/a", 30), "2026-09-01")
            .unwrap();
        store
            .record_worker_active_seconds("2026-10-12", &busy("/w/a", 45), "2026-09-01")
            .unwrap();
        store
            .record_worker_active_seconds(
                "2026-10-12",
                &busy("/w/b", MAX_ACTIVE_SECONDS_PER_DAY + 1),
                "2026-09-01",
            )
            .unwrap();
        assert_eq!(
            store.worker_active_since("2026-10-01").unwrap(),
            [
                UsageActive {
                    day: "2026-10-12".to_owned(),
                    workspace: "/w/a".to_owned(),
                    seconds: 75,
                },
                UsageActive {
                    day: "2026-10-12".to_owned(),
                    workspace: "/w/b".to_owned(),
                    seconds: MAX_ACTIVE_SECONDS_PER_DAY,
                },
            ]
        );
        // Writing with a later window drops the day that fell out of it.
        store
            .record_worker_active_seconds("2026-10-12", &[], "2026-09-15")
            .unwrap();
        assert_eq!(store.worker_active_since("2026-01-01").unwrap().len(), 2);
    }

    #[test]
    fn a_member_usage_report_replaces_its_last_and_only_a_member_may_send_one() {
        let now = 150_000;
        let (keeper, member) = crate::federation::tests::joined_member(now);
        let credential = member
            .federation_member_connection()
            .unwrap()
            .node_credential;
        let hive = member.local_hive_identity().unwrap().hive.id;
        let report = |output_tokens| HiveUsageReport {
            schema_version: HIVE_USAGE_SCHEMA_VERSION,
            observed_at: now,
            tokens: vec![UsageTokens {
                day: "2026-10-12".to_owned(),
                workspace: "/home/cora/budgetbug".to_owned(),
                model: "claude-opus-5-5".to_owned(),
                input_tokens: 1,
                cache_write_tokens: 2,
                cache_read_tokens: 3,
                output_tokens,
                messages: 1,
            }],
            active: Vec::new(),
        };
        keeper
            .accept_hive_usage(&credential, &report(10), now + 10)
            .unwrap();
        keeper
            .accept_hive_usage(&credential, &report(20), now + 20)
            .unwrap();
        assert_eq!(
            keeper.apiary_hive_usage().unwrap(),
            [StoredHiveUsage {
                hive_id: hive,
                received_at: now + 20,
                report: report(20),
            }]
        );
        assert!(matches!(
            keeper.accept_hive_usage("not-a-credential", &report(1), now + 30),
            Err(TaskStoreError::InvalidFederationCredential)
        ));
    }
}
