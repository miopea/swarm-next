use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecisionRequestId(Uuid);

impl DecisionRequestId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for DecisionRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DecisionRequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for DecisionRequestId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRequestKind {
    Input,
    Approval,
    Credentials,
    Conflict,
    Help,
}

impl fmt::Display for DecisionRequestKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "input",
            Self::Approval => "approval",
            Self::Credentials => "credentials",
            Self::Conflict => "conflict",
            Self::Help => "help",
        })
    }
}

impl FromStr for DecisionRequestKind {
    type Err = ParseDecisionRequestKindError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "input" => Ok(Self::Input),
            "approval" => Ok(Self::Approval),
            "credentials" => Ok(Self::Credentials),
            "conflict" => Ok(Self::Conflict),
            "help" => Ok(Self::Help),
            _ => Err(ParseDecisionRequestKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionRequestKindError;
impl fmt::Display for ParseDecisionRequestKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision request kind")
    }
}
impl std::error::Error for ParseDecisionRequestKindError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionUrgency {
    #[default]
    Normal,
    TimeSensitive,
}
impl fmt::Display for DecisionUrgency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Normal => "normal",
            Self::TimeSensitive => "time_sensitive",
        })
    }
}
impl FromStr for DecisionUrgency {
    type Err = ParseDecisionUrgencyError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "normal" => Ok(Self::Normal),
            "time_sensitive" => Ok(Self::TimeSensitive),
            _ => Err(ParseDecisionUrgencyError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionUrgencyError;
impl fmt::Display for ParseDecisionUrgencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision urgency")
    }
}
impl std::error::Error for ParseDecisionUrgencyError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionDeliveryState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}
impl fmt::Display for DecisionDeliveryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}
impl FromStr for DecisionDeliveryState {
    type Err = ParseDecisionDeliveryStateError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseDecisionDeliveryStateError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionDeliveryStateError;
impl fmt::Display for ParseDecisionDeliveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision delivery state")
    }
}
impl std::error::Error for ParseDecisionDeliveryStateError {}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionRequestState {
    Pending,
    Resolved,
}
impl fmt::Display for DecisionRequestState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
        })
    }
}
impl FromStr for DecisionRequestState {
    type Err = ParseDecisionRequestStateError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "resolved" => Ok(Self::Resolved),
            _ => Err(ParseDecisionRequestStateError),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseDecisionRequestStateError;
impl fmt::Display for ParseDecisionRequestStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown decision request state")
    }
}
impl std::error::Error for ParseDecisionRequestStateError {}

/// One question in an interview-shaped decision request.
///
/// A button set is a good instrument for a ruling that is already understood,
/// and a bad one for a question that is still open: it forces the asker to
/// collapse the question into guesses before the operator has said anything.
/// A record carrying questions asks instead of guessing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionQuestion {
    /// Short label, and the key the answer is recorded under.
    pub header: String,
    /// The question as the operator reads it.
    pub question: String,
    /// The choices offered. The operator is not limited to them; an answer that
    /// matches none of these is the most informative kind and must survive.
    pub options: Vec<String>,
    /// Whether more than one option may be chosen.
    #[serde(default)]
    pub multi_select: bool,
}

/// The most questions one interview may carry.
///
/// Mirrors `AskUserQuestion`, which the operator was interviewed with when this
/// was specified. An unbounded interview is a worse instrument than a button.
pub const MAX_DECISION_QUESTIONS: usize = 4;
/// The fewest and most options a question may offer, also mirroring
/// `AskUserQuestion`. One option is not a question; too many is a list.
pub const MIN_DECISION_QUESTION_OPTIONS: usize = 2;
pub const MAX_DECISION_QUESTION_OPTIONS: usize = 4;
pub const MAX_DECISION_QUESTION_HEADER_BYTES: usize = 40;
pub const MAX_DECISION_QUESTION_TEXT_BYTES: usize = 600;
pub const MAX_DECISION_QUESTION_OPTION_BYTES: usize = 200;
/// The most a decision's summary may run to.
///
/// Short enough that it has to be the decision rather than the argument for it:
/// reason, risk and evidence are each capped at ten thousand characters and
/// routinely run to thousands.
pub const MAX_DECISION_SUMMARY_BYTES: usize = 400;
