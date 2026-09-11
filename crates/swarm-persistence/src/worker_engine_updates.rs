//! What happened the last time the worker engine was replaced.
//!
//! ⚠️ AN ENGINE UPDATE USED TO LEAVE NO TRACE ON THE BOARD. It stops every
//! loaded worker, swaps the terminal host, and waits for the new one to report
//! in -- and the only record of whether that worked was the journal. An operator
//! asking "did the update go through, and did it take my workers with it" had
//! nowhere to look but logs on the machine.
//!
//! THE ROW IS WRITTEN BEFORE ANYTHING IS STOPPED, and its outcome afterwards.
//! That ordering is the whole design: a protocol migration replaces the API
//! process itself, so the code that would write "succeeded" may not survive to
//! write anything. An attempt with no outcome is therefore not a bug in this
//! table -- it is the most important thing it can say, and it can only say it
//! because the row went in first.

use rusqlite::params;
use uuid::Uuid;

use super::{TaskStore, TaskStoreError};

/// How many attempts are kept.
///
/// This is a history for a person, not an audit log. Twenty reaches back
/// further than anybody has ever needed while keeping the table a rounding
/// error, and an unbounded one would grow forever on a Hive that updates often.
const KEPT_ATTEMPTS: i64 = 20;

/// Bounds the one free-text field, which carries an error message from
/// elsewhere and must not become a place arbitrary output accumulates.
const MAX_DETAIL_BYTES: usize = 500;

pub(crate) fn migrate(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_engine_update_attempts (
             id TEXT PRIMARY KEY,
             started_at INTEGER NOT NULL CHECK (started_at >= 0),
             from_version TEXT NOT NULL,
             to_version TEXT NOT NULL,
             -- Present only for a protocol migration, which is the one update
             -- that cannot preserve running terminals.
             to_protocol INTEGER,
             stopped_sessions INTEGER NOT NULL DEFAULT 0 CHECK (stopped_sessions >= 0),
             -- NULL is a state, not a gap: started and never heard from again.
             outcome TEXT CHECK (outcome IN ('succeeded','timed_out','failed')),
             detail TEXT NOT NULL DEFAULT '',
             finished_at INTEGER CHECK (finished_at >= started_at)
         );
         CREATE INDEX IF NOT EXISTS worker_engine_update_attempts_by_start
             ON worker_engine_update_attempts(started_at DESC, id DESC);",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        super::WORKER_ENGINE_UPDATE_HISTORY_SCHEMA_VERSION,
    )
}

/// How an engine update ended, or that nobody ever found out.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerEngineUpdateOutcome {
    Succeeded,
    /// The engine never reported the expected release inside the budget. The
    /// workers it unloaded are still owed a return.
    TimedOut,
    /// It could not be carried out; `detail` says what refused.
    Failed,
}

impl WorkerEngineUpdateOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::TimedOut => "timed_out",
            Self::Failed => "failed",
        }
    }
}

/// One attempt, as recorded.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct WorkerEngineUpdateAttempt {
    pub id: String,
    pub started_at: i64,
    pub from_version: String,
    pub to_version: String,
    pub to_protocol: Option<u16>,
    pub stopped_sessions: i64,
    /// `None` means this attempt never recorded an ending. Read it as unknown,
    /// never as success -- a Hive that was replaced mid-update leaves exactly
    /// this, and it is the case an operator most needs to see.
    pub outcome: Option<WorkerEngineUpdateOutcome>,
    pub detail: String,
    pub finished_at: Option<i64>,
}

impl TaskStore {
    /// Records that an engine update is about to stop workers.
    ///
    /// CALL THIS BEFORE STOPPING ANYTHING. Afterwards is too late: the update
    /// may replace the process that would have made the call.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn begin_worker_engine_update(
        &self,
        from_version: &str,
        to_version: &str,
        to_protocol: Option<u16>,
        stopped_sessions: usize,
        now: i64,
    ) -> Result<String, TaskStoreError> {
        let id = Uuid::now_v7().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO worker_engine_update_attempts (
                 id, started_at, from_version, to_version, to_protocol, stopped_sessions
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                now,
                from_version,
                to_version,
                to_protocol,
                i64::try_from(stopped_sessions).unwrap_or(i64::MAX)
            ],
        )?;
        // Pruned by START ORDER, which is also insert order, so an attempt that
        // never finished ages out like any other rather than being kept forever
        // for having no ending.
        connection.execute(
            "DELETE FROM worker_engine_update_attempts
              WHERE id NOT IN (
                  SELECT id FROM worker_engine_update_attempts
                   ORDER BY started_at DESC, id DESC LIMIT ?1
              )",
            [KEPT_ATTEMPTS],
        )?;
        Ok(id)
    }

    /// Records how an attempt ended.
    ///
    /// FIRST ENDING WINS. An attempt has one outcome, and a later call saying
    /// something different is a caller reporting the same event twice rather
    /// than new information -- rewriting "timed out" as "succeeded" because a
    /// retry path ran would erase the fact somebody needs.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn finish_worker_engine_update(
        &self,
        id: &str,
        outcome: WorkerEngineUpdateOutcome,
        detail: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let detail = detail.trim();
        let detail = if detail.len() > MAX_DETAIL_BYTES {
            let mut cut = MAX_DETAIL_BYTES;
            while cut > 0 && !detail.is_char_boundary(cut) {
                cut -= 1;
            }
            &detail[..cut]
        } else {
            detail
        };
        let connection = self.connection()?;
        connection.execute(
            "UPDATE worker_engine_update_attempts
                SET outcome = ?2, detail = ?3, finished_at = max(?4, started_at)
              WHERE id = ?1 AND outcome IS NULL",
            params![id, outcome.as_str(), detail, now],
        )?;
        Ok(())
    }

    /// The most recent attempts, newest first.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn recent_worker_engine_updates(
        &self,
        limit: usize,
    ) -> Result<Vec<WorkerEngineUpdateAttempt>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, started_at, from_version, to_version, to_protocol,
                    stopped_sessions, outcome, detail, finished_at
               FROM worker_engine_update_attempts
              ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        Ok(statement
            .query_map([i64::try_from(limit).unwrap_or(KEPT_ATTEMPTS)], |row| {
                Ok(WorkerEngineUpdateAttempt {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    from_version: row.get(2)?,
                    to_version: row.get(3)?,
                    to_protocol: row
                        .get::<_, Option<i64>>(4)?
                        .and_then(|value| u16::try_from(value).ok()),
                    stopped_sessions: row.get(5)?,
                    outcome: match row.get::<_, Option<String>>(6)?.as_deref() {
                        Some("succeeded") => Some(WorkerEngineUpdateOutcome::Succeeded),
                        Some("timed_out") => Some(WorkerEngineUpdateOutcome::TimedOut),
                        Some("failed") => Some(WorkerEngineUpdateOutcome::Failed),
                        // An unreadable outcome is unknown, not a success.
                        _ => None,
                    },
                    detail: row.get(7)?,
                    finished_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// The newest attempt, if this Hive has ever tried.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn last_worker_engine_update(
        &self,
    ) -> Result<Option<WorkerEngineUpdateAttempt>, TaskStoreError> {
        Ok(self.recent_worker_engine_updates(1)?.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::{KEPT_ATTEMPTS, WorkerEngineUpdateOutcome};
    use crate::TaskStore;

    #[test]
    fn an_attempt_that_never_reported_an_ending_reads_as_unknown() {
        // ⚠️ THE CASE THIS TABLE EXISTS FOR. A protocol migration replaces the
        // API process, so the code that would record the outcome can be gone
        // before it runs. That must present as "we do not know", never as
        // silence and never as success.
        let store = TaskStore::in_memory().unwrap();
        store
            .begin_worker_engine_update("1.8.1", "1.9.0", Some(18), 4, 1_000)
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();

        assert_eq!(attempt.outcome, None);
        assert_eq!(attempt.finished_at, None);
        assert_eq!(attempt.stopped_sessions, 4);
        assert_eq!(attempt.to_protocol, Some(18));
        assert_eq!(attempt.from_version, "1.8.1");
    }

    #[test]
    fn an_ending_is_recorded_once_and_is_not_rewritten_afterwards() {
        let store = TaskStore::in_memory().unwrap();
        let id = store
            .begin_worker_engine_update("1.8.1", "1.9.0", None, 2, 1_000)
            .unwrap();
        store
            .finish_worker_engine_update(
                &id,
                WorkerEngineUpdateOutcome::TimedOut,
                "the engine has not reported the expected release",
                1_200,
            )
            .unwrap();

        // A retry path reporting the same event again must not turn a timeout
        // into a success; the workers were still stopped and still owed.
        store
            .finish_worker_engine_update(&id, WorkerEngineUpdateOutcome::Succeeded, "", 1_300)
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();
        assert_eq!(attempt.outcome, Some(WorkerEngineUpdateOutcome::TimedOut));
        assert_eq!(attempt.finished_at, Some(1_200));
        assert!(attempt.detail.contains("expected release"));
    }

    #[test]
    fn an_ending_stamped_before_its_start_does_not_travel_backwards() {
        // A clock that moved between the two calls must not produce a row
        // claiming the update finished before it began; the CHECK would reject
        // it and the ending would be lost entirely.
        let store = TaskStore::in_memory().unwrap();
        let id = store
            .begin_worker_engine_update("1.8.1", "1.9.0", None, 0, 5_000)
            .unwrap();
        store
            .finish_worker_engine_update(&id, WorkerEngineUpdateOutcome::Succeeded, "", 4_000)
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();
        assert_eq!(attempt.outcome, Some(WorkerEngineUpdateOutcome::Succeeded));
        assert_eq!(attempt.finished_at, Some(5_000));
    }

    #[test]
    fn the_history_stays_bounded_and_keeps_the_newest() {
        let store = TaskStore::in_memory().unwrap();
        for index in 0..(KEPT_ATTEMPTS + 5) {
            store
                .begin_worker_engine_update(
                    "1.8.1",
                    &format!("1.9.{index}"),
                    None,
                    0,
                    1_000 + index,
                )
                .unwrap();
        }

        let kept = store.recent_worker_engine_updates(100).unwrap();

        assert_eq!(i64::try_from(kept.len()).unwrap(), KEPT_ATTEMPTS);
        assert_eq!(kept[0].to_version, format!("1.9.{}", KEPT_ATTEMPTS + 4));
        assert_eq!(kept[kept.len() - 1].to_version, "1.9.5");
    }

    #[test]
    fn a_detail_longer_than_the_field_is_cut_rather_than_refused() {
        // The detail carries somebody else's error message. Refusing an
        // over-long one would lose the ending for the sake of the explanation.
        let store = TaskStore::in_memory().unwrap();
        let id = store
            .begin_worker_engine_update("1.8.1", "1.9.0", None, 0, 1_000)
            .unwrap();
        store
            .finish_worker_engine_update(
                &id,
                WorkerEngineUpdateOutcome::Failed,
                &"unreadable ".repeat(500),
                1_100,
            )
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();
        assert_eq!(attempt.outcome, Some(WorkerEngineUpdateOutcome::Failed));
        assert!(attempt.detail.len() <= super::MAX_DETAIL_BYTES);
        assert!(attempt.detail.starts_with("unreadable"));
    }

    #[test]
    fn a_hive_that_has_never_updated_says_so_rather_than_inventing_one() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(store.last_worker_engine_update().unwrap(), None);
    }
}
