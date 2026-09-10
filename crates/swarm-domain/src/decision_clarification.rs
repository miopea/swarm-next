//! Explanation exchanges never confer operator permission or settle a decision.
use crate::{DecisionRequestState, WorkerId};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

pub const MAX_CLARIFICATION_TEXT_BYTES: usize = 4_000;
pub const MAX_DECISION_CLARIFICATIONS: usize = 32;
pub const MAX_HIVE_CLARIFICATIONS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecisionClarificationId(Uuid);

impl DecisionClarificationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}
impl Default for DecisionClarificationId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for DecisionClarificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for DecisionClarificationId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationNextMove {
    Operator,
    Requester,
    None,
}

/// Compact inbox facts; private question/reply text is loaded only on demand.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionClarificationSummary {
    pub round_count: usize,
    pub waiting_clarification_id: Option<DecisionClarificationId>,
    pub delivery_state: Option<ClarificationDeliveryState>,
    pub latest_reply_at: Option<i64>,
    pub next_move: ClarificationNextMove,
}

/// A decision and its compact explanation state from the same read snapshot.
#[derive(Clone, Serialize)]
pub struct DecisionInboxEntry {
    #[serde(flatten)]
    pub decision: crate::DecisionRequest,
    pub clarification: Option<DecisionClarificationSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationDeliveryState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
    Cancelled,
}

impl fmt::Display for ClarificationDeliveryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
            Self::Cancelled => "cancelled",
        })
    }
}
impl FromStr for ClarificationDeliveryState {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err("Unknown clarification delivery state"),
        }
    }
}

/// A deferral is only valid before any terminal bytes may have been written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClarificationDeliveryOutcome {
    Delivered,
    DeferredBeforeWrite,
    Uncertain,
}

#[must_use]
pub fn clarification_delivery_result(
    outcome: ClarificationDeliveryOutcome,
    still_applicable: bool,
) -> ClarificationDeliveryState {
    match outcome {
        ClarificationDeliveryOutcome::Delivered => ClarificationDeliveryState::Delivered,
        ClarificationDeliveryOutcome::Uncertain => ClarificationDeliveryState::Uncertain,
        ClarificationDeliveryOutcome::DeferredBeforeWrite if still_applicable => {
            ClarificationDeliveryState::Queued
        }
        ClarificationDeliveryOutcome::DeferredBeforeWrite => ClarificationDeliveryState::Cancelled,
    }
}

/// Delivery is deliberately not an argument: receipt is not a reply.
#[must_use]
pub fn clarification_next_move(
    parent: DecisionRequestState,
    unanswered: bool,
) -> ClarificationNextMove {
    if parent != DecisionRequestState::Pending {
        ClarificationNextMove::None
    } else if unanswered {
        ClarificationNextMove::Requester
    } else {
        ClarificationNextMove::Operator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionClarificationError {
    InvalidText,
    DecisionNotPending,
    AlreadyWaiting,
    Capacity,
    Conflict,
    Unauthorized,
    NotFound,
}
impl fmt::Display for DecisionClarificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidText => "A clarification needs nonempty text of at most 4000 UTF-8 bytes",
            Self::DecisionNotPending => "This decision is no longer waiting for an answer",
            Self::AlreadyWaiting => "This decision is already waiting for a clarification reply",
            Self::Capacity => "Clarification history is at capacity; no question was sent",
            Self::Conflict => "This clarification identity already has different saved content",
            Self::Unauthorized => "Only the requester or Queen may reply to this clarification",
            Self::NotFound => "The clarification was not found in this Hive",
        })
    }
}
impl std::error::Error for DecisionClarificationError {}

/// Check without trimming or changing the text that will be recorded.
/// # Errors
/// Rejects empty/whitespace-only or oversized UTF-8 text.
pub fn validate_clarification_text(text: &str) -> Result<(), DecisionClarificationError> {
    if text.trim().is_empty() || text.len() > MAX_CLARIFICATION_TEXT_BYTES {
        Err(DecisionClarificationError::InvalidText)
    } else {
        Ok(())
    }
}

/// Counts and parent state must come from the admitting transaction.
/// Exact immutable-ID replay is checked before new admission.
/// # Errors
/// Rejects settled decisions, concurrent questions, invalid text or capacity.
pub fn validate_new_clarification(
    parent: DecisionRequestState,
    text: &str,
    unanswered: bool,
    decision_count: usize,
    hive_count: usize,
) -> Result<(), DecisionClarificationError> {
    validate_clarification_text(text)?;
    if parent != DecisionRequestState::Pending {
        return Err(DecisionClarificationError::DecisionNotPending);
    }
    if unanswered {
        return Err(DecisionClarificationError::AlreadyWaiting);
    }
    if decision_count >= MAX_DECISION_CLARIFICATIONS || hive_count >= MAX_HIVE_CLARIFICATIONS {
        return Err(DecisionClarificationError::Capacity);
    }
    Ok(())
}

/// Role and identity are authenticated by the application, never client labels.
/// # Errors
/// Rejects a worker other than the original requester or the local Queen.
pub fn validate_clarification_responder(
    requester: WorkerId,
    responder: WorkerId,
    authenticated_queen: bool,
) -> Result<(), DecisionClarificationError> {
    if responder == requester || authenticated_queen {
        Ok(())
    } else {
        Err(DecisionClarificationError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarification_never_turns_into_an_operator_answer() {
        assert_eq!(
            clarification_next_move(DecisionRequestState::Pending, true),
            ClarificationNextMove::Requester
        );
        assert_eq!(
            clarification_next_move(DecisionRequestState::Pending, false),
            ClarificationNextMove::Operator
        );
        for state in [
            DecisionRequestState::Resolved,
            DecisionRequestState::Withdrawn,
        ] {
            assert_eq!(
                clarification_next_move(state, true),
                ClarificationNextMove::None
            );
            assert_eq!(
                clarification_next_move(state, false),
                ClarificationNextMove::None
            );
        }
    }

    #[test]
    fn text_is_utf8_bounded_and_never_silently_rewritten() {
        let text = "  Please explain this choice.\n";
        assert!(validate_clarification_text(text).is_ok());
        for invalid in [String::new(), " \n\t".into(), "é".repeat(2001)] {
            assert_eq!(
                validate_clarification_text(&invalid),
                Err(DecisionClarificationError::InvalidText)
            );
        }
        assert!(validate_clarification_text(&"é".repeat(2000)).is_ok());
    }

    #[test]
    fn admission_is_bounded_and_requires_a_pending_parent() {
        let admit = |state, waiting, decision, hive| {
            validate_new_clarification(state, "Why?", waiting, decision, hive)
        };
        assert!(admit(DecisionRequestState::Pending, false, 31, 4095).is_ok());
        assert_eq!(
            admit(DecisionRequestState::Resolved, false, 0, 0),
            Err(DecisionClarificationError::DecisionNotPending)
        );
        assert_eq!(
            admit(DecisionRequestState::Withdrawn, false, 0, 0),
            Err(DecisionClarificationError::DecisionNotPending)
        );
        assert_eq!(
            admit(DecisionRequestState::Pending, true, 0, 0),
            Err(DecisionClarificationError::AlreadyWaiting)
        );
        for (decision, hive) in [(32, 0), (0, 4096)] {
            assert_eq!(
                admit(DecisionRequestState::Pending, false, decision, hive),
                Err(DecisionClarificationError::Capacity)
            );
        }
    }

    #[test]
    fn other_workers_cannot_supply_the_reply() {
        let requester = WorkerId::new();
        let other = WorkerId::new();
        assert!(validate_clarification_responder(requester, requester, false).is_ok());
        assert!(validate_clarification_responder(requester, other, true).is_ok());
        assert_eq!(
            validate_clarification_responder(requester, other, false),
            Err(DecisionClarificationError::Unauthorized)
        );
    }
}
