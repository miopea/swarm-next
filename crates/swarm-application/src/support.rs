use std::sync::{Arc, Mutex};

use swarm_domain::{
    SUPPORT_CONVERSATION_PAGE_SIZE, SUPPORT_HISTORY_MAX_MESSAGES, SupportContact,
    SupportConversationMessage, SupportConversationSummary, SupportConversationThread,
    SupportConversationsPage, SupportMessagePart, support_message_parts, support_text_prefix,
};
use swarm_domain::{SupportSubmissionInput, SupportValidationError};
use swarm_persistence::{SupportReceipt, SupportStore, SupportStoreError};
use thiserror::Error;

/// Central support only; no execution store or worker principal is accepted.
#[derive(Clone)]
pub struct SupportService {
    store: Arc<Mutex<SupportStore>>,
}

#[derive(Debug, Error)]
pub enum SupportServiceError {
    #[error("conversation history changed; refresh before continuing")]
    StaleHistory,
    #[error(transparent)]
    Invalid(#[from] SupportValidationError),
    #[error(transparent)]
    Store(#[from] SupportStoreError),
    #[error("support storage is unavailable")]
    Unavailable,
}

impl SupportService {
    /// Private Admin projection. The support adapter must authenticate its reader.
    ///
    /// # Errors
    /// Refuses invalid cursors and unavailable source data, never reports false emptiness.
    pub fn conversations(
        &self,
        cursor: Option<&str>,
        collected_at: i64,
    ) -> Result<SupportConversationsPage, SupportServiceError> {
        let page = self
            .store
            .lock()
            .map_err(|_| SupportServiceError::Unavailable)?
            .conversation_records(cursor)?;
        Ok(SupportConversationsPage {
            resource: "conversations",
            version: 1,
            collected_at,
            conversations: page
                .conversations
                .into_iter()
                .map(|record| summary(record, None))
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    /// A revision-bound history page. No reply or AI capability is implied.
    ///
    /// # Errors
    /// Refuses cross-thread/stale cursors, oversized histories and storage failures.
    pub fn conversation(
        &self,
        id: &str,
        cursor: Option<&str>,
        collected_at: i64,
    ) -> Result<SupportConversationThread, SupportServiceError> {
        let records = self
            .store
            .lock()
            .map_err(|_| SupportServiceError::Unavailable)?
            .conversation_thread_records(id)?;
        thread_page(records, id, cursor, collected_at)
    }

    #[must_use]
    pub fn new(store: SupportStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Validates public contact input, then atomically records one report.
    /// The adapter supplies server time, never caller-authored audit time.
    ///
    /// # Errors
    /// Refuses invalid content, conflicting retries, capacity or storage failure.
    pub fn submit(
        &self,
        input: SupportSubmissionInput,
        received_at: i64,
    ) -> Result<SupportReceipt, SupportServiceError> {
        let submission = input.validate()?;
        let mut store = self
            .store
            .lock()
            .map_err(|_| SupportServiceError::Unavailable)?;
        Ok(store.submit(&submission, received_at)?)
    }
}

fn contact(record: &swarm_persistence::SupportConversationRecord) -> SupportContact {
    SupportContact {
        account_id: None,
        name: record.submission.name.clone(),
        email: Some(record.submission.email.clone()),
    }
}

fn summary(
    record: swarm_persistence::SupportConversationRecord,
    revision: Option<String>,
) -> SupportConversationSummary {
    let customer = contact(&record);
    SupportConversationSummary {
        id: record.id,
        kind: record.submission.kind,
        subject: record.submission.subject,
        preview: support_text_prefix(&record.submission.body, 500).into(),
        customer,
        status: "open".into(),
        needs_attention: true,
        created_at: record.created_at,
        updated_at: record.updated_at,
        revision,
    }
}

fn thread_page(
    records: swarm_persistence::SupportThreadRecords,
    id: &str,
    cursor: Option<&str>,
    collected_at: i64,
) -> Result<SupportConversationThread, SupportServiceError> {
    let offset = if let Some(cursor) = cursor {
        let parts: Vec<_> = cursor.split('.').collect();
        if cursor.len() > 256 || parts.len() != 4 || parts[0] != "t1" {
            return Err(SupportStoreError::InvalidCursor.into());
        }
        if parts[1] != id || parts[2] != records.revision {
            return Err(SupportServiceError::StaleHistory);
        }
        parts[3]
            .parse::<usize>()
            .map_err(|_| SupportStoreError::InvalidCursor)?
    } else {
        0
    };
    if offset > SUPPORT_HISTORY_MAX_MESSAGES {
        return Err(SupportStoreError::InvalidCursor.into());
    }
    let customer = contact(&records.conversation);
    let mut messages = Vec::new();
    let mut total = 0;
    for message in records.messages {
        let parts = support_message_parts(&message.body);
        let count = parts.len();
        for (index, body) in parts.into_iter().enumerate() {
            if total >= SUPPORT_HISTORY_MAX_MESSAGES {
                return Err(SupportStoreError::HistoryCapacity.into());
            }
            if total >= offset && messages.len() < SUPPORT_CONVERSATION_PAGE_SIZE {
                messages.push(SupportConversationMessage {
                    id: if count > 1 {
                        format!("{}.p{}", message.id, index + 1)
                    } else {
                        message.id.clone()
                    },
                    // Schema v1 only admits customer intake. Operator/provider
                    // history needs explicit persisted role and author first.
                    role: "customer".into(),
                    author: customer.clone(),
                    body: body.into(),
                    created_at: message.created_at,
                    delivery: None,
                    part: (count > 1).then_some(SupportMessagePart {
                        index: index + 1,
                        total: count,
                    }),
                });
            }
            total += 1;
        }
    }
    if offset > total {
        return Err(SupportStoreError::InvalidCursor.into());
    }
    let next_cursor = (offset + messages.len() < total)
        .then(|| format!("t1.{id}.{}.{}", records.revision, offset + messages.len()));
    Ok(SupportConversationThread {
        resource: "conversation",
        version: 1,
        collected_at,
        conversation: summary(records.conversation, Some(records.revision)),
        messages,
        next_cursor,
        ai_drafting_supported: false,
        reply_unavailable_reason: Some(
            "Approved reply delivery is not configured for this source yet.".into(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{num::NonZeroU32, path::Path};
    use swarm_domain::SupportKind;

    const THREAD: &str = "00000000-0000-0000-0000-000000000002";

    fn history(count: usize) -> swarm_persistence::SupportThreadRecords {
        let mut store =
            SupportStore::open(Path::new(":memory:"), NonZeroU32::new(1).unwrap()).unwrap();
        let input = SupportSubmissionInput {
            submission_key: "00000000-0000-0000-0000-000000000001".parse().unwrap(),
            kind: SupportKind::BugReport,
            email: "fictional@example.invalid".into(),
            name: Some("Fictional Bee".into()),
            subject: "Fictional history".into(),
            body: "Initial".into(),
        }
        .validate()
        .unwrap();
        let receipt = store.submit(&input, 10).unwrap();
        let mut records = store
            .conversation_thread_records(&receipt.conversation_id)
            .unwrap();
        records.conversation.id = THREAD.into();
        records.revision = "a".repeat(64);
        let template = records.messages.pop().unwrap();
        for index in 0..count {
            let mut message = template.clone();
            message.id = format!("message-{index}");
            message.body = format!("Fictional message {index}");
            records.messages.push(message);
        }
        records
    }

    #[test]
    fn history_pages_preserve_order_without_duplicates_or_missing_messages() {
        let first = thread_page(history(201), THREAD, None, 20).unwrap();
        assert_eq!(first.messages.len(), 100);
        let second = thread_page(history(201), THREAD, first.next_cursor.as_deref(), 21).unwrap();
        let third = thread_page(history(201), THREAD, second.next_cursor.as_deref(), 22).unwrap();
        assert_eq!(second.messages.len(), 100);
        assert_eq!(third.messages.len(), 1);
        assert!(third.next_cursor.is_none());
        let all: Vec<_> = first
            .messages
            .into_iter()
            .chain(second.messages)
            .chain(third.messages)
            .collect();
        for (index, message) in all.iter().enumerate() {
            assert_eq!(message.id, format!("message-{index}"));
            assert_eq!(message.body, format!("Fictional message {index}"));
        }
    }

    #[test]
    fn stale_cross_thread_and_out_of_range_cursors_refuse() {
        let page = thread_page(history(101), THREAD, None, 20).unwrap();
        let cursor = page.next_cursor.unwrap();
        let mut changed = history(102);
        changed.revision = "b".repeat(64);
        assert!(matches!(
            thread_page(changed, THREAD, Some(&cursor), 21),
            Err(SupportServiceError::StaleHistory)
        ));
        assert!(matches!(
            thread_page(history(101), "another-thread", Some(&cursor), 21),
            Err(SupportServiceError::StaleHistory)
        ));
        let beyond = format!("t1.{THREAD}.{}.999", "a".repeat(64));
        assert!(matches!(
            thread_page(history(101), THREAD, Some(&beyond), 21),
            Err(SupportServiceError::Store(SupportStoreError::InvalidCursor))
        ));
    }

    #[test]
    fn multipart_history_crosses_page_boundary_losslessly() {
        let body = format!("{}🐝{}", "a".repeat(19_999), "b".repeat(20_001));
        let fixture = || {
            let mut records = history(100);
            records.messages[99].body.clone_from(&body);
            records
        };
        let first = thread_page(fixture(), THREAD, None, 20).unwrap();
        let second = thread_page(fixture(), THREAD, first.next_cursor.as_deref(), 21).unwrap();
        let parts: Vec<_> = first
            .messages
            .into_iter()
            .skip(99)
            .chain(second.messages)
            .collect();
        assert_eq!(
            parts
                .iter()
                .map(|part| part.body.as_str())
                .collect::<String>(),
            body
        );
        assert_eq!(parts.len(), 3);
        for (index, message) in parts.iter().enumerate() {
            assert_eq!(message.id, format!("message-99.p{}", index + 1));
            assert_eq!(message.part.as_ref().unwrap().index, index + 1);
            assert_eq!(message.part.as_ref().unwrap().total, 3);
        }
    }

    #[test]
    fn expanded_part_limit_refuses_even_when_source_message_count_is_allowed() {
        let mut records = history(SUPPORT_HISTORY_MAX_MESSAGES);
        records.messages[0].body = "x".repeat(20_001);
        assert!(matches!(
            thread_page(records, THREAD, None, 20),
            Err(SupportServiceError::Store(
                SupportStoreError::HistoryCapacity
            ))
        ));
    }
}
