use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::BrowserEvidenceHour;
use thiserror::Error;

use crate::{TaskStore, TaskStoreError};

const MAX_CAPTURES: i64 = 4_096;
const RETENTION_SECONDS: i64 = 90 * 86_400;

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("invalid browser evidence")]
    Invalid,
    #[error("browser evidence conflicts with an existing capture")]
    Conflict,
    #[error("stored browser evidence is invalid")]
    Corrupt,
    #[error(transparent)]
    Store(#[from] TaskStoreError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceWrite {
    pub updated: bool,
    pub pruned: usize,
}

pub(super) fn migrate(tx: &Transaction<'_>, version: i64) -> rusqlite::Result<()> {
    if version >= crate::DOGFOOD_EVIDENCE_SCHEMA_VERSION {
        return Ok(());
    }
    tx.execute_batch(
        "CREATE TABLE browser_evidence (
            capture_id TEXT PRIMARY KEY,
            hour INTEGER NOT NULL CHECK(hour >= 0 AND hour % 3600 = 0),
            payload TEXT NOT NULL CHECK(length(CAST(payload AS BLOB)) <= 4096)
        );
        CREATE INDEX browser_evidence_hour ON browser_evidence(hour, capture_id);",
    )?;
    tx.pragma_update(None, "user_version", crate::DOGFOOD_EVIDENCE_SCHEMA_VERSION)
}

fn decode(payload: &str) -> Result<BrowserEvidenceHour, EvidenceError> {
    let evidence: BrowserEvidenceHour =
        serde_json::from_str(payload).map_err(|_| EvidenceError::Corrupt)?;
    if payload.len() > 4_096 || !evidence.valid() {
        return Err(EvidenceError::Corrupt);
    }
    Ok(evidence)
}

fn prune(tx: &Transaction<'_>, now: i64) -> Result<usize, EvidenceError> {
    let expired = tx.execute(
        "DELETE FROM browser_evidence WHERE hour < ?1",
        [now.saturating_sub(RETENTION_SECONDS)],
    )?;
    let overflow = tx.execute(
        "DELETE FROM browser_evidence WHERE capture_id IN (
            SELECT capture_id FROM browser_evidence ORDER BY hour DESC, capture_id DESC
            LIMIT -1 OFFSET ?1)",
        [MAX_CAPTURES],
    )?;
    Ok(expired + overflow)
}

impl TaskStore {
    /// Atomically replaces cumulative evidence and enforces its retention budget.
    ///
    /// # Errors
    /// Rejects invalid input, rewritten captures, or persistence failures.
    pub fn record_browser_evidence(
        &self,
        evidence: &BrowserEvidenceHour,
        now: i64,
    ) -> Result<EvidenceWrite, EvidenceError> {
        if !evidence.valid_at(now) {
            return Err(EvidenceError::Invalid);
        }
        let payload = serde_json::to_string(evidence).map_err(|_| EvidenceError::Invalid)?;
        if payload.len() > 4_096 {
            return Err(EvidenceError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT payload FROM browser_evidence WHERE capture_id = ?1",
                [evidence.capture_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let updated = match previous {
            None => true,
            Some(payload) => replacement(evidence, &decode(&payload)?)?,
        };
        if updated {
            tx.execute(
                "INSERT INTO browser_evidence(capture_id,hour,payload) VALUES(?1,?2,?3)
                ON CONFLICT(capture_id) DO UPDATE SET payload=excluded.payload",
                params![evidence.capture_id.to_string(), evidence.hour, payload],
            )?;
        }
        let pruned = prune(&tx, now)?;
        tx.commit()?;
        Ok(EvidenceWrite { updated, pruned })
    }

    /// Reads at most 100 newest captures, pruning expired evidence first.
    ///
    /// # Errors
    /// Returns invalid clock, stored-data integrity, or persistence errors.
    pub fn browser_evidence(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<BrowserEvidenceHour>, EvidenceError> {
        if now < 0 {
            return Err(EvidenceError::Invalid);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        prune(&tx, now)?;
        let values = {
            let mut statement = tx.prepare(
                "SELECT payload FROM browser_evidence
                ORDER BY hour DESC, capture_id DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit.min(100)], |row| row.get::<_, String>(0))?;
            rows.map(|row| decode(&row?))
                .collect::<Result<Vec<_>, EvidenceError>>()?
        };
        tx.commit()?;
        Ok(values)
    }
}

fn replacement(
    next: &BrowserEvidenceHour,
    prior: &BrowserEvidenceHour,
) -> Result<bool, EvidenceError> {
    if next.capture_id != prior.capture_id || next.build != prior.build || next.hour != prior.hour {
        return Err(EvidenceError::Conflict);
    }
    if next.revision < prior.revision || next == prior {
        return Ok(false);
    }
    if !next.extends(prior) {
        return Err(EvidenceError::Conflict);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::TimingAggregate;
    use uuid::Uuid;

    fn evidence(id: u128) -> BrowserEvidenceHour {
        BrowserEvidenceHour {
            capture_id: Uuid::from_u128(id),
            build: "1.4.1-dev-abc".into(),
            hour: 172_800,
            revision: 1,
            long_task: TimingAggregate::default(),
            interaction: TimingAggregate::default(),
            route: TimingAggregate::default(),
            terminal_render: TimingAggregate::default(),
            terminal_reconnect: TimingAggregate::default(),
        }
    }

    #[test]
    fn retries_replace_once_and_conflicts_preserve_the_previous_capture() {
        let store = TaskStore::in_memory().unwrap();
        let first = evidence(1);
        assert!(
            store
                .record_browser_evidence(&first, first.hour)
                .unwrap()
                .updated
        );
        assert!(
            !store
                .record_browser_evidence(&first, first.hour)
                .unwrap()
                .updated
        );
        let mut next = first.clone();
        next.revision = 2;
        next.route = TimingAggregate {
            count: 1,
            total_ms: 10,
            max_ms: 10,
        };
        assert!(
            store
                .record_browser_evidence(&next, next.hour)
                .unwrap()
                .updated
        );
        assert!(
            !store
                .record_browser_evidence(&first, first.hour)
                .unwrap()
                .updated
        );
        let mut conflict = next.clone();
        conflict.route = TimingAggregate::default();
        assert!(matches!(
            store.record_browser_evidence(&conflict, conflict.hour),
            Err(EvidenceError::Conflict)
        ));
        conflict.revision = 3;
        assert!(matches!(
            store.record_browser_evidence(&conflict, conflict.hour),
            Err(EvidenceError::Conflict)
        ));
        assert_eq!(store.browser_evidence(next.hour, 100).unwrap(), vec![next]);
    }

    #[test]
    fn schema_125_migrates_and_reopen_preserves_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("evidence.sqlite");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TABLE operator_statements; ALTER TABLE task_dispatches DROP COLUMN generation; DROP TABLE worker_startup_context; DROP TABLE browser_evidence; PRAGMA user_version = 125;")
                .unwrap();
        }
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .record_browser_evidence(&evidence(1), 172_800)
                .unwrap();
        }
        let reopened = TaskStore::open(&path).unwrap();
        assert_eq!(
            reopened.browser_evidence(172_800, 100).unwrap(),
            vec![evidence(1)]
        );
        let version: i64 = reopened
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, crate::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn invalid_uploads_do_not_mutate_existing_evidence() {
        let store = TaskStore::in_memory().unwrap();
        let first = evidence(1);
        store.record_browser_evidence(&first, first.hour).unwrap();
        assert!(matches!(
            store.record_browser_evidence(&first, first.hour + 86_401),
            Err(EvidenceError::Invalid)
        ));
        let mut changed = first.clone();
        changed.build = "other".into();
        assert!(matches!(
            store.record_browser_evidence(&changed, first.hour),
            Err(EvidenceError::Conflict)
        ));
        changed = first.clone();
        changed.hour += 3_600;
        assert!(matches!(
            store.record_browser_evidence(&changed, changed.hour),
            Err(EvidenceError::Conflict)
        ));
        assert_eq!(
            store.browser_evidence(first.hour, 100).unwrap(),
            vec![first]
        );
    }

    #[test]
    fn retention_and_read_caps_are_enforced() {
        let store = TaskStore::in_memory().unwrap();
        for id in 1..=4_097 {
            store
                .record_browser_evidence(&evidence(id), 172_800)
                .unwrap();
        }
        assert_eq!(
            store.browser_evidence(172_800, u32::MAX).unwrap().len(),
            100
        );
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM browser_evidence", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, MAX_CAPTURES);
        assert_eq!(
            store
                .browser_evidence(172_800 + RETENTION_SECONDS, 1)
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .browser_evidence(172_800 + RETENTION_SECONDS + 1, 100)
                .unwrap()
                .is_empty()
        );
    }
}
