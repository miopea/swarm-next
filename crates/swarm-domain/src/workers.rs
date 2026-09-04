use crate::{HiveId, ProviderConversationId};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(Uuid);

impl WorkerId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorkerId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerSessionId(Uuid);

impl WorkerSessionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for WorkerSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorkerSessionId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerRole {
    Queen,
    Worker,
}

impl fmt::Display for WorkerRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queen => "queen",
            Self::Worker => "worker",
        })
    }
}

impl FromStr for WorkerRole {
    type Err = ParseWorkerRoleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queen" => Ok(Self::Queen),
            "worker" => Ok(Self::Worker),
            _ => Err(ParseWorkerRoleError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseWorkerRoleError;

impl fmt::Display for ParseWorkerRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown worker role")
    }
}

impl std::error::Error for ParseWorkerRoleError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
    /// ALPHA. Its activity classifier is a literal glyph match against a TUI
    /// nobody here has watched redraw under load.
    ///
    /// The three below are grouped deliberately: what makes them alpha is not
    /// that the adapter is unfinished but that `provider_activity` cannot be
    /// trusted for them yet, and a wrong arm fails QUIETLY in both directions
    /// -- a worker busy forever, or resting mid-turn while delivery is deferred
    /// indefinitely. Neither is visible from the board.
    Gemini,
    /// ALPHA. See [`ProviderKind::Gemini`].
    Grok,
    /// ALPHA. See [`ProviderKind::Gemini`].
    OpenCode,
    /// A provider this build does not recognise, read back from storage.
    ///
    /// A provider is stored as a plain string with no CHECK constraint, so a
    /// Hive that rolls back to a release predating a provider will read a value
    /// it has never heard of. Without this the row fails to parse and, because
    /// the roster maps every profile through one query, ONE unreadable worker
    /// takes down the whole listing.
    ///
    /// Deliberately carries no payload. A `Box<str>` would preserve the unknown
    /// name for display, and would also cost `Copy` across every consumer and
    /// change the serialised shape. Losing the name is the cheaper loss.
    ///
    /// Only [`ProviderKind::from_stored`] produces this. `FromStr` stays strict
    /// so the API still refuses an unknown provider at worker creation, which is
    /// input validation and a different question from reading old data.
    Unsupported,
}

impl ProviderKind {
    /// Builder-owned Night Watch promotion list; availability is not approval.
    pub const NIGHT_WATCH_APPROVED: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    /// Reads a stored provider, tolerating a value this build does not know.
    ///
    /// Use for anything coming OUT of the database. Use `from_str` for anything
    /// coming in from an operator or an API caller.
    #[must_use]
    pub fn from_stored(value: &str) -> Self {
        Self::from_str(value).unwrap_or(Self::Unsupported)
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Grok => "grok",
            Self::OpenCode => "opencode",
            Self::Unsupported => "unsupported",
        })
    }
}

impl FromStr for ProviderKind {
    type Err = ParseProviderKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude_code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "grok" => Ok(Self::Grok),
            "opencode" => Ok(Self::OpenCode),
            _ => Err(ParseProviderKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseProviderKindError;

impl fmt::Display for ParseProviderKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown provider kind")
    }
}

impl std::error::Error for ParseProviderKindError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkerProfile {
    pub id: WorkerId,
    pub hive_id: HiveId,
    pub name: String,
    /// Operator-reviewed routing context describing what this worker owns.
    pub description: String,
    pub role: WorkerRole,
    pub provider: ProviderKind,
    pub workspace: String,
    pub autostart: bool,
    pub position: i64,
    pub active_session_id: Option<WorkerSessionId>,
    /// Provider-owned conversation identity used for exact process recovery.
    #[serde(skip)]
    pub provider_conversation_id: Option<ProviderConversationId>,
    /// Whether this profile should resume a provider conversation, because it
    /// previously launched here or imported an exact provider-owned identity.
    #[serde(skip)]
    pub has_session_history: bool,
    /// Expiry of the active operator engagement lease, when one exists.
    #[serde(skip)]
    pub engagement_expires_at: Option<i64>,
    /// Whether this worker is TEMPORARY: spawned beside another to try a second
    /// provider, and not yet adopted into the Hive.
    ///
    /// Serialized rather than skipped because the roster has to show it. A
    /// temporary worker that looks permanent is one an operator will rely on and
    /// then lose.
    pub ephemeral: bool,
    /// The bee this worker wears, when an operator chose one.
    ///
    /// None is the ordinary case and means "derive it from my id", so a Hive
    /// where nobody has chosen anything is still dressed. An unrecognised value
    /// falls back to the derived mark at the render boundary rather than
    /// failing — a choice from a newer build, or one since retired, must not
    /// cost a worker its face.
    #[serde(default)]
    pub mark: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerAttentionState {
    Sleeping,
    Resting,
    Buzzing,
    WithOperator,
    AwaitingOperator,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerSessionState {
    Starting,
    Running,
    Exited,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkerSession {
    pub id: WorkerSessionId,
    pub worker_id: WorkerId,
    pub provider: ProviderKind,
    pub state: WorkerSessionState,
}
