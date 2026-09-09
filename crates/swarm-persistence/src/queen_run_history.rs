use rusqlite::{Transaction, params};
use swarm_domain::{QueenAutomationOutcome, QueenRunEvidence, QueenRunHistory};

use crate::{TaskStore, TaskStoreError};

const MAX_RETAINED: u32 = 4_096;
const RETENTION_DAYS: u32 = 30;
const RETENTION_SECONDS: i64 = RETENTION_DAYS as i64 * 86_400;

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS queen_run_history (
        run_id TEXT PRIMARY KEY CHECK(length(run_id) BETWEEN 1 AND 128),
        finished_on_build TEXT CHECK(finished_on_build IS NULL OR length(finished_on_build) BETWEEN 1 AND 128),
        trigger TEXT NOT NULL CHECK(trigger IN ('manual','actionable_work')),
        requested_at INTEGER,
        delivered_at INTEGER,
        finished_at INTEGER NOT NULL,
        attempts INTEGER NOT NULL CHECK(attempts BETWEEN 0 AND 4294967295),
        initial_actionable_count INTEGER NOT NULL CHECK(initial_actionable_count BETWEEN 0 AND 4294967295),
        requested_outcome TEXT NOT NULL CHECK(requested_outcome IN ('completed','needs_operator','no_action','incomplete')),
        accepted_outcome TEXT NOT NULL CHECK(accepted_outcome IN ('completed','needs_operator','no_action','incomplete'))
    ); CREATE INDEX IF NOT EXISTS queen_run_history_finished ON queen_run_history(finished_at,run_id);")?;
    tx.pragma_update(
        None,
        "user_version",
        crate::QUEEN_RUN_HISTORY_SCHEMA_VERSION,
    )
}

fn prune(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM queen_run_history WHERE finished_at < ?1",
        [now.saturating_sub(RETENTION_SECONDS)],
    )?;
    tx.execute("DELETE FROM queen_run_history WHERE run_id IN (
        SELECT run_id FROM queen_run_history ORDER BY finished_at DESC,run_id DESC LIMIT -1 OFFSET ?1)", [MAX_RETAINED])?;
    Ok(())
}

/// Only called after the exact live-run finish transition succeeded, in its transaction.
pub(super) fn record_finish(
    tx: &Transaction<'_>,
    requested: QueenAutomationOutcome,
    build: Option<&str>,
    now: i64,
) -> rusqlite::Result<()> {
    // Build metadata cannot upgrade unknown source data or break a valid finish.
    let build = build.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte))
    });
    tx.execute("INSERT INTO queen_run_history
        (run_id,finished_on_build,trigger,requested_at,delivered_at,finished_at,attempts,
         initial_actionable_count,requested_outcome,accepted_outcome)
        SELECT run_id,?1,trigger,requested_at,delivered_at,finished_at,attempts,actionable_count,?2,outcome
        FROM queen_automation WHERE id=1 AND state='completed'", params![build,requested.to_string()])?;
    prune(tx, now)
}

impl TaskStore {
    /// Private bounded finished-run history. Missing history is not zero activity.
    ///
    /// # Errors
    /// Returns invalid clocks, persistence failures, or invalid stored enum values.
    pub fn queen_run_history(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<QueenRunHistory, TaskStoreError> {
        if now < 0 {
            return Err(rusqlite::Error::InvalidQuery.into());
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        prune(&tx, now)?;
        let retained_count: u32 =
            tx.query_row("SELECT COUNT(*) FROM queen_run_history", [], |row| {
                row.get(0)
            })?;
        let records = {
            let mut query = tx.prepare(
                "SELECT run_id,finished_on_build,trigger,requested_at,delivered_at,
                finished_at,attempts,initial_actionable_count,requested_outcome,accepted_outcome
                FROM queen_run_history ORDER BY finished_at DESC,run_id DESC LIMIT ?1",
            )?;
            query
                .query_map([limit.clamp(1, 100)], |row| {
                    Ok(QueenRunEvidence {
                        run_id: row.get(0)?,
                        finished_on_build: row.get(1)?,
                        trigger: row
                            .get::<_, String>(2)?
                            .parse()
                            .map_err(|()| rusqlite::Error::InvalidQuery)?,
                        requested_at: row.get(3)?,
                        delivered_at: row.get(4)?,
                        finished_at: row.get(5)?,
                        attempts: row.get(6)?,
                        initial_actionable_count: row.get(7)?,
                        requested_outcome: crate::queen_conductor::parse_outcome(
                            &row.get::<_, String>(8)?,
                        )?,
                        accepted_outcome: crate::queen_conductor::parse_outcome(
                            &row.get::<_, String>(9)?,
                        )?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let review_returns = crate::review_return_history::read(&tx, now, limit)?;
        tx.commit()?;
        Ok(QueenRunHistory {
            retention_days: RETENTION_DAYS,
            max_retained: MAX_RETAINED,
            retained_count,
            records,
            review_returns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QueenAutomationFinish;
    use swarm_domain::WorkerSessionId;

    fn start(store: &TaskStore) -> String {
        let queen = store.ensure_queen("/fixture/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.set_queen_automation_enabled(true, 99).unwrap();
        store.request_queen_automation_run(100).unwrap();
        let run = store.claim_queen_automation(101).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&run.run_id, 102)
            .unwrap();
        run.run_id
    }

    #[test]
    fn accepted_finish_is_recorded_once_with_requested_and_normalized_outcomes() {
        let store = TaskStore::in_memory().unwrap();
        let run = start(&store);
        assert_eq!(
            store
                .finish_queen_automation_run_with_recovery(
                    &run,
                    QueenAutomationOutcome::NeedsOperator,
                    110,
                    &[],
                    true,
                    Some("1.6.0-dev-test")
                )
                .unwrap(),
            QueenAutomationFinish::Closed(QueenAutomationOutcome::NoAction)
        );
        let history = store.queen_run_history(111, 100).unwrap();
        assert_eq!(history.retained_count, 1);
        let record = &history.records[0];
        assert_eq!(record.run_id, run);
        assert_eq!(record.finished_on_build.as_deref(), Some("1.6.0-dev-test"));
        assert_eq!(
            record.requested_outcome,
            QueenAutomationOutcome::NeedsOperator
        );
        assert_eq!(record.accepted_outcome, QueenAutomationOutcome::NoAction);
        assert_eq!(record.delivery_wait_seconds(), Some(2));
        assert_eq!(record.delivered_to_finish_seconds(), Some(8));
        store
            .finish_queen_automation_run(&run, QueenAutomationOutcome::Completed, 112)
            .unwrap();
        store
            .finish_queen_automation_run("stale", QueenAutomationOutcome::Completed, 113)
            .unwrap();
        assert_eq!(store.queen_run_history(114, 100).unwrap(), history);
    }

    #[test]
    fn failed_history_write_rolls_back_the_finish_and_retry_recovers() {
        let store = TaskStore::in_memory().unwrap();
        let run = start(&store);
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_history BEFORE INSERT ON queen_run_history
            BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",
            )
            .unwrap();
        assert!(
            store
                .finish_queen_automation_run(&run, QueenAutomationOutcome::NoAction, 110)
                .is_err()
        );
        assert_eq!(
            store.queen_automation_status(110).unwrap().state,
            swarm_domain::QueenAutomationState::Running
        );
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_history")
            .unwrap();
        store
            .finish_queen_automation_run(&run, QueenAutomationOutcome::NoAction, 111)
            .unwrap();
        assert_eq!(store.queen_run_history(112, 100).unwrap().retained_count, 1);
    }

    #[test]
    fn reads_enforce_age_count_and_page_bounds() {
        let store = TaskStore::in_memory().unwrap();
        {
            let mut connection = store.connection().unwrap();
            let tx = connection.transaction().unwrap();
            for index in 0..4_110 {
                tx.execute("INSERT INTO queen_run_history VALUES(?1,NULL,'manual',NULL,NULL,?2,0,0,'no_action','no_action')",
                    params![format!("run-{index}"),index]).unwrap();
            }
            tx.commit().unwrap();
        }
        let history = store.queen_run_history(4_110, u32::MAX).unwrap();
        assert_eq!(history.retained_count, MAX_RETAINED);
        assert_eq!(history.records.len(), 100);
        assert_eq!(history.records[0].finished_at, 4_109);
        assert_eq!(history.records[0].delivery_wait_seconds(), None);
        assert!(store.queen_run_history(-1, 10).is_err());
        assert_eq!(
            store
                .queen_run_history(RETENTION_SECONDS + 4_110, 100)
                .unwrap()
                .retained_count,
            0
        );
    }

    #[test]
    fn upgrade_and_restart_preserve_workers_and_new_history() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hive.db");
        let run;
        {
            let store = TaskStore::open(&path).unwrap();
            run = start(&store);
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE queen_run_history; PRAGMA user_version=153;")
                .unwrap();
        }
        {
            let store = TaskStore::open(&path).unwrap();
            assert_eq!(
                store
                    .queen_automation_status(105)
                    .unwrap()
                    .run_id
                    .as_deref(),
                Some(run.as_str())
            );
            assert!(
                store
                    .queen_run_history(105, 100)
                    .unwrap()
                    .records
                    .is_empty()
            );
            store
                .finish_queen_automation_run(&run, QueenAutomationOutcome::NoAction, 110)
                .unwrap();
        }
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(
            store.queen_run_history(111, 100).unwrap().records[0].run_id,
            run
        );
    }
}
