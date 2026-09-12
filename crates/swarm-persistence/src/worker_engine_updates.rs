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

/// Makes room for updates NOBODY ASKED FOR, which are most of them.
///
/// ⚠️ SCHEMA 169 RECORDED ONLY THE PATH THAT ASKS PERMISSION. The endpoint an
/// operator clicks was its only writer — and a systemd timer replaces the engine
/// every two minutes whenever no session reports mid-turn, through the package
/// layer, touching none of this. The path that actually stops workers unprompted
/// was the one leaving no trace.
///
/// Two columns change to let an OBSERVED swap be recorded honestly:
///
/// - `initiated` says whether anyone asked. Existing rows are 'operator',
///   which is true: nothing else could write one.
/// - `stopped_sessions` becomes NULLABLE, because an observer learns the engine
///   moved and cannot know what it cost. Recording 0 would read as "it stopped
///   no workers", which is a claim, and the wrong one.
pub(crate) fn migrate_unprompted_updates(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE worker_engine_update_attempts_v170 (
             id TEXT PRIMARY KEY,
             started_at INTEGER NOT NULL CHECK (started_at >= 0),
             from_version TEXT NOT NULL,
             to_version TEXT NOT NULL,
             to_protocol INTEGER,
             -- NULL means nobody counted, which is what an observation knows.
             stopped_sessions INTEGER CHECK (stopped_sessions >= 0),
             outcome TEXT CHECK (outcome IN ('succeeded','timed_out','failed')),
             detail TEXT NOT NULL DEFAULT '',
             finished_at INTEGER CHECK (finished_at >= started_at),
             initiated TEXT NOT NULL DEFAULT 'operator'
                 CHECK (initiated IN ('operator','automatic'))
         );
         INSERT INTO worker_engine_update_attempts_v170 (
             id, started_at, from_version, to_version, to_protocol,
             stopped_sessions, outcome, detail, finished_at, initiated
         )
         SELECT id, started_at, from_version, to_version, to_protocol,
                stopped_sessions, outcome, detail, finished_at, 'operator'
           FROM worker_engine_update_attempts;
         DROP TABLE worker_engine_update_attempts;
         ALTER TABLE worker_engine_update_attempts_v170
             RENAME TO worker_engine_update_attempts;
         CREATE INDEX IF NOT EXISTS worker_engine_update_attempts_by_start
             ON worker_engine_update_attempts(started_at DESC, id DESC);",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        super::UNPROMPTED_ENGINE_UPDATE_SCHEMA_VERSION,
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

/// Whether anybody asked for this update.
///
/// ⚠️ THE DISTINCTION THE OPERATOR RULED ON. Most engine replacements on a Hive
/// are `Automatic`: a systemd timer swaps the engine whenever no session reports
/// mid-turn, stopping loaded workers without anyone deciding to. Operator
/// decision 01a092cd, 2026-09-11: keep that automatic, but say so when it
/// happens. A record that cannot tell the two apart cannot say it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerEngineUpdateInitiator {
    /// Somebody clicked the maintenance action and was told what it would cost.
    Operator,
    /// Swarm replaced the engine on its own. Nobody was asked and nobody chose
    /// the moment.
    Automatic,
}

impl WorkerEngineUpdateInitiator {
    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Automatic => "automatic",
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
    /// `None` means nobody counted. An observed swap learns that the engine
    /// moved and cannot know what it cost; 0 would say it cost nothing.
    pub stopped_sessions: Option<i64>,
    pub initiated: WorkerEngineUpdateInitiator,
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
        stopped_sessions: Option<usize>,
        initiated: WorkerEngineUpdateInitiator,
        now: i64,
    ) -> Result<String, TaskStoreError> {
        let id = Uuid::now_v7().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO worker_engine_update_attempts (
                 id, started_at, from_version, to_version, to_protocol,
                 stopped_sessions, initiated
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                now,
                from_version,
                to_version,
                to_protocol,
                stopped_sessions.map(|count| i64::try_from(count).unwrap_or(i64::MAX)),
                initiated.as_str()
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
                    stopped_sessions, outcome, detail, finished_at, initiated
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
                    // An unreadable initiator reads as automatic: "nobody asked"
                    // is the answer that prompts a look, and the other way round
                    // would quietly attribute an unprompted swap to the operator.
                    initiated: if row.get::<_, String>(9)?.as_str() == "operator" {
                        WorkerEngineUpdateInitiator::Operator
                    } else {
                        WorkerEngineUpdateInitiator::Automatic
                    },
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
    use super::{KEPT_ATTEMPTS, WorkerEngineUpdateInitiator, WorkerEngineUpdateOutcome};
    use crate::TaskStore;

    #[test]
    fn an_attempt_that_never_reported_an_ending_reads_as_unknown() {
        // ⚠️ THE CASE THIS TABLE EXISTS FOR. A protocol migration replaces the
        // API process, so the code that would record the outcome can be gone
        // before it runs. That must present as "we do not know", never as
        // silence and never as success.
        let store = TaskStore::in_memory().unwrap();
        store
            .begin_worker_engine_update(
                "1.8.1",
                "1.9.0",
                Some(18),
                Some(4),
                WorkerEngineUpdateInitiator::Operator,
                1_000,
            )
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();

        assert_eq!(attempt.outcome, None);
        assert_eq!(attempt.finished_at, None);
        assert_eq!(attempt.stopped_sessions, Some(4));
        assert_eq!(attempt.to_protocol, Some(18));
        assert_eq!(attempt.from_version, "1.8.1");
    }

    #[test]
    fn an_ending_is_recorded_once_and_is_not_rewritten_afterwards() {
        let store = TaskStore::in_memory().unwrap();
        let id = store
            .begin_worker_engine_update(
                "1.8.1",
                "1.9.0",
                None,
                Some(2),
                WorkerEngineUpdateInitiator::Operator,
                1_000,
            )
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
            .begin_worker_engine_update(
                "1.8.1",
                "1.9.0",
                None,
                Some(0),
                WorkerEngineUpdateInitiator::Operator,
                5_000,
            )
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
                    Some(0),
                    WorkerEngineUpdateInitiator::Operator,
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
            .begin_worker_engine_update(
                "1.8.1",
                "1.9.0",
                None,
                Some(0),
                WorkerEngineUpdateInitiator::Operator,
                1_000,
            )
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
    fn an_update_nobody_asked_for_says_so_and_claims_no_worker_count() {
        // ⚠️ THE CASE SCHEMA 169 COULD NOT RECORD. A timer replaces the engine
        // whenever no session reports mid-turn; an observer learns afterwards
        // that it moved and never learns what it cost. Recording 0 there would
        // say "it stopped no workers", which is a claim rather than a silence.
        let store = TaskStore::in_memory().unwrap();
        store
            .begin_worker_engine_update(
                "1.8.1",
                "1.9.0",
                None,
                None,
                WorkerEngineUpdateInitiator::Automatic,
                1_000,
            )
            .unwrap();

        let attempt = store.last_worker_engine_update().unwrap().unwrap();

        assert_eq!(attempt.initiated, WorkerEngineUpdateInitiator::Automatic);
        assert_eq!(attempt.stopped_sessions, None);
    }

    #[test]
    fn an_attempt_recorded_before_the_distinction_existed_is_the_operator_s() {
        // Nothing but the maintenance endpoint could write a row before schema
        // 170, so backfilling those as 'operator' states a fact rather than a
        // guess. The reverse would accuse Swarm of swaps a person made.
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        connection
            .execute(
                "INSERT INTO worker_engine_update_attempts
                     (id, started_at, from_version, to_version)
                 VALUES ('legacy', 900, '1.7.0', '1.8.0')",
                [],
            )
            .unwrap();
        drop(connection);

        let attempt = store.last_worker_engine_update().unwrap().unwrap();
        assert_eq!(attempt.initiated, WorkerEngineUpdateInitiator::Operator);
    }

    #[test]
    fn a_hive_that_has_never_updated_says_so_rather_than_inventing_one() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(store.last_worker_engine_update().unwrap(), None);
    }
}
