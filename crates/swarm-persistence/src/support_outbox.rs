//! Private execution-Hive outbox; never a central conversation database.
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{
    SUPPORT_OUTBOX_MAX_BYTES, SUPPORT_OUTBOX_MAX_ROWS, SupportDeliveryState, SupportSubmission,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{SupportReceipt, TaskStore, TaskStoreError};

#[derive(Debug, Error)]
pub enum SupportOutboxError {
    #[error("support submission not found")]
    NotFound,
    #[error("support submission identity conflicts with saved content or destination")]
    Conflict,
    #[error("support outbox capacity reached; existing reports are retained")]
    Capacity,
    #[error("support delivery transition or receipt is invalid")]
    InvalidTransition,
    #[error("support delivery attempt has been superseded")]
    StaleAttempt,
    #[error("support outbox storage unavailable")]
    Store(#[from] TaskStoreError),
    #[error("support outbox database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("support outbox encoding failed")]
    Encoding(#[from] serde_json::Error),
}

/// Deliberately no Debug/Serialize: reviewed customer content is private.
pub struct SupportOutboxEntry {
    pub submission_key: Uuid,
    pub destination: String,
    pub frozen_submission: String,
    pub delivery: SupportOutboxDelivery,
    pub created_at: i64,
}

/// Content-free status, safe for the operator's delivery list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportOutboxDelivery {
    #[serde(default)]
    pub retry_not_before: Option<i64>,
    #[serde(default)]
    pub refusal: Option<SupportRefusal>,
    #[serde(default)]
    pub manual_retry_id: Option<Uuid>,
    #[serde(default)]
    pub manual_retry_expected_attempt: Option<Uuid>,
    #[serde(default)]
    pub manual_retry_pending: bool,
    pub state: SupportDeliveryState,
    pub attempts: u32,
    pub attempt_id: Option<Uuid>,
    pub updated_at: i64,
    pub receipt: Option<SupportReceipt>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportRefusal {
    Conflict,
    Rejected,
    RateLimited,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupportOutboxStatus {
    pub submission_key: String,
    pub created_at: i64,
    pub delivery: SupportOutboxDelivery,
}

pub(super) fn migrate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS hive_support_outbox (
            submission_key TEXT PRIMARY KEY NOT NULL,
            destination TEXT NOT NULL,
            frozen_submission TEXT NOT NULL,
            delivery TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )?;
    transaction.pragma_update(None, "user_version", crate::SUPPORT_OUTBOX_SCHEMA_VERSION)
}

impl TaskStore {
    /// Removes only a receipt-confirmed local copy, never a central conversation.
    ///
    /// # Errors
    /// Refuses unconfirmed state or a changed receipt. Exact absence is idempotent.
    pub fn forget_confirmed_support_report(
        &self,
        request: &swarm_domain::ForgetSupportReport,
    ) -> Result<bool, SupportOutboxError> {
        if request.submission_key.is_nil() || request.expected_message_id.is_nil() {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(entry) = read(&transaction, request.submission_key)? else {
            return Ok(false);
        };
        if !entry.delivery.state.may_remove_local_copy() {
            return Err(SupportOutboxError::InvalidTransition);
        }
        if entry
            .delivery
            .receipt
            .as_ref()
            .map(|receipt| receipt.message_id.as_str())
            != Some(request.expected_message_id.to_string().as_str())
        {
            return Err(SupportOutboxError::Conflict);
        }
        transaction.execute(
            "DELETE FROM hive_support_outbox WHERE submission_key = ?1",
            [request.submission_key.to_string()],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Grants exactly one explicit retry without resetting the automatic attempt count.
    ///
    /// # Errors
    /// Refuses changed command replay, stale observations, unsafe state and changed destination.
    pub fn request_support_retry(
        &self,
        request: &swarm_domain::SupportRetryRequest,
        destination: &str,
        now: i64,
    ) -> Result<SupportOutboxDelivery, SupportOutboxError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut entry =
            read(&transaction, request.submission_key)?.ok_or(SupportOutboxError::NotFound)?;
        if request.retry_id.is_nil() || request.expected_attempt_id.is_nil() || now < 0 {
            return Err(SupportOutboxError::InvalidTransition);
        }
        if entry.destination != destination {
            return Err(SupportOutboxError::Conflict);
        }
        if entry.delivery.manual_retry_id == Some(request.retry_id) {
            if entry.delivery.manual_retry_expected_attempt != Some(request.expected_attempt_id) {
                return Err(SupportOutboxError::Conflict);
            }
            return Ok(entry.delivery);
        }
        if entry.delivery.attempt_id != Some(request.expected_attempt_id) {
            return Err(SupportOutboxError::StaleAttempt);
        }
        if entry.delivery.manual_retry_pending {
            return Err(SupportOutboxError::InvalidTransition);
        }
        entry
            .delivery
            .state
            .begin_manual(entry.delivery.attempts)
            .map_err(|_| SupportOutboxError::InvalidTransition)?;
        entry.delivery.manual_retry_id = Some(request.retry_id);
        entry.delivery.manual_retry_expected_attempt = Some(request.expected_attempt_id);
        entry.delivery.manual_retry_pending = true;
        entry.delivery.updated_at = now;
        save_delivery(&transaction, request.submission_key, &entry.delivery)?;
        transaction.commit()?;
        Ok(entry.delivery)
    }

    /// Bounded content-free operator status; never loads customer message bodies.
    ///
    /// # Errors
    /// Invalid persisted status is unavailable, not a fabricated successful send.
    pub fn support_submission_statuses(
        &self,
    ) -> Result<Vec<SupportOutboxStatus>, SupportOutboxError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT submission_key, created_at, delivery FROM hive_support_outbox ORDER BY created_at, submission_key LIMIT ?1",
        )?;
        let rows = statement.query_map([SUPPORT_OUTBOX_MAX_ROWS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (submission_key, created_at, delivery) = row?;
            Ok(SupportOutboxStatus {
                submission_key,
                created_at,
                delivery: serde_json::from_str(&delivery)?,
            })
        })
        .collect()
    }

    /// Freezes explicitly reviewed content and the configured central destination.
    /// The application validates the destination before this persistence boundary.
    ///
    /// # Errors
    /// Changed-key reuse and capacity refuse new work without deleting saved reports.
    pub fn enqueue_support_submission(
        &self,
        submission: &SupportSubmission,
        destination: &str,
        now: i64,
    ) -> Result<SupportOutboxEntry, SupportOutboxError> {
        if destination.is_empty() || destination.len() > 2048 || now < 0 {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let frozen = serde_json::to_string(submission)?;
        let key = submission.submission_key();
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = read(&transaction, key)? {
            if existing.frozen_submission != frozen || existing.destination != destination {
                return Err(SupportOutboxError::Conflict);
            }
            return Ok(existing);
        }
        let (count, bytes): (usize, usize) = transaction.query_row(
            "SELECT count(*), coalesce(sum(length(CAST(frozen_submission AS BLOB))), 0) FROM hive_support_outbox",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count >= SUPPORT_OUTBOX_MAX_ROWS
            || bytes.saturating_add(frozen.len()) > SUPPORT_OUTBOX_MAX_BYTES
        {
            return Err(SupportOutboxError::Capacity);
        }
        let delivery = SupportOutboxDelivery {
            retry_not_before: None,
            refusal: None,
            manual_retry_id: None,
            manual_retry_expected_attempt: None,
            manual_retry_pending: false,
            state: SupportDeliveryState::Pending,
            attempts: 0,
            attempt_id: None,
            updated_at: now,
            receipt: None,
        };
        transaction.execute(
            "INSERT INTO hive_support_outbox VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                key.to_string(),
                destination,
                frozen,
                serde_json::to_string(&delivery)?,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(SupportOutboxEntry {
            submission_key: key,
            destination: destination.into(),
            frozen_submission: frozen,
            delivery,
            created_at: now,
        })
    }

    /// Reads a single frozen report; no worker-facing listing or terminal access.
    ///
    /// # Errors
    /// Reports absent or unavailable storage explicitly.
    pub fn support_submission(&self, key: Uuid) -> Result<SupportOutboxEntry, SupportOutboxError> {
        let connection = self.connection()?;
        read(&connection, key)?.ok_or(SupportOutboxError::NotFound)
    }

    /// Claims a specific retryable report under the durable transaction lock.
    ///
    /// # Errors
    /// Refuses concurrent claims, terminal outcomes and exhausted retry budgets.
    pub fn claim_support_submission(
        &self,
        key: Uuid,
        now: i64,
    ) -> Result<SupportOutboxEntry, SupportOutboxError> {
        self.claim_support_submission_at(key, None, now)
    }

    /// Claims only when the frozen destination still matches the configured sender.
    ///
    /// # Errors
    /// A changed destination refuses without consuming an attempt or rewriting content.
    pub fn claim_support_submission_for_destination(
        &self,
        key: Uuid,
        destination: &str,
        now: i64,
    ) -> Result<SupportOutboxEntry, SupportOutboxError> {
        self.claim_support_submission_at(key, Some(destination), now)
    }

    fn claim_support_submission_at(
        &self,
        key: Uuid,
        destination: Option<&str>,
        now: i64,
    ) -> Result<SupportOutboxEntry, SupportOutboxError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut entry = read(&transaction, key)?.ok_or(SupportOutboxError::NotFound)?;
        if destination.is_some_and(|destination| destination != entry.destination) {
            return Err(SupportOutboxError::Conflict);
        }
        if now < 0 {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let transition = if entry.delivery.manual_retry_pending {
            entry.delivery.state.begin_manual(entry.delivery.attempts)
        } else {
            entry.delivery.state.begin(entry.delivery.attempts)
        };
        if entry
            .delivery
            .retry_not_before
            .is_some_and(|deadline| now < deadline)
        {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let (state, attempts) = transition.map_err(|_| SupportOutboxError::InvalidTransition)?;
        entry.delivery = SupportOutboxDelivery {
            retry_not_before: None,
            refusal: None,
            manual_retry_id: entry.delivery.manual_retry_id,
            manual_retry_expected_attempt: entry.delivery.manual_retry_expected_attempt,
            manual_retry_pending: false,
            state,
            attempts,
            attempt_id: Some(Uuid::now_v7()),
            updated_at: now,
            receipt: None,
        };
        save_delivery(&transaction, key, &entry.delivery)?;
        transaction.commit()?;
        Ok(entry)
    }

    /// Records a fenced network result. Unknown/invalid receipts are not success.
    ///
    /// # Errors
    /// Refuses old attempt completions and receipts for another submission.
    pub fn settle_support_submission(
        &self,
        key: Uuid,
        attempt: Uuid,
        outcome: SupportDeliveryState,
        receipt: Option<SupportReceipt>,
        now: i64,
    ) -> Result<(), SupportOutboxError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let entry = read(&transaction, key)?.ok_or(SupportOutboxError::NotFound)?;
        if outcome == SupportDeliveryState::RateLimited {
            return Err(SupportOutboxError::InvalidTransition);
        }
        if entry.delivery.attempt_id != Some(attempt)
            || entry.delivery.state != SupportDeliveryState::Delivering
        {
            return Err(SupportOutboxError::StaleAttempt);
        }
        let state = entry
            .delivery
            .state
            .settle(outcome)
            .map_err(|_| SupportOutboxError::InvalidTransition)?;
        if now < 0 || (state == SupportDeliveryState::Confirmed) != receipt.is_some() {
            return Err(SupportOutboxError::InvalidTransition);
        }
        if let Some(receipt) = &receipt {
            let valid_id = |value: &str| Uuid::parse_str(value).is_ok_and(|id| !id.is_nil());
            if receipt.submission_key != key.to_string()
                || !valid_id(&receipt.conversation_id)
                || !valid_id(&receipt.message_id)
                || receipt.created_at < 0
            {
                return Err(SupportOutboxError::InvalidTransition);
            }
        }
        let delivery = SupportOutboxDelivery {
            retry_not_before: None,
            refusal: None,
            manual_retry_id: entry.delivery.manual_retry_id,
            manual_retry_expected_attempt: entry.delivery.manual_retry_expected_attempt,
            manual_retry_pending: false,
            state,
            attempts: entry.delivery.attempts,
            attempt_id: Some(attempt),
            updated_at: now,
            receipt,
        };
        save_delivery(&transaction, key, &delivery)?;
        transaction.commit()?;
        Ok(())
    }

    /// Records an explicit remote refusal under the original attempt fence.
    /// # Errors
    /// Refuses stale attempts, malformed retry bounds and unavailable storage.
    pub fn refuse_support_submission(
        &self,
        key: Uuid,
        attempt: Uuid,
        refusal: SupportRefusal,
        retry_after_seconds: Option<u32>,
        now: i64,
    ) -> Result<(), SupportOutboxError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut entry = read(&transaction, key)?.ok_or(SupportOutboxError::NotFound)?;
        if entry.delivery.attempt_id != Some(attempt)
            || entry.delivery.state != SupportDeliveryState::Delivering
        {
            return Err(SupportOutboxError::StaleAttempt);
        }
        if now < 0
            || (!matches!(refusal, SupportRefusal::RateLimited) && retry_after_seconds.is_some())
            || retry_after_seconds.is_some_and(|seconds| seconds == 0 || seconds > 604_800)
        {
            return Err(SupportOutboxError::InvalidTransition);
        }
        let outcome =
            if matches!(refusal, SupportRefusal::RateLimited) && retry_after_seconds.is_some() {
                SupportDeliveryState::RateLimited
            } else {
                SupportDeliveryState::Failed
            };
        entry.delivery.state = entry
            .delivery
            .state
            .settle(outcome)
            .map_err(|_| SupportOutboxError::InvalidTransition)?;
        entry.delivery.retry_not_before =
            retry_after_seconds.map(|seconds| now.saturating_add(i64::from(seconds)));
        entry.delivery.refusal = Some(refusal);
        entry.delivery.manual_retry_pending = false;
        entry.delivery.updated_at = now;
        save_delivery(&transaction, key, &entry.delivery)?;
        transaction.commit()?;
        Ok(())
    }

    /// Called once by the sole process-owned sender after its previous instance ends.
    /// Never run concurrently with a live sender merely because a timer expired.
    ///
    /// # Errors
    /// A failed recovery transaction preserves the original in-flight records.
    pub fn recover_support_submissions(&self, now: i64) -> Result<usize, SupportOutboxError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let entries = {
            let mut statement = transaction
                .prepare("SELECT submission_key, delivery FROM hive_support_outbox LIMIT ?1")?;
            statement
                .query_map([SUPPORT_OUTBOX_MAX_ROWS], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut recovered = 0;
        for (key, encoded) in entries {
            let mut delivery: SupportOutboxDelivery = serde_json::from_str(&encoded)?;
            if delivery.state == SupportDeliveryState::Delivering {
                delivery.state = delivery.state.interrupted();
                // Retain the last observed identity for explicit retry fencing.
                // The non-Delivering state refuses late settlement from that attempt.
                delivery.updated_at = now.max(delivery.updated_at);
                transaction.execute(
                    "UPDATE hive_support_outbox SET delivery = ?2 WHERE submission_key = ?1",
                    params![key, serde_json::to_string(&delivery)?],
                )?;
                recovered += 1;
            }
        }
        transaction.commit()?;
        Ok(recovered)
    }
}

fn read(
    connection: &rusqlite::Connection,
    key: Uuid,
) -> Result<Option<SupportOutboxEntry>, SupportOutboxError> {
    let row = connection.query_row(
        "SELECT destination, frozen_submission, delivery, created_at FROM hive_support_outbox WHERE submission_key = ?1",
        [key.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)),
    ).optional()?;
    row.map(|(destination, frozen_submission, delivery, created_at)| {
        Ok(SupportOutboxEntry {
            submission_key: key,
            destination,
            frozen_submission,
            delivery: serde_json::from_str(&delivery)?,
            created_at,
        })
    })
    .transpose()
}

fn save_delivery(
    connection: &rusqlite::Connection,
    key: Uuid,
    delivery: &SupportOutboxDelivery,
) -> Result<(), SupportOutboxError> {
    connection.execute(
        "UPDATE hive_support_outbox SET delivery = ?2 WHERE submission_key = ?1",
        params![key.to_string(), serde_json::to_string(delivery)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{SupportKind, SupportSubmissionInput};

    const DESTINATION: &str = "https://support.example.invalid/api/support/v1/submissions";

    #[test]
    fn rate_limit_survives_restart_and_refusal_cannot_settle_a_new_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let hive = TaskStore::open(&path).unwrap();
        let key = Uuid::from_u128(1);
        let saved = hive
            .enqueue_support_submission(&submission(1, "Fictional only"), DESTINATION, 1)
            .unwrap();
        let first = hive
            .claim_support_submission(key, 2)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        hive.refuse_support_submission(key, first, SupportRefusal::RateLimited, Some(120), 3)
            .unwrap();
        drop(hive);
        let hive = TaskStore::open(&path).unwrap();
        assert_eq!(hive.recover_support_submissions(4).unwrap(), 0);
        assert!(hive.claim_support_submission(key, 122).is_err());
        assert_eq!(
            hive.support_submission_statuses().unwrap()[0]
                .delivery
                .attempts,
            1
        );
        let second = hive.claim_support_submission(key, 123).unwrap();
        assert_eq!(second.frozen_submission, saved.frozen_submission);
        assert!(matches!(
            hive.refuse_support_submission(key, first, SupportRefusal::Conflict, None, 124),
            Err(SupportOutboxError::StaleAttempt)
        ));
        hive.refuse_support_submission(
            key,
            second.delivery.attempt_id.unwrap(),
            SupportRefusal::Conflict,
            None,
            125,
        )
        .unwrap();
        assert!(hive.claim_support_submission(key, 1000).is_err());
        let status = hive.support_submission_statuses().unwrap();
        assert_eq!(status[0].delivery.state, SupportDeliveryState::Failed);
        assert!(matches!(
            status[0].delivery.refusal,
            Some(SupportRefusal::Conflict)
        ));
    }

    #[test]
    fn invalid_rate_limit_is_held_without_guessing_an_automatic_retry() {
        let hive = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        hive.enqueue_support_submission(&submission(1, "Fictional only"), DESTINATION, 1)
            .unwrap();
        let attempt = hive
            .claim_support_submission(key, 2)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        assert!(
            hive.refuse_support_submission(
                key,
                attempt,
                SupportRefusal::RateLimited,
                Some(604_801),
                3
            )
            .is_err()
        );
        hive.refuse_support_submission(key, attempt, SupportRefusal::RateLimited, None, 3)
            .unwrap();
        assert!(hive.claim_support_submission(key, 1000).is_err());
        assert_eq!(
            hive.support_submission_statuses().unwrap()[0]
                .delivery
                .state,
            SupportDeliveryState::Failed
        );
    }

    #[test]
    fn status_is_content_free_and_clock_correction_cannot_strand_a_fenced_attempt() {
        let hive = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        hive.enqueue_support_submission(&submission(1, "Private fictional body"), DESTINATION, 100)
            .unwrap();
        let attempt = hive
            .claim_support_submission(key, 90)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        hive.settle_support_submission(key, attempt, SupportDeliveryState::Uncertain, None, 80)
            .unwrap();
        let statuses = hive.support_submission_statuses().unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].delivery.state, SupportDeliveryState::Uncertain);
        let encoded = serde_json::to_string(&statuses).unwrap();
        for private in [
            "Private fictional body",
            "Fictional+test",
            "Fictional Tester",
            DESTINATION,
        ] {
            assert!(!encoded.contains(private));
        }
        assert!(hive.claim_support_submission(key, 70).is_ok());
    }

    #[test]
    fn deployed_schemas_create_the_previously_unshipped_outbox() {
        for previous_version in [150, 152] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("deployed.db");
            let hive = TaskStore::open(&path).unwrap();
            hive.connection()
                .unwrap()
                .execute_batch(&format!(
                    "DROP TABLE hive_support_outbox; PRAGMA user_version = {previous_version};"
                ))
                .unwrap();
            drop(hive);
            let hive = TaskStore::open(&path).unwrap();
            let saved = hive
                .enqueue_support_submission(
                    &submission(1, "Exact reviewed report after upgrade"),
                    DESTINATION,
                    10,
                )
                .unwrap();
            let version: i64 = hive
                .connection()
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, crate::CURRENT_SCHEMA_VERSION);
            assert!(version > previous_version);
            drop(hive);
            let hive = TaskStore::open(&path).unwrap();
            let restored = hive.support_submission(Uuid::from_u128(1)).unwrap();
            assert_eq!(restored.frozen_submission, saved.frozen_submission);
            assert_eq!(restored.destination, saved.destination);
            assert_eq!(restored.delivery.state, SupportDeliveryState::Pending);
        }
    }

    #[test]
    fn migration_preserves_existing_hive_and_saved_outbox_content() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let hive = TaskStore::open(&path).unwrap();
        let worker = hive
            .create_worker(
                "Fictional",
                swarm_domain::ProviderKind::ClaudeCode,
                "/fictional",
                false,
                1,
            )
            .unwrap();
        hive.connection()
            .unwrap()
            .execute_batch("DROP TABLE hive_support_outbox; PRAGMA user_version = 142;")
            .unwrap();
        drop(hive);
        let hive = TaskStore::open(&path).unwrap();
        assert!(hive.get_worker_profile(worker.id).is_ok());
        let saved = hive
            .enqueue_support_submission(&submission(1, "Keep the reviewed bytes"), DESTINATION, 10)
            .unwrap();
        // Idempotent replay of the migration cannot replace an existing outbox.
        hive.connection()
            .unwrap()
            .pragma_update(None, "user_version", 142)
            .unwrap();
        drop(hive);
        let hive = TaskStore::open(&path).unwrap();
        let restored = hive.support_submission(Uuid::from_u128(1)).unwrap();
        assert_eq!(restored.frozen_submission, saved.frozen_submission);
        assert_eq!(restored.delivery.state, SupportDeliveryState::Pending);
        assert!(hive.get_worker_profile(worker.id).is_ok());
    }

    fn submission(key: u128, body: &str) -> SupportSubmission {
        SupportSubmissionInput {
            submission_key: Uuid::from_u128(key),
            kind: SupportKind::BugReport,
            email: "Fictional+test@example.invalid".into(),
            name: Some("Fictional Tester".into()),
            subject: "Reconnect test".into(),
            body: body.into(),
        }
        .validate()
        .unwrap()
    }

    #[test]
    fn explicit_retry_is_once_and_replay_cannot_reset_the_exhausted_budget() {
        let store = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        let saved = store
            .enqueue_support_submission(&submission(1, "Frozen"), DESTINATION, 1)
            .unwrap();
        let mut last = None;
        for _ in 0..swarm_domain::SUPPORT_OUTBOX_MAX_ATTEMPTS {
            let entry = store.claim_support_submission(key, 2).unwrap();
            last = entry.delivery.attempt_id;
            store
                .settle_support_submission(
                    key,
                    last.unwrap(),
                    SupportDeliveryState::Uncertain,
                    None,
                    3,
                )
                .unwrap();
        }
        assert!(store.claim_support_submission(key, 4).is_err());
        let request = swarm_domain::SupportRetryRequest {
            submission_key: key,
            retry_id: Uuid::from_u128(99),
            expected_attempt_id: last.unwrap(),
        };
        store
            .request_support_retry(&request, DESTINATION, 4)
            .unwrap();
        store
            .request_support_retry(&request, DESTINATION, 5)
            .unwrap();
        let next = store.claim_support_submission(key, 6).unwrap();
        assert_eq!(next.delivery.attempts, 6);
        assert_eq!(next.frozen_submission, saved.frozen_submission);
        store
            .settle_support_submission(
                key,
                next.delivery.attempt_id.unwrap(),
                SupportDeliveryState::Uncertain,
                None,
                7,
            )
            .unwrap();
        let replay = store
            .request_support_retry(&request, DESTINATION, 8)
            .unwrap();
        assert!(!replay.manual_retry_pending);
        assert!(store.claim_support_submission(key, 9).is_err());
        let mut changed = request.clone();
        changed.expected_attempt_id = next.delivery.attempt_id.unwrap();
        assert!(matches!(
            store.request_support_retry(&changed, DESTINATION, 10),
            Err(SupportOutboxError::Conflict)
        ));
        let mut stale = request;
        stale.retry_id = Uuid::from_u128(100);
        assert!(matches!(
            store.request_support_retry(&stale, DESTINATION, 11),
            Err(SupportOutboxError::StaleAttempt)
        ));
    }

    #[test]
    fn local_retention_protects_unconfirmed_reports_and_never_deletes_central_history() {
        let store = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        let input = submission(1, "Fictional retained conversation");
        store
            .enqueue_support_submission(&input, DESTINATION, 1)
            .unwrap();
        let mut central = crate::SupportStore::open(
            std::path::Path::new(":memory:"),
            std::num::NonZeroU32::new(1).unwrap(),
        )
        .unwrap();
        let receipt = central.submit(&input, 2).unwrap();
        let request = swarm_domain::ForgetSupportReport {
            submission_key: key,
            expected_message_id: receipt.message_id.parse().unwrap(),
        };
        assert!(store.forget_confirmed_support_report(&request).is_err());
        let attempt = store
            .claim_support_submission(key, 3)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        assert!(store.forget_confirmed_support_report(&request).is_err());
        store
            .settle_support_submission(key, attempt, SupportDeliveryState::Uncertain, None, 4)
            .unwrap();
        assert!(store.forget_confirmed_support_report(&request).is_err());
        let next = store
            .claim_support_submission(key, 5)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        store
            .settle_support_submission(
                key,
                next,
                SupportDeliveryState::Confirmed,
                Some(receipt.clone()),
                6,
            )
            .unwrap();
        let mut wrong = request.clone();
        wrong.expected_message_id = Uuid::from_u128(50);
        assert!(matches!(
            store.forget_confirmed_support_report(&wrong),
            Err(SupportOutboxError::Conflict)
        ));
        assert!(store.forget_confirmed_support_report(&request).unwrap());
        assert!(!store.forget_confirmed_support_report(&request).unwrap());
        assert!(store.support_submission_statuses().unwrap().is_empty());
        assert!(
            central
                .conversation_thread_records(&receipt.conversation_id)
                .is_ok()
        );
        // A lost old browser save still replays to the original central conversation.
        assert!(central.submit(&input, 7).unwrap().deduplicated);
    }

    #[test]
    fn recovered_attempt_retains_retry_identity_but_late_receipt_is_refused() {
        let store = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        store
            .enqueue_support_submission(&submission(1, "Interrupted"), DESTINATION, 1)
            .unwrap();
        let attempt = store
            .claim_support_submission(key, 2)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        store.recover_support_submissions(3).unwrap();
        assert_eq!(
            store.support_submission(key).unwrap().delivery.attempt_id,
            Some(attempt)
        );
        assert!(matches!(
            store.settle_support_submission(key, attempt, SupportDeliveryState::Failed, None, 4),
            Err(SupportOutboxError::StaleAttempt)
        ));
        let request = swarm_domain::SupportRetryRequest {
            submission_key: key,
            retry_id: Uuid::from_u128(50),
            expected_attempt_id: attempt,
        };
        assert!(
            store
                .request_support_retry(&request, DESTINATION, 5)
                .unwrap()
                .manual_retry_pending
        );
    }

    #[test]
    fn freezes_content_destination_and_identity_without_claiming_sent() {
        let store = TaskStore::in_memory().unwrap();
        let submitted = submission(1, "Original\n🐝 report");
        let saved = store
            .enqueue_support_submission(&submitted, DESTINATION, 10)
            .unwrap();
        assert_eq!(saved.delivery.state, SupportDeliveryState::Pending);
        assert_eq!(saved.delivery.attempts, 0);
        let retried = store
            .enqueue_support_submission(&submitted, DESTINATION, 20)
            .unwrap();
        assert_eq!(saved.frozen_submission, retried.frozen_submission);
        assert_eq!(retried.created_at, 10);
        assert!(matches!(
            store.enqueue_support_submission(&submission(1, "Changed"), DESTINATION, 20),
            Err(SupportOutboxError::Conflict)
        ));
        assert!(matches!(
            store.enqueue_support_submission(&submitted, "https://other.example.invalid", 20),
            Err(SupportOutboxError::Conflict)
        ));
    }

    #[test]
    fn restart_recovery_reuses_frozen_bytes_and_fences_the_old_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let key = Uuid::from_u128(1);
        let (attempt, frozen) = {
            let store = TaskStore::open(&path).unwrap();
            store
                .enqueue_support_submission(&submission(1, "Reviewed text"), DESTINATION, 10)
                .unwrap();
            let claimed = store.claim_support_submission(key, 11).unwrap();
            assert!(store.claim_support_submission(key, 12).is_err());
            (
                claimed.delivery.attempt_id.unwrap(),
                claimed.frozen_submission,
            )
        };
        let store = TaskStore::open(path).unwrap();
        assert_eq!(store.recover_support_submissions(20).unwrap(), 1);
        assert_eq!(store.recover_support_submissions(20).unwrap(), 0);
        let next = store.claim_support_submission(key, 21).unwrap();
        assert_eq!(next.frozen_submission, frozen);
        assert_ne!(next.delivery.attempt_id, Some(attempt));
        assert_eq!(next.delivery.attempts, 2);
        assert!(matches!(
            store.settle_support_submission(key, attempt, SupportDeliveryState::Failed, None, 22),
            Err(SupportOutboxError::StaleAttempt)
        ));
    }

    #[test]
    fn lost_central_response_replays_into_the_same_conversation_after_hive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let hive_path = directory.path().join("hive.db");
        let mut central = crate::SupportStore::open(
            &directory.path().join("support.db"),
            std::num::NonZeroU32::new(1).unwrap(),
        )
        .unwrap();
        let key = Uuid::from_u128(1);
        let first = {
            let hive = TaskStore::open(&hive_path).unwrap();
            hive.enqueue_support_submission(
                &submission(1, "Fictional lost-response test"),
                DESTINATION,
                1,
            )
            .unwrap();
            let claimed = hive.claim_support_submission(key, 2).unwrap();
            let input: SupportSubmissionInput =
                serde_json::from_str(&claimed.frozen_submission).unwrap();
            central.submit(&input.validate().unwrap(), 3).unwrap()
            // Simulate remote acceptance followed by process loss, without settling locally.
        };
        let hive = TaskStore::open(&hive_path).unwrap();
        hive.recover_support_submissions(4).unwrap();
        let retry = hive.claim_support_submission(key, 5).unwrap();
        let input: SupportSubmissionInput = serde_json::from_str(&retry.frozen_submission).unwrap();
        let receipt = central.submit(&input.validate().unwrap(), 6).unwrap();
        assert!(receipt.deduplicated);
        assert_eq!(receipt.conversation_id, first.conversation_id);
        assert_eq!(receipt.message_id, first.message_id);
        hive.settle_support_submission(
            key,
            retry.delivery.attempt_id.unwrap(),
            SupportDeliveryState::Confirmed,
            Some(receipt),
            7,
        )
        .unwrap();
        assert_eq!(
            hive.support_submission(key).unwrap().delivery.state,
            SupportDeliveryState::Confirmed
        );
        assert!(hive.claim_support_submission(key, 8).is_err());
    }

    #[test]
    fn invalid_receipt_does_not_confirm_or_replace_an_inflight_report() {
        let hive = TaskStore::in_memory().unwrap();
        let key = Uuid::from_u128(1);
        hive.enqueue_support_submission(&submission(1, "Fictional"), DESTINATION, 1)
            .unwrap();
        let attempt = hive
            .claim_support_submission(key, 2)
            .unwrap()
            .delivery
            .attempt_id
            .unwrap();
        let receipt = SupportReceipt {
            submission_key: Uuid::from_u128(2).to_string(),
            conversation_id: Uuid::from_u128(3).to_string(),
            message_id: Uuid::from_u128(4).to_string(),
            created_at: 2,
            deduplicated: false,
        };
        assert!(
            hive.settle_support_submission(
                key,
                attempt,
                SupportDeliveryState::Confirmed,
                Some(receipt),
                3
            )
            .is_err()
        );
        assert!(
            hive.settle_support_submission(key, attempt, SupportDeliveryState::Confirmed, None, 3)
                .is_err()
        );
        assert_eq!(
            hive.support_submission(key).unwrap().delivery.state,
            SupportDeliveryState::Delivering
        );
        hive.settle_support_submission(key, attempt, SupportDeliveryState::Uncertain, None, 3)
            .unwrap();
    }

    #[test]
    fn capacity_preserves_pending_reports_and_exact_replays_still_work() {
        let hive = TaskStore::in_memory().unwrap();
        for n in 1..=SUPPORT_OUTBOX_MAX_ROWS {
            hive.enqueue_support_submission(&submission(n as u128, "Keep me"), DESTINATION, 1)
                .unwrap();
        }
        assert!(matches!(
            hive.enqueue_support_submission(&submission(1000, "Overflow"), DESTINATION, 1),
            Err(SupportOutboxError::Capacity)
        ));
        assert!(
            hive.enqueue_support_submission(&submission(1, "Keep me"), DESTINATION, 2)
                .is_ok()
        );
        assert_eq!(
            hive.support_submission(Uuid::from_u128(1))
                .unwrap()
                .delivery
                .state,
            SupportDeliveryState::Pending
        );
    }
}
