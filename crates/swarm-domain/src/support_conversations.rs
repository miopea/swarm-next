//! Private operator-conversation contract; never a public support receipt.
use serde::Serialize;

use crate::SupportKind;

pub const SUPPORT_CONVERSATION_PAGE_SIZE: usize = 100;
pub const SUPPORT_HISTORY_MAX_MESSAGES: usize = 10_000;
pub const SUPPORT_HISTORY_MAX_TEXT_UNITS: usize = 5_000_000;
pub const SUPPORT_MESSAGE_PART_UNITS: usize = 20_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportContact {
    pub account_id: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportConversationSummary {
    pub id: String,
    pub kind: SupportKind,
    pub subject: String,
    pub preview: String,
    pub customer: SupportContact,
    pub status: String,
    pub needs_attention: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct SupportMessagePart {
    pub index: usize,
    pub total: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportConversationMessage {
    pub id: String,
    pub role: String,
    pub author: SupportContact,
    pub body: String,
    pub created_at: i64,
    pub delivery: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<SupportMessagePart>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportConversationsPage {
    pub resource: &'static str,
    pub version: u8,
    pub collected_at: i64,
    pub conversations: Vec<SupportConversationSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportConversationThread {
    pub resource: &'static str,
    pub version: u8,
    pub collected_at: i64,
    pub conversation: SupportConversationSummary,
    pub messages: Vec<SupportConversationMessage>,
    pub next_cursor: Option<String>,
    pub ai_drafting_supported: bool,
    pub reply_unavailable_reason: Option<String>,
}

/// Return a prefix within the contract's UTF-16 limit, never half a character.
#[must_use]
pub fn support_text_prefix(value: &str, maximum_units: usize) -> &str {
    let mut units = 0;
    for (offset, character) in value.char_indices() {
        units += character.len_utf16();
        if units > maximum_units {
            return &value[..offset];
        }
    }
    value
}

/// Lossless message parts; callers enforce the total history bound first.
#[must_use]
pub fn support_message_parts(mut body: &str) -> Vec<&str> {
    if body.is_empty() {
        return vec![body];
    }
    let mut parts = Vec::new();
    while !body.is_empty() {
        let part = support_text_prefix(body, SUPPORT_MESSAGE_PART_UNITS);
        parts.push(part);
        body = &body[part.len()..];
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_parts_are_lossless_and_contract_bounded() {
        let body = format!("{}🐝{}", "a".repeat(19_999), "b".repeat(20_001));
        let parts = support_message_parts(&body);
        assert_eq!(parts.concat(), body);
        assert!(
            parts
                .iter()
                .all(|part| part.encode_utf16().count() <= SUPPORT_MESSAGE_PART_UNITS)
        );
        assert_eq!(parts[0].len(), 19_999);
        assert!(parts[1].starts_with('🐝'));
        assert_eq!(support_text_prefix("🐝bee", 1), "");
        assert_eq!(support_text_prefix("🐝bee", 2), "🐝");
    }

    #[test]
    fn empty_and_exact_boundary_messages_are_not_lost_or_duplicated() {
        assert_eq!(support_message_parts(""), vec![""]);
        let body = "x".repeat(SUPPORT_MESSAGE_PART_UNITS);
        assert_eq!(support_message_parts(&body), vec![body.as_str()]);
    }
}
