//! What each workspace has actually spent with its provider, per day.
//!
//! WHY THIS IS STORED RATHER THAN COMPUTED ON DEMAND. The numbers live in the
//! provider's own transcripts -- `~/.claude/projects/<slug>/*.jsonl`, one
//! `usage` block per assistant message -- and on this Hive that corpus is 5.1GB
//! across 890 files. A full pass takes minutes. Nobody opens a settings panel
//! that takes minutes, and a panel that rescans on every render would make the
//! measurement more expensive than the thing it measures.
//!
//! So the scan is incremental and its results are durable. Each file is
//! remembered by the byte offset already read; a later pass reads only what was
//! appended. The first pass pays for the history once.
//!
//! ⚠️ DEDUPLICATION IS NOT AN OPTIMISATION HERE, IT IS CORRECTNESS. Measured on
//! this Hive: 135,779 of 283,132 usage lines in a 30-day window were replays of
//! a message already counted -- 48%. Resumes, sidechains and compaction all
//! rewrite entries that were already on disk. Summing lines does not produce a
//! number that is slightly too high; it roughly doubles it, and it inflates
//! long-running workers most, which is exactly the population a "who is
//! spending" panel exists to compare. `provider_usage_seen` keys every message
//! by its own id so a replay is counted once no matter which file or which pass
//! it arrives in.
//!
//! WHAT THIS IS NOT. It is not a bill. Anthropic's invoice is the authority on
//! money, subscription usage is metered on their side, and no price is recorded
//! here -- only token counts, kept apart by kind so a reader can weigh them
//! however they need to.

use rusqlite::{OptionalExtension, Transaction, params};

use super::{TaskStore, TaskStoreError};

/// One workspace's spend on one day with one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderUsageDay {
    pub workspace: String,
    pub day: String,
    pub model: String,
    pub input_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    pub messages: i64,
}

/// One transcript's read position, so a later pass reads only what was appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderUsageCursor {
    /// Bytes already consumed. A file SHORTER than this was rewritten rather
    /// than appended to, and the caller rescans it from zero.
    pub offset: i64,
}

/// A batch of counted messages, ready to fold into the daily totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedMessage {
    pub message_id: String,
    pub workspace: String,
    pub day: String,
    pub model: String,
    pub input_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
}

pub(super) fn migrate_provider_usage(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS provider_usage_daily (
             workspace TEXT NOT NULL,
             day TEXT NOT NULL,
             model TEXT NOT NULL,
             input_tokens INTEGER NOT NULL DEFAULT 0,
             cache_write_tokens INTEGER NOT NULL DEFAULT 0,
             cache_read_tokens INTEGER NOT NULL DEFAULT 0,
             output_tokens INTEGER NOT NULL DEFAULT 0,
             messages INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (workspace, day, model)
         );
         CREATE INDEX IF NOT EXISTS provider_usage_daily_day
             ON provider_usage_daily(day);
         -- One row per counted message. This is the whole defence against the
         -- 48% replay rate; without it the daily totals are roughly double.
         CREATE TABLE IF NOT EXISTS provider_usage_seen (
             message_id TEXT PRIMARY KEY,
             day TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS provider_usage_seen_day
             ON provider_usage_seen(day);
         CREATE TABLE IF NOT EXISTS provider_usage_cursor (
             path TEXT PRIMARY KEY,
             offset INTEGER NOT NULL,
             scanned_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS provider_usage_scan (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             last_completed_at INTEGER
         );",
    )?;
    // ⚠️ EVERY MIGRATION STAMPS ITS OWN CEILING, and this is the last step, so
    // this is the number a database ends up carrying. Omitting it does not fail
    // loudly: the tables get created, everything works on THIS machine, and a
    // freshly opened database reports the previous version -- so the step runs
    // again on every open, and any later step added after it is skipped. The
    // reload-backup guard caught exactly that here, comparing the version it
    // reads from this source against the one a fresh database reports.
    tx.pragma_update(None, "user_version", crate::PROVIDER_USAGE_SCHEMA_VERSION)
}

impl TaskStore {
    /// Where a previous pass stopped reading this transcript.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read.
    pub fn provider_usage_cursor(
        &self,
        path: &str,
    ) -> Result<Option<ProviderUsageCursor>, TaskStoreError> {
        let connection = self.connection()?;
        let offset: Option<i64> = connection
            .query_row(
                "SELECT offset FROM provider_usage_cursor WHERE path = ?1",
                [path],
                |row| row.get(0),
            )
            .optional()?;
        Ok(offset.map(|offset| ProviderUsageCursor { offset }))
    }

    /// Folds one transcript's newly read messages into the daily totals and
    /// advances its cursor, in a single transaction.
    ///
    /// Counting and advancing MUST commit together. Advancing first and then
    /// failing loses that span of the file forever -- silently, because the
    /// cursor says it was read. Counting first and then failing double-counts
    /// on the next pass for every message the seen-set has since been pruned
    /// of. One transaction makes both impossible.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or written.
    pub fn record_provider_usage(
        &self,
        path: &str,
        offset: i64,
        scanned_at: i64,
        messages: &[CountedMessage],
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut counted = 0usize;
        {
            let mut mark = transaction.prepare(
                "INSERT OR IGNORE INTO provider_usage_seen (message_id, day) VALUES (?1, ?2)",
            )?;
            let mut fold = transaction.prepare(
                "INSERT INTO provider_usage_daily
                     (workspace, day, model, input_tokens, cache_write_tokens,
                      cache_read_tokens, output_tokens, messages)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
                 ON CONFLICT(workspace, day, model) DO UPDATE SET
                     input_tokens = input_tokens + excluded.input_tokens,
                     cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
                     cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                     output_tokens = output_tokens + excluded.output_tokens,
                     messages = messages + 1",
            )?;
            for message in messages {
                // INSERT OR IGNORE returns 0 when the id is already present,
                // which is the deduplication. A message only reaches the totals
                // on the pass that first saw it.
                let fresh = mark.execute(params![message.message_id, message.day])?;
                if fresh == 0 {
                    continue;
                }
                fold.execute(params![
                    message.workspace,
                    message.day,
                    message.model,
                    message.input_tokens,
                    message.cache_write_tokens,
                    message.cache_read_tokens,
                    message.output_tokens,
                ])?;
                counted += 1;
            }
        }
        transaction.execute(
            "INSERT INTO provider_usage_cursor (path, offset, scanned_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET offset = excluded.offset,
                                             scanned_at = excluded.scanned_at",
            params![path, offset, scanned_at],
        )?;
        transaction.commit()?;
        Ok(counted)
    }

    /// Daily totals from `earliest_day` onward, newest last.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read.
    pub fn provider_usage_since(
        &self,
        earliest_day: &str,
    ) -> Result<Vec<ProviderUsageDay>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT workspace, day, model, input_tokens, cache_write_tokens,
                    cache_read_tokens, output_tokens, messages
             FROM provider_usage_daily
             WHERE day >= ?1
             ORDER BY day, workspace, model",
        )?;
        let rows = statement
            .query_map([earliest_day], |row| {
                Ok(ProviderUsageDay {
                    workspace: row.get(0)?,
                    day: row.get(1)?,
                    model: row.get(2)?,
                    input_tokens: row.get(3)?,
                    cache_write_tokens: row.get(4)?,
                    cache_read_tokens: row.get(5)?,
                    output_tokens: row.get(6)?,
                    messages: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// When the last full pass finished, if one ever has.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read.
    pub fn provider_usage_last_scan(&self) -> Result<Option<i64>, TaskStoreError> {
        let connection = self.connection()?;
        let at: Option<Option<i64>> = connection
            .query_row(
                "SELECT last_completed_at FROM provider_usage_scan WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(at.flatten())
    }

    /// Records that a full pass finished, and drops what has aged out.
    ///
    /// Pruning is here rather than on a timer because this is the only moment
    /// the totals are known to be complete. `retain_days` bounds BOTH tables:
    /// the daily rows a reader can ask for, and the seen-set that keeps replays
    /// from being recounted. They are pruned at the same boundary on purpose --
    /// a seen-set pruned ahead of the totals would let an old replay land on a
    /// day whose total is still being shown.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or written.
    pub fn finish_provider_usage_scan(
        &self,
        completed_at: i64,
        earliest_day_to_keep: &str,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM provider_usage_daily WHERE day < ?1",
            [earliest_day_to_keep],
        )?;
        transaction.execute(
            "DELETE FROM provider_usage_seen WHERE day < ?1",
            [earliest_day_to_keep],
        )?;
        transaction.execute(
            "INSERT INTO provider_usage_scan (id, last_completed_at) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET last_completed_at = excluded.last_completed_at",
            [completed_at],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> TaskStore {
        TaskStore::in_memory().expect("in-memory store")
    }

    fn message(id: &str, day: &str, read: i64) -> CountedMessage {
        CountedMessage {
            message_id: id.to_owned(),
            workspace: "/w".to_owned(),
            day: day.to_owned(),
            model: "claude-opus-5".to_owned(),
            input_tokens: 1,
            cache_write_tokens: 2,
            cache_read_tokens: read,
            output_tokens: 4,
        }
    }

    /// ⚠️ THE ONE THAT MATTERS. 48% of the usage lines on the Hive this was
    /// built for are replays of a message already on disk. If a replay is
    /// counted again the panel does not read slightly high -- it reads roughly
    /// double, and worst for the longest-running workers, which is precisely
    /// the comparison it exists to make.
    #[test]
    fn a_replayed_message_is_counted_once_however_often_it_arrives() {
        let store = store();
        let first = store
            .record_provider_usage("/t.jsonl", 100, 10, &[message("m1", "2026-09-14", 10)])
            .expect("first pass");
        // The same id again, in a later pass, from a longer file -- which is
        // exactly what a resume writes.
        let second = store
            .record_provider_usage(
                "/t.jsonl",
                200,
                20,
                &[
                    message("m1", "2026-09-14", 10),
                    message("m2", "2026-09-14", 30),
                ],
            )
            .expect("second pass");

        assert_eq!(first, 1, "first sighting counts");
        assert_eq!(second, 1, "only the new message counts");
        let days = store.provider_usage_since("2026-09-01").expect("read back");
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].messages, 2, "two distinct messages, not three");
        assert_eq!(
            days[0].cache_read_tokens, 40,
            "10 + 30, the replay excluded"
        );
    }

    #[test]
    fn a_cursor_remembers_where_a_transcript_was_left() {
        let store = store();
        assert_eq!(
            store.provider_usage_cursor("/t.jsonl").expect("absent"),
            None,
            "a transcript never read has no cursor, which is different from one read to zero"
        );
        store
            .record_provider_usage("/t.jsonl", 4096, 10, &[])
            .expect("record");
        assert_eq!(
            store.provider_usage_cursor("/t.jsonl").expect("present"),
            Some(ProviderUsageCursor { offset: 4096 }),
        );
    }

    /// Counting and advancing commit together, so a failure cannot leave the
    /// cursor past messages that were never folded in.
    #[test]
    fn an_empty_batch_still_advances_the_cursor() {
        let store = store();
        store
            .record_provider_usage("/t.jsonl", 512, 10, &[])
            .expect("record");
        let days = store.provider_usage_since("2026-09-01").expect("read back");
        assert!(days.is_empty(), "nothing counted");
        assert_eq!(
            store.provider_usage_cursor("/t.jsonl").expect("present"),
            Some(ProviderUsageCursor { offset: 512 }),
            "a span with no assistant messages in it is still read"
        );
    }

    #[test]
    fn totals_separate_the_token_kinds_rather_than_summing_them() {
        let store = store();
        store
            .record_provider_usage("/t.jsonl", 10, 10, &[message("m1", "2026-09-14", 500)])
            .expect("record");
        let days = store.provider_usage_since("2026-09-01").expect("read back");
        // Kept apart because they are billed at different rates and answer
        // different questions: a high cache_read share is cheap burn, while
        // cache_write rivalling it is a cache being rebuilt rather than reused.
        assert_eq!(days[0].input_tokens, 1);
        assert_eq!(days[0].cache_write_tokens, 2);
        assert_eq!(days[0].cache_read_tokens, 500);
        assert_eq!(days[0].output_tokens, 4);
    }

    #[test]
    fn finishing_a_scan_prunes_both_tables_at_the_same_boundary() {
        let store = store();
        store
            .record_provider_usage("/a.jsonl", 10, 10, &[message("old", "2026-08-01", 1)])
            .expect("old");
        store
            .record_provider_usage("/b.jsonl", 10, 10, &[message("new", "2026-09-14", 1)])
            .expect("new");

        store
            .finish_provider_usage_scan(1_789_000_000, "2026-09-01")
            .expect("finish");

        let days = store.provider_usage_since("2020-01-01").expect("read back");
        assert_eq!(days.len(), 1, "the aged-out day is gone");
        assert_eq!(days[0].day, "2026-09-14");
        assert_eq!(
            store.provider_usage_last_scan().expect("last scan"),
            Some(1_789_000_000),
        );
        // The seen-set was pruned to the SAME boundary, so an old id is
        // countable again -- which is harmless only because its day is gone
        // too. Were the seen-set pruned further ahead, a replay could land on a
        // day still on screen and inflate it.
        let recounted = store
            .record_provider_usage("/a.jsonl", 20, 20, &[message("old", "2026-08-01", 1)])
            .expect("recount");
        assert_eq!(recounted, 1);
        let days = store.provider_usage_since("2026-09-01").expect("read back");
        assert_eq!(days.len(), 1, "and it still cannot reach a day being shown");
    }
}
