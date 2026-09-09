//! Reviewed file bytes share the parent's `SQLite` commit and retention boundary.
use crate::SupportOutboxError;
use rusqlite::{Connection, Transaction, params};
use swarm_domain::{
    SUPPORT_ATTACHMENT_MAX_BYTES, SupportAttachment, validate_support_attachment_set,
};
use uuid::Uuid;

pub(super) fn migrate(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    let has_manifest: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('hive_support_outbox') WHERE name='attachments_manifest')",
        [], |row| row.get(0))?;
    if !has_manifest {
        transaction.execute_batch("ALTER TABLE hive_support_outbox ADD COLUMN attachments_manifest TEXT NOT NULL DEFAULT '[]';")?;
    }
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS hive_support_outbox_attachments (
        submission_key TEXT NOT NULL REFERENCES hive_support_outbox(submission_key) ON DELETE CASCADE,
        position INTEGER NOT NULL CHECK(position >= 0 AND position < 4),
        metadata TEXT NOT NULL CHECK(length(CAST(metadata AS BLOB)) <= 2048),
        bytes BLOB NOT NULL CHECK(length(bytes) <= 5242880),
        PRIMARY KEY(submission_key, position)
    );")?;
    transaction.pragma_update(
        None,
        "user_version",
        crate::SUPPORT_OUTBOX_ATTACHMENTS_SCHEMA_VERSION,
    )
}

pub(super) fn encoded_size(files: &[SupportAttachment]) -> Result<usize, SupportOutboxError> {
    files.iter().try_fold(0usize, |total, file| {
        Ok(total
            .saturating_add(file.bytes().len())
            .saturating_add(serde_json::to_vec(file.metadata())?.len()))
    })
}

pub(super) fn insert(
    transaction: &Transaction<'_>,
    key: Uuid,
    files: &[SupportAttachment],
) -> Result<(), SupportOutboxError> {
    for (position, file) in files.iter().enumerate() {
        transaction.execute("INSERT INTO hive_support_outbox_attachments (submission_key, position, metadata, bytes) VALUES (?1, ?2, ?3, ?4)",
            params![key.to_string(), position, serde_json::to_string(file.metadata())?, file.bytes()])?;
    }
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    key: Uuid,
    manifest: &str,
) -> Result<Vec<SupportAttachment>, SupportOutboxError> {
    let mut statement = connection.prepare(
        "SELECT position, length(CAST(metadata AS BLOB)), length(bytes), metadata, bytes
        FROM hive_support_outbox_attachments WHERE submission_key = ?1 ORDER BY position LIMIT 5",
    )?;
    let mut rows = statement.query([key.to_string()])?;
    let mut files = Vec::new();
    while let Some(row) = rows.next()? {
        let position: usize = row.get(0)?;
        let metadata_bytes: usize = row.get(1)?;
        let file_bytes: usize = row.get(2)?;
        if position != files.len()
            || position >= 4
            || metadata_bytes > 2048
            || file_bytes > SUPPORT_ATTACHMENT_MAX_BYTES
        {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let metadata: String = row.get(3)?;
        let bytes: Vec<u8> = row.get(4)?;
        files.push(SupportAttachment::validate(
            serde_json::from_str(&metadata)?,
            bytes,
        )?);
    }
    validate_support_attachment_set(&files)?;
    let expected: Vec<swarm_domain::SupportAttachmentMetadata> = serde_json::from_str(manifest)?;
    if expected
        != files
            .iter()
            .map(|file| file.metadata().clone())
            .collect::<Vec<_>>()
    {
        return Err(SupportOutboxError::InvalidTransition);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SupportReceipt, TaskStore};
    use sha2::{Digest, Sha256};
    use swarm_domain::{
        SupportAttachmentMetadata, SupportDeliveryState, SupportKind, SupportSubmission,
        SupportSubmissionInput,
    };
    const DESTINATION: &str =
        "https://admin.example.invalid/api/feedback/swarm-support/submissions";
    fn submission(key: u128) -> SupportSubmission {
        SupportSubmissionInput {
            submission_key: Uuid::from_u128(key),
            kind: SupportKind::BugReport,
            email: "fictional@example.invalid".into(),
            name: None,
            subject: "Fixture".into(),
            body: "Reviewed fixture".into(),
        }
        .validate()
        .unwrap()
    }
    fn file(id: u128, bytes: &[u8]) -> SupportAttachment {
        SupportAttachment::validate(
            SupportAttachmentMetadata {
                id: Uuid::from_u128(id),
                file_name: format!("fixture-{id}.txt"),
                media_type: "text/plain".into(),
                size_bytes: bytes.len(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
            },
            bytes.to_vec(),
        )
        .unwrap()
    }
    #[test]
    fn restart_replays_exact_files_and_fences_old_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let store = TaskStore::open(&path).unwrap();
        let files = vec![file(1, b"first"), file(2, b"second")];
        store
            .enqueue_support_submission_with_attachments(&submission(10), DESTINATION, &files, 1)
            .unwrap();
        let old = store
            .claim_support_submission(Uuid::from_u128(10), 2)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        assert_eq!(store.recover_support_submissions(3).unwrap(), 1);
        let retry = store
            .claim_support_submission(Uuid::from_u128(10), 4)
            .unwrap();
        assert!(retry.attachments == files);
        assert_eq!(retry.delivery.attempts, 2);
        assert!(matches!(
            store.settle_support_submission(
                Uuid::from_u128(10),
                old,
                SupportDeliveryState::Uncertain,
                None,
                5
            ),
            Err(SupportOutboxError::StaleAttempt)
        ));
        assert!(
            store
                .enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &files,
                    6
                )
                .unwrap()
                .attachments
                == files
        );
    }
    #[test]
    fn text_and_attachment_routes_share_identity_and_order_is_frozen() {
        let store = TaskStore::in_memory().unwrap();
        let files = vec![file(1, b"first"), file(2, b"second")];
        store
            .enqueue_support_submission_with_attachments(&submission(10), DESTINATION, &files, 1)
            .unwrap();
        for changed in [
            vec![],
            vec![file(1, b"changed"), files[1].clone()],
            vec![files[1].clone(), files[0].clone()],
            vec![files[0].clone()],
        ] {
            assert!(matches!(
                store.enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &changed,
                    2
                ),
                Err(SupportOutboxError::Conflict)
            ));
        }
        store
            .enqueue_support_submission(&submission(20), DESTINATION, 1)
            .unwrap();
        assert!(matches!(
            store.enqueue_support_submission_with_attachments(
                &submission(20),
                DESTINATION,
                &files,
                2
            ),
            Err(SupportOutboxError::Conflict)
        ));
    }
    #[test]
    fn failed_second_file_rolls_back_parent_and_first_file() {
        let store = TaskStore::in_memory().unwrap();
        store.connection().unwrap().execute_batch("CREATE TRIGGER fixture_fail BEFORE INSERT ON hive_support_outbox_attachments WHEN NEW.position=1 BEGIN SELECT RAISE(ABORT, 'fictional interruption'); END;").unwrap();
        assert!(
            store
                .enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &[file(1, b"one"), file(2, b"two")],
                    1
                )
                .is_err()
        );
        assert!(store.support_submission_statuses().unwrap().is_empty());
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM hive_support_outbox_attachments",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_fail")
            .unwrap();
        assert!(
            store
                .enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &[file(1, b"one")],
                    2
                )
                .is_ok()
        );
    }
    #[test]
    fn missing_saved_file_cannot_silently_become_text_delivery() {
        let store = TaskStore::in_memory().unwrap();
        store
            .enqueue_support_submission_with_attachments(
                &submission(10),
                DESTINATION,
                &[file(1, b"one")],
                1,
            )
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute("DELETE FROM hive_support_outbox_attachments", [])
            .unwrap();
        assert!(matches!(
            store.claim_support_submission(Uuid::from_u128(10), 2),
            Err(SupportOutboxError::InvalidTransition)
        ));
        assert_eq!(
            store.support_submission_statuses().unwrap()[0]
                .delivery
                .attempts,
            0
        );
    }
    #[test]
    fn byte_capacity_retains_reports_and_confirmed_forget_reclaims_files() {
        let store = TaskStore::in_memory().unwrap();
        let files = vec![
            file(1, &vec![b'x'; SUPPORT_ATTACHMENT_MAX_BYTES]),
            file(2, &vec![b'y'; SUPPORT_ATTACHMENT_MAX_BYTES]),
        ];
        store
            .enqueue_support_submission_with_attachments(&submission(10), DESTINATION, &files, 1)
            .unwrap();
        assert!(matches!(
            store.enqueue_support_submission_with_attachments(
                &submission(20),
                DESTINATION,
                &files,
                1
            ),
            Err(SupportOutboxError::Capacity)
        ));
        assert!(
            store
                .enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &files,
                    2
                )
                .is_ok()
        );
        let key = Uuid::from_u128(10);
        let request = swarm_domain::ForgetSupportReport {
            submission_key: key,
            expected_message_id: Uuid::from_u128(30),
        };
        assert!(store.forget_confirmed_support_report(&request).is_err());
        let attempt = store
            .claim_support_submission(key, 3)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        store
            .settle_support_submission(
                key,
                attempt,
                SupportDeliveryState::Confirmed,
                Some(SupportReceipt {
                    submission_key: key.to_string(),
                    conversation_id: Uuid::from_u128(40).to_string(),
                    message_id: request.expected_message_id.to_string(),
                    created_at: 3,
                    deduplicated: false,
                }),
                4,
            )
            .unwrap();
        assert!(store.forget_confirmed_support_report(&request).unwrap());
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM hive_support_outbox_attachments",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            store
                .enqueue_support_submission_with_attachments(
                    &submission(20),
                    DESTINATION,
                    &files,
                    5
                )
                .is_ok()
        );
    }
    #[test]
    fn migration_keeps_legacy_frozen_text_and_delivery_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.db");
        let store = TaskStore::open(&path).unwrap();
        let old = store
            .enqueue_support_submission(&submission(10), DESTINATION, 1)
            .unwrap();
        store.connection().unwrap().execute_batch("DROP TABLE hive_support_outbox_attachments; ALTER TABLE hive_support_outbox DROP COLUMN attachments_manifest; PRAGMA user_version=154;").unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        let current = store.support_submission(Uuid::from_u128(10)).unwrap();
        assert!(current.attachments.is_empty());
        assert_eq!(current.frozen_submission, old.frozen_submission);
        assert_eq!(current.destination, old.destination);
        assert_eq!(current.created_at, old.created_at);
        assert_eq!(
            serde_json::to_string(&current.delivery).unwrap(),
            serde_json::to_string(&old.delivery).unwrap()
        );
    }

    #[test]
    fn two_connections_cannot_duplicate_a_report_or_over_admit_bytes() {
        use std::sync::{Arc, Barrier};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("race.db");
        let observer = TaskStore::open(&path).unwrap();
        // Independent SQLite connections, not clones sharing a Rust connection lock.
        let first = TaskStore::open(&path).unwrap();
        let second = TaskStore::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let other = barrier.clone();
        let run = |store: TaskStore, barrier: Arc<Barrier>| {
            let files = vec![file(1, b"same bytes")];
            barrier.wait();
            store
                .enqueue_support_submission_with_attachments(
                    &submission(10),
                    DESTINATION,
                    &files,
                    1,
                )
                .unwrap()
                .created_at
        };
        let thread = std::thread::spawn(move || run(first, other));
        assert_eq!(run(second, barrier), thread.join().unwrap());
        assert_eq!(observer.support_submission_statuses().unwrap().len(), 1);

        let first = TaskStore::open(&path).unwrap();
        let second = TaskStore::open(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let other = barrier.clone();
        let run = |store: TaskStore, barrier: Arc<Barrier>, key| {
            let files = vec![
                file(1, &vec![b'x'; SUPPORT_ATTACHMENT_MAX_BYTES]),
                file(2, &vec![b'y'; SUPPORT_ATTACHMENT_MAX_BYTES]),
            ];
            barrier.wait();
            match store.enqueue_support_submission_with_attachments(
                &submission(key),
                DESTINATION,
                &files,
                2,
            ) {
                Ok(_) => true,
                Err(SupportOutboxError::Capacity) => false,
                Err(error) => panic!("unexpected admission error: {error}"),
            }
        };
        let thread = std::thread::spawn(move || run(first, other, 20));
        assert_ne!(run(second, barrier, 30), thread.join().unwrap());
        assert_eq!(observer.support_submission_statuses().unwrap().len(), 2);
    }
}
