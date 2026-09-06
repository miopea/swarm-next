//! Isolated central support storage. Never open the execution Hive database here.
use std::{num::NonZeroU32, path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use swarm_domain::SupportSubmission;
use thiserror::Error;
use uuid::Uuid;

const APPLICATION_ID: i64 = 0x5357_5350;

mod reads;
pub use reads::{SupportConversationRecord, SupportListRecords, SupportThreadRecords};

#[derive(Debug, Error)]
pub enum SupportStoreError {
    #[error("support conversation not found")]
    NotFound,
    #[error("support cursor is invalid")]
    InvalidCursor,
    #[error("support history exceeds its explicit read limit")]
    HistoryCapacity,
    #[error("database is not a supported central support database")]
    WrongDatabase,
    #[error("submission key already belongs to different content")]
    Conflict,
    #[error("support intake capacity reached; existing conversations are retained")]
    Capacity,
    #[error("support database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("support payload encoding failed")]
    Encoding(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SupportReceipt {
    pub submission_key: String,
    pub conversation_id: String,
    pub message_id: String,
    pub created_at: i64,
    pub deduplicated: bool,
}

pub struct SupportStore {
    connection: Connection,
    capacity: NonZeroU32,
}

impl SupportStore {
    /// Opens only a new database or this support schema, never a Hive database.
    ///
    /// # Errors
    /// Refuses foreign/newer databases and propagates storage errors.
    pub fn open(path: &Path, capacity: NonZeroU32) -> Result<Self, SupportStoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let application_id: i64 =
            transaction.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let objects: i64 = transaction.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'",
            [],
            |r| r.get(0),
        )?;
        if application_id == 0 && version == 0 && objects == 0 {
            transaction.execute_batch(
                "CREATE TABLE support_conversations (
                    id TEXT PRIMARY KEY NOT NULL,
                    submission_key TEXT NOT NULL UNIQUE,
                    initial_message_id TEXT NOT NULL UNIQUE,
                    frozen_submission TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE support_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    conversation_id TEXT NOT NULL REFERENCES support_conversations(id),
                    body TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE INDEX support_message_conversation ON support_messages(conversation_id);
                PRAGMA application_id = 1398231888;
                PRAGMA user_version = 1;",
            )?;
        } else if application_id != APPLICATION_ID || !(1..=2).contains(&version) {
            return Err(SupportStoreError::WrongDatabase);
        }
        if version < 2 {
            transaction.execute_batch(
                "CREATE TABLE support_health_probe (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    value INTEGER NOT NULL CHECK (value IN (0, 1))
                );
                INSERT INTO support_health_probe(id, value) VALUES (1, 0);
                PRAGMA user_version = 2;",
            )?;
        }
        transaction.commit()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self {
            connection,
            capacity,
        })
    }

    /// Checks durable read/write availability without reading or changing customer content.
    /// One fixed probe row bounds storage; successful commit is required.
    ///
    /// # Errors
    /// Reports read-only, unavailable, malformed or failed-commit storage honestly.
    pub fn check_health(&mut self) -> Result<(), SupportStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let _: i64 = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM support_conversations) + EXISTS(SELECT 1 FROM support_messages)",
            [], |row| row.get(0),
        )?;
        if transaction.execute(
            "UPDATE support_health_probe SET value = 1 - value WHERE id = 1",
            [],
        )? != 1
        {
            return Err(rusqlite::Error::InvalidQuery.into());
        }
        transaction.commit()?;
        Ok(())
    }

    /// Atomically saves a reviewed report and its initial conversation message.
    /// Exact retry is recoverable even when admission is full.
    ///
    /// # Errors
    /// Refuses changed retries or capacity overflow without partial writes.
    pub fn submit(
        &mut self,
        submission: &SupportSubmission,
        created_at: i64,
    ) -> Result<SupportReceipt, SupportStoreError> {
        let frozen = serde_json::to_string(submission)?;
        let key = submission.submission_key().to_string();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = transaction.query_row(
            "SELECT c.frozen_submission, c.id, m.id, c.created_at
             FROM support_conversations c JOIN support_messages m ON m.id = c.initial_message_id AND m.conversation_id = c.id
             WHERE c.submission_key = ?1", [&key], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
            },
        ).optional()?;
        if let Some((original, conversation_id, message_id, original_time)) = existing {
            if original != frozen {
                return Err(SupportStoreError::Conflict);
            }
            return Ok(SupportReceipt {
                submission_key: key,
                conversation_id,
                message_id,
                created_at: original_time,
                deduplicated: true,
            });
        }
        let count: i64 =
            transaction.query_row("SELECT count(*) FROM support_conversations", [], |r| {
                r.get(0)
            })?;
        if count >= i64::from(self.capacity.get()) {
            return Err(SupportStoreError::Capacity);
        }
        let conversation_id = Uuid::now_v7().to_string();
        let message_id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT INTO support_conversations (id, submission_key, frozen_submission, created_at, initial_message_id)
             VALUES (?1, ?2, ?3, ?4, ?5)", params![conversation_id, key, frozen, created_at, message_id],
        )?;
        transaction.execute(
            "INSERT INTO support_messages (id, conversation_id, body, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![message_id, conversation_id, submission.body(), created_at],
        )?;
        transaction.commit()?;
        Ok(SupportReceipt {
            submission_key: key,
            conversation_id,
            message_id,
            created_at,
            deduplicated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{SupportKind, SupportSubmissionInput};

    #[test]
    fn health_probe_is_bounded_and_refuses_read_only_storage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(1).unwrap();
        let mut store = SupportStore::open(&path, capacity).unwrap();
        let original = store
            .submit(&report(1, "Private fictional body"), 10)
            .unwrap();
        for _ in 0..5 {
            store.check_health().unwrap();
        }
        let count: i64 = store
            .connection
            .query_row("SELECT count(*) FROM support_health_probe", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        store
            .connection
            .pragma_update(None, "query_only", "ON")
            .unwrap();
        assert!(store.check_health().is_err());
        store
            .connection
            .pragma_update(None, "query_only", "OFF")
            .unwrap();
        store.check_health().unwrap();
        drop(store);
        let replay = SupportStore::open(&path, capacity)
            .unwrap()
            .submit(&report(1, "Private fictional body"), 20)
            .unwrap();
        assert_eq!(replay.conversation_id, original.conversation_id);
        assert!(replay.deduplicated);
    }

    #[test]
    fn version_one_support_upgrade_preserves_frozen_retries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(1).unwrap();
        let mut store = SupportStore::open(&path, capacity).unwrap();
        let original = store.submit(&report(1, "Original"), 10).unwrap();
        store
            .connection
            .execute_batch("DROP TABLE support_health_probe; PRAGMA user_version = 1;")
            .unwrap();
        drop(store);
        let mut upgraded = SupportStore::open(&path, capacity).unwrap();
        upgraded.check_health().unwrap();
        assert_eq!(
            upgraded
                .submit(&report(1, "Original"), 20)
                .unwrap()
                .message_id,
            original.message_id
        );
        assert!(matches!(
            upgraded.submit(&report(1, "Changed"), 20),
            Err(SupportStoreError::Conflict)
        ));
    }

    fn report(key: u128, body: &str) -> SupportSubmission {
        SupportSubmissionInput {
            submission_key: Uuid::from_u128(key),
            kind: SupportKind::BugReport,
            email: "fictional@example.invalid".into(),
            name: None,
            subject: "Fictional reconnect bug".into(),
            body: body.into(),
        }
        .validate()
        .unwrap()
    }

    #[test]
    fn lost_response_recovers_after_reopen_even_at_capacity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(1).unwrap();
        let first = SupportStore::open(&path, capacity)
            .unwrap()
            .submit(&report(1, "Original"), 10)
            .unwrap();
        let mut reopened = SupportStore::open(&path, capacity).unwrap();
        let replay = reopened.submit(&report(1, "Original"), 20).unwrap();
        assert_eq!(
            replay,
            SupportReceipt {
                deduplicated: true,
                ..first
            }
        );
        assert!(matches!(
            reopened.submit(&report(1, "Changed"), 30),
            Err(SupportStoreError::Conflict)
        ));
        assert!(matches!(
            reopened.submit(&report(2, "New"), 30),
            Err(SupportStoreError::Capacity)
        ));
    }

    #[test]
    fn conversation_pages_resume_after_reopen_without_duplicates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(102).unwrap();
        let mut store = SupportStore::open(&path, capacity).unwrap();
        for key in 1..=101 {
            store.submit(&report(key, "Fictional"), 10).unwrap();
        }
        let first = store.conversation_records(None).unwrap();
        assert_eq!(first.conversations.len(), 100);
        let cursor = first.next_cursor.unwrap();
        drop(store);
        let mut store = SupportStore::open(&path, capacity).unwrap();
        let second = store.conversation_records(Some(&cursor)).unwrap();
        assert_eq!(second.conversations.len(), 1);
        assert!(second.next_cursor.is_none());
        assert!(
            !first
                .conversations
                .iter()
                .any(|row| row.id == second.conversations[0].id)
        );
        assert!(matches!(
            store.conversation_records(Some("invalid")),
            Err(SupportStoreError::InvalidCursor)
        ));
    }

    #[test]
    fn thread_revision_survives_reopen_and_changes_with_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(1).unwrap();
        let mut store = SupportStore::open(&path, capacity).unwrap();
        let receipt = store.submit(&report(1, "Fictional"), 10).unwrap();
        let original = store
            .conversation_thread_records(&receipt.conversation_id)
            .unwrap();
        drop(store);
        let mut store = SupportStore::open(&path, capacity).unwrap();
        assert_eq!(
            store
                .conversation_thread_records(&receipt.conversation_id)
                .unwrap()
                .revision,
            original.revision
        );
        store.connection.execute("INSERT INTO support_messages(id,conversation_id,body,created_at) VALUES (?1,?2,?3,?4)", params![Uuid::now_v7().to_string(), receipt.conversation_id, "Fictional continuation", 20]).unwrap();
        let changed = store
            .conversation_thread_records(&receipt.conversation_id)
            .unwrap();
        assert_ne!(changed.revision, original.revision);
        assert_eq!(changed.messages.len(), 2);
        assert_eq!(changed.conversation.updated_at, 20);
        assert!(
            store
                .submit(&report(1, "Fictional"), 30)
                .unwrap()
                .deduplicated
        );
    }

    #[test]
    fn oversized_history_refuses_instead_of_returning_partial_success() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SupportStore::open(
            &directory.path().join("support.db"),
            NonZeroU32::new(1).unwrap(),
        )
        .unwrap();
        let receipt = store.submit(&report(1, "Fictional"), 10).unwrap();
        store.connection.execute("INSERT INTO support_messages(id,conversation_id,body,created_at) VALUES (?1,?2,?3,?4)", params![Uuid::now_v7().to_string(), receipt.conversation_id, "x".repeat(swarm_domain::SUPPORT_HISTORY_MAX_TEXT_UNITS), 20]).unwrap();
        assert!(matches!(
            store.conversation_thread_records(&receipt.conversation_id),
            Err(SupportStoreError::HistoryCapacity)
        ));
    }

    #[test]
    fn refuses_existing_execution_or_unknown_database_without_modifying_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE workers (id TEXT); INSERT INTO workers VALUES ('keep');")
            .unwrap();
        assert!(matches!(
            SupportStore::open(&path, NonZeroU32::new(10).unwrap()),
            Err(SupportStoreError::WrongDatabase)
        ));
        let value: String = connection
            .query_row("SELECT id FROM workers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "keep");
        let id: i64 = connection
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, 0);
    }

    #[test]
    fn foreign_database_guard_does_not_treat_underscore_as_a_wildcard() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("foreign.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE sqliteX_private (id TEXT);")
            .unwrap();
        assert!(matches!(
            SupportStore::open(&path, NonZeroU32::new(1).unwrap()),
            Err(SupportStoreError::WrongDatabase)
        ));
    }

    #[test]
    fn concurrent_duplicate_intake_commits_one_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("support.db");
        let capacity = NonZeroU32::new(1).unwrap();
        let first = SupportStore::open(&path, capacity).unwrap();
        let second = SupportStore::open(&path, capacity).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|mut store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.submit(&report(1, "Original"), 10).unwrap()
                })
            })
            .collect();
        let receipts: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(receipts[0].conversation_id, receipts[1].conversation_id);
        assert_eq!(receipts[0].message_id, receipts[1].message_id);
        assert_ne!(receipts[0].deduplicated, receipts[1].deduplicated);
    }

    #[test]
    fn separate_reports_from_same_email_do_not_merge_conversations() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SupportStore::open(
            &directory.path().join("support.db"),
            NonZeroU32::new(2).unwrap(),
        )
        .unwrap();
        let first = store.submit(&report(1, "Same text"), 10).unwrap();
        let second = store.submit(&report(2, "Same text"), 10).unwrap();
        assert_ne!(first.conversation_id, second.conversation_id);
    }

    #[test]
    fn message_failure_rolls_back_report_and_allows_retry() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SupportStore::open(
            &directory.path().join("support.db"),
            NonZeroU32::new(1).unwrap(),
        )
        .unwrap();
        store.connection.execute_batch("CREATE TRIGGER fail_message BEFORE INSERT ON support_messages BEGIN SELECT RAISE(ABORT, 'simulated storage failure'); END;").unwrap();
        assert!(matches!(
            store.submit(&report(1, "Original"), 10),
            Err(SupportStoreError::Database(_))
        ));
        store
            .connection
            .execute_batch("DROP TRIGGER fail_message;")
            .unwrap();
        let receipt = store.submit(&report(1, "Original"), 20).unwrap();
        assert!(!receipt.deduplicated);
        assert_eq!(receipt.created_at, 20);
    }
}
