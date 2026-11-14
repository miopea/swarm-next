//! Images the operator sends with an answer or a question on a request (ADR 0174).
//!
//! The bytes live in a private directory the API owns; this table holds what
//! each file is, which request it went with, and when. A row is written only
//! inside the transaction that records the answer or question it belongs to,
//! so an image is never attached to something that was not sent.
use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{DecisionClarificationId, DecisionRequestId};

use crate::{TaskStore, TaskStoreError};

/// The largest image Swarm keeps for a request: what a worker's model takes
/// in one image block once base64-encoded (5 MiB), the limit ADR 0155 set.
pub const MAX_DECISION_IMAGE_BYTES: u64 = 3_932_160;
/// Images in one answer or one question.
pub const MAX_IMAGES_PER_SUBMISSION: usize = 4;
/// Images across one request: its answer and every question asked about it.
pub const MAX_IMAGES_PER_DECISION: usize = 16;
/// Images this Hive keeps at once.
pub const MAX_DECISION_IMAGES: usize = 512;
/// Bytes this Hive keeps at once.
pub const MAX_DECISION_IMAGE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// How long images outlive the request they went with, once it is closed.
/// The same window a closed request's questions are kept (ADR 0094).
const RETAINED_AFTER_CLOSE_SECONDS: i64 = 90 * 86_400;

/// The media types a worker's model can be shown.
pub const DECISION_IMAGE_MEDIA_TYPES: [&str; 4] =
    ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// An image about to be recorded. The caller has already written its bytes,
/// checked against `media_type`, under `id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewDecisionImage {
    pub id: uuid::Uuid,
    pub media_type: String,
    pub byte_len: u64,
}

/// An image the operator sent. `clarification_id` is set when it went with a
/// question rather than with the answer.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct DecisionImage {
    pub id: uuid::Uuid,
    pub decision_id: DecisionRequestId,
    pub clarification_id: Option<DecisionClarificationId>,
    pub media_type: String,
    pub byte_len: u64,
    pub attached_at: i64,
}

pub(crate) fn migrate_decision_images(
    transaction: &Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS decision_images (
             id TEXT PRIMARY KEY CHECK (length(id) = 36),
             decision_id TEXT NOT NULL REFERENCES decision_requests(id) ON DELETE CASCADE,
             clarification_id TEXT
                 REFERENCES decision_clarifications(id) ON DELETE CASCADE,
             position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 3),
             media_type TEXT NOT NULL CHECK (
                 media_type IN ('image/png','image/jpeg','image/gif','image/webp')
             ),
             byte_len INTEGER NOT NULL CHECK (byte_len BETWEEN 1 AND 3932160),
             attached_at INTEGER NOT NULL CHECK (attached_at >= 0)
         );
         CREATE INDEX IF NOT EXISTS decision_images_by_decision
             ON decision_images(decision_id, attached_at, position);",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// Records `images` as sent with this request's answer, or with one question
/// about it, inside the caller's transaction.
///
/// Closed requests' images past retention are dropped first, so a full store
/// makes room from history before it refuses. Nothing is evicted that a
/// pending request or a recent answer still holds: a full store refuses.
pub(crate) fn attach(
    transaction: &Transaction<'_>,
    decision: DecisionRequestId,
    clarification: Option<DecisionClarificationId>,
    images: &[NewDecisionImage],
    now: i64,
) -> Result<(), TaskStoreError> {
    if images.is_empty() {
        return Ok(());
    }
    if images.len() > MAX_IMAGES_PER_SUBMISSION {
        return Err(TaskStoreError::TooManyDecisionImages {
            limit: MAX_IMAGES_PER_SUBMISSION,
        });
    }
    if images.iter().any(|image| {
        image.byte_len == 0
            || image.byte_len > MAX_DECISION_IMAGE_BYTES
            || !DECISION_IMAGE_MEDIA_TYPES.contains(&image.media_type.as_str())
    }) {
        return Err(TaskStoreError::InvalidDecisionImage);
    }
    prune_expired(transaction, now)?;
    let on_decision: usize = transaction.query_row(
        "SELECT COUNT(*) FROM decision_images WHERE decision_id = ?1",
        [decision.to_string()],
        |row| row.get(0),
    )?;
    if on_decision + images.len() > MAX_IMAGES_PER_DECISION {
        return Err(TaskStoreError::TooManyDecisionImages {
            limit: MAX_IMAGES_PER_DECISION,
        });
    }
    let (count, bytes): (usize, u64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(byte_len), 0) FROM decision_images",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let adding: u64 = images.iter().map(|image| image.byte_len).sum();
    if count + images.len() > MAX_DECISION_IMAGES || bytes + adding > MAX_DECISION_IMAGE_TOTAL_BYTES
    {
        return Err(TaskStoreError::DecisionImageStorageFull);
    }
    for (position, image) in images.iter().enumerate() {
        transaction.execute(
            "INSERT INTO decision_images
                 (id, decision_id, clarification_id, position, media_type, byte_len, attached_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                image.id.to_string(),
                decision.to_string(),
                clarification.map(|id| id.to_string()),
                position,
                image.media_type,
                image.byte_len,
                now,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn prune_expired(transaction: &Transaction<'_>, now: i64) -> rusqlite::Result<usize> {
    transaction.execute(
        "DELETE FROM decision_images WHERE decision_id IN (
             SELECT id FROM decision_requests WHERE state != 'pending'
             AND COALESCE(resolved_at, withdrawn_at) < ?1)",
        [now.saturating_sub(RETAINED_AFTER_CLOSE_SECONDS)],
    )
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionImage> {
    let invalid = |index: usize| {
        move |_| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                "not a valid identifier".into(),
            )
        }
    };
    Ok(DecisionImage {
        id: row.get::<_, String>(0)?.parse().map_err(invalid(0))?,
        decision_id: row.get::<_, String>(1)?.parse().map_err(invalid(1))?,
        clarification_id: row
            .get::<_, Option<String>>(2)?
            .map(|id| id.parse())
            .transpose()
            .map_err(invalid(2))?,
        media_type: row.get(3)?,
        byte_len: row.get(4)?,
        attached_at: row.get(5)?,
    })
}

const SELECT: &str = "SELECT id, decision_id, clarification_id, media_type, byte_len, attached_at
     FROM decision_images";

impl TaskStore {
    /// Every image sent with these requests, oldest first within each.
    ///
    /// # Errors
    /// Returns an error when the query fails or a row is damaged.
    pub fn decision_images(
        &self,
        decisions: &[DecisionRequestId],
    ) -> Result<Vec<DecisionImage>, TaskStoreError> {
        if decisions.is_empty() {
            return Ok(Vec::new());
        }
        let ids = serde_json::to_string(
            &decisions
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let connection = self.connection()?;
        let mut query = connection.prepare(&format!(
            "{SELECT} WHERE decision_id IN (SELECT value FROM json_each(?1))
             ORDER BY decision_id, attached_at, clarification_id IS NOT NULL, position"
        ))?;
        let images = query
            .query_map([ids], from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(images)
    }

    /// One image, only as part of the request it was sent with.
    ///
    /// # Errors
    /// Returns an error when the query fails or the row is damaged.
    pub fn decision_image(
        &self,
        decision: DecisionRequestId,
        image: uuid::Uuid,
    ) -> Result<Option<DecisionImage>, TaskStoreError> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                &format!("{SELECT} WHERE id = ?1 AND decision_id = ?2"),
                params![image.to_string(), decision.to_string()],
                from_row,
            )
            .optional()?)
    }

    /// Every image id this Hive still keeps, so files nothing records can be
    /// removed. Bounded by `MAX_DECISION_IMAGES`.
    ///
    /// # Errors
    /// Returns an error when the query fails or a row is damaged.
    pub fn kept_decision_image_ids(
        &self,
    ) -> Result<std::collections::HashSet<uuid::Uuid>, TaskStoreError> {
        let connection = self.connection()?;
        let mut query = connection.prepare("SELECT id FROM decision_images")?;
        let ids = query
            .query_map([], |row| {
                row.get::<_, String>(0)?.parse().map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        "not a valid identifier".into(),
                    )
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests;
