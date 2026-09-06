//! Bounded snapshots from the separate support database, not the execution Hive.
use rusqlite::OptionalExtension;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use swarm_domain::{
    SUPPORT_CONVERSATION_PAGE_SIZE, SUPPORT_HISTORY_MAX_MESSAGES, SUPPORT_HISTORY_MAX_TEXT_UNITS,
    SupportKind,
};
use uuid::Uuid;

use super::{SupportStore, SupportStoreError};

#[derive(Deserialize)]
pub struct StoredSupportSubmission {
    pub kind: SupportKind,
    pub email: String,
    pub name: Option<String>,
    pub subject: String,
    pub body: String,
}

pub struct SupportConversationRecord {
    pub id: String,
    pub submission: StoredSupportSubmission,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct SupportListRecords {
    pub conversations: Vec<SupportConversationRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Clone)]
pub struct SupportMessageRecord {
    pub id: String,
    pub body: String,
    pub created_at: i64,
}

pub struct SupportThreadRecords {
    pub conversation: SupportConversationRecord,
    pub messages: Vec<SupportMessageRecord>,
    pub revision: String,
}

fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, i64, i64)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn decode(
    value: (String, String, i64, i64),
) -> Result<SupportConversationRecord, SupportStoreError> {
    Ok(SupportConversationRecord {
        id: value.0,
        submission: serde_json::from_str(&value.1)?,
        created_at: value.2,
        updated_at: value.3,
    })
}

impl SupportStore {
    /// Stable ID keyset pages with a frozen upper bound for new submissions.
    ///
    /// # Errors
    /// Refuses malformed cursors and corrupt or unavailable storage.
    pub fn conversation_records(
        &mut self,
        cursor: Option<&str>,
    ) -> Result<SupportListRecords, SupportStoreError> {
        let transaction = self.connection.transaction()?;
        let (after, ceiling) = if let Some(cursor) = cursor {
            let parts: Vec<_> = cursor.split('.').collect();
            if cursor.len() > 256
                || parts.len() != 3
                || parts[0] != "l1"
                || parts[1..].iter().any(|part| Uuid::parse_str(part).is_err())
            {
                return Err(SupportStoreError::InvalidCursor);
            }
            (parts[2].to_owned(), Some(parts[1].to_owned()))
        } else {
            (
                String::new(),
                transaction.query_row("SELECT max(id) FROM support_conversations", [], |row| {
                    row.get::<_, Option<String>>(0)
                })?,
            )
        };
        let Some(ceiling) = ceiling else {
            return Ok(SupportListRecords {
                conversations: Vec::new(),
                next_cursor: None,
            });
        };
        let mut rows = {
            let mut statement = transaction.prepare(
                "SELECT c.id,c.frozen_submission,c.created_at,coalesce((SELECT max(m.created_at) FROM support_messages m WHERE m.conversation_id=c.id),c.created_at)
                 FROM support_conversations c WHERE c.id>?1 AND c.id<=?2 ORDER BY c.id LIMIT 101",
            )?;
            statement
                .query_map(rusqlite::params![after, ceiling], record)?
                .collect::<Result<Vec<_>, _>>()?
        };
        let more = rows.len() > SUPPORT_CONVERSATION_PAGE_SIZE;
        rows.truncate(SUPPORT_CONVERSATION_PAGE_SIZE);
        let next_cursor = if more {
            rows.last().map(|row| format!("l1.{ceiling}.{}", row.0))
        } else {
            None
        };
        let conversations = rows.into_iter().map(decode).collect::<Result<_, _>>()?;
        transaction.commit()?;
        Ok(SupportListRecords {
            conversations,
            next_cursor,
        })
    }

    /// One bounded transaction for the header, full source history and revision.
    ///
    /// # Errors
    /// Refuses unknown threads, oversized history and corrupt source data.
    pub fn conversation_thread_records(
        &mut self,
        id: &str,
    ) -> Result<SupportThreadRecords, SupportStoreError> {
        if Uuid::parse_str(id).is_err() {
            return Err(SupportStoreError::NotFound);
        }
        let transaction = self.connection.transaction()?;
        let raw = transaction.query_row(
            "SELECT c.id,c.frozen_submission,c.created_at,coalesce((SELECT max(m.created_at) FROM support_messages m WHERE m.conversation_id=c.id),c.created_at)
             FROM support_conversations c WHERE c.id=?1", [id], record,
        ).optional()?.ok_or(SupportStoreError::NotFound)?;
        let mut digest = Sha256::new();
        digest.update(b"swarm-support-thread-v1");
        digest.update(serde_json::to_vec(&raw)?);
        let conversation = decode(raw)?;
        let mut messages = Vec::new();
        let mut text_units = 0;
        {
            let mut statement = transaction.prepare("SELECT id,body,created_at FROM support_messages WHERE conversation_id=?1 ORDER BY created_at,id LIMIT 10001")?;
            let mut rows = statement.query([id])?;
            while let Some(row) = rows.next()? {
                let message = SupportMessageRecord {
                    id: row.get(0)?,
                    body: row.get(1)?,
                    created_at: row.get(2)?,
                };
                text_units += message.body.encode_utf16().count();
                if messages.len() == SUPPORT_HISTORY_MAX_MESSAGES
                    || text_units > SUPPORT_HISTORY_MAX_TEXT_UNITS
                {
                    return Err(SupportStoreError::HistoryCapacity);
                }
                let bytes = serde_json::to_vec(&(&message.id, &message.body, message.created_at))?;
                digest.update((bytes.len() as u64).to_be_bytes());
                digest.update(bytes);
                messages.push(message);
            }
        }
        transaction.commit()?;
        Ok(SupportThreadRecords {
            conversation,
            messages,
            revision: format!("{:x}", digest.finalize()),
        })
    }
}
