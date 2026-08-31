use crate::{HiveId, WorkerId, WorkerSessionId};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(Uuid);

impl TaskId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TaskId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Draft,
    Ready,
    Active,
    Blocked,
    Review,
    Completed,
    /// Closed for a reason other than success.
    ///
    /// NOT Completed-with-a-flag, on purpose. Completed asks what evidence
    /// shows the work is running, and for work nobody finished that question
    /// has no answer -- so it was being answered with an exemption claim
    /// somebody then had to approve. A separate state does not make the
    /// question cheaper to answer; it removes the question.
    Abandoned,
}

impl TaskState {
    #[must_use]
    pub const fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Draft, Self::Ready)
                | (Self::Ready, Self::Active | Self::Blocked)
                | (Self::Active, Self::Blocked | Self::Review)
                | (Self::Blocked, Self::Ready | Self::Active)
                | (Self::Review, Self::Active | Self::Ready | Self::Completed)
                // ABANDONED IS REACHABLE FROM EVERY UNFINISHED STATE, including
                // Draft. Forcing a detour through Blocked to abandon something
                // would be clicking, which is the thing this state exists to
                // delete. Draft is included because a draft Queen decides
                // against is a decision worth keeping: `remove_task` disposes
                // of mistakes and duplicates and destroys the record, which is
                // a different act from declining work on purpose.
                | (
                    Self::Draft | Self::Ready | Self::Active | Self::Blocked | Self::Review,
                    Self::Abandoned,
                )
        )
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        };
        formatter.write_str(value)
    }
}

impl FromStr for TaskState {
    type Err = ParseTaskStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "ready" => Ok(Self::Ready),
            "active" => Ok(Self::Active),
            "blocked" => Ok(Self::Blocked),
            "review" => Ok(Self::Review),
            "completed" => Ok(Self::Completed),
            "abandoned" => Ok(Self::Abandoned),
            _ => Err(ParseTaskStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskStateError;

impl fmt::Display for ParseTaskStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task state")
    }
}

impl std::error::Error for ParseTaskStateError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDispatchState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}

impl fmt::Display for TaskDispatchState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}

impl FromStr for TaskDispatchState {
    type Err = ParseTaskDispatchStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseTaskDispatchStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskDispatchStateError;

impl fmt::Display for ParseTaskDispatchStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task dispatch state")
    }
}

impl std::error::Error for ParseTaskDispatchStateError {}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcomeDeliveryState {
    Queued,
    Dispatching,
    Delivered,
    Uncertain,
}

impl fmt::Display for TaskOutcomeDeliveryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Queued => "queued",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Uncertain => "uncertain",
        })
    }
}

impl FromStr for TaskOutcomeDeliveryState {
    type Err = ParseTaskOutcomeDeliveryStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseTaskOutcomeDeliveryStateError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskOutcomeDeliveryStateError;

impl fmt::Display for ParseTaskOutcomeDeliveryStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task outcome delivery state")
    }
}

impl std::error::Error for ParseTaskOutcomeDeliveryStateError {}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

impl fmt::Display for TaskPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        })
    }
}

impl FromStr for TaskPriority {
    type Err = ParseTaskPriorityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "urgent" => Ok(Self::Urgent),
            _ => Err(ParseTaskPriorityError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskPriorityError;

impl fmt::Display for ParseTaskPriorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task priority")
    }
}

impl std::error::Error for ParseTaskPriorityError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityKind {
    Created,
    DetailsUpdated,
    StateChanged,
    Assigned,
    Unassigned,
    Removed,
    Restored,
    /// A correction appended to a task's record without moving it.
    ///
    /// A handoff true when written stops being true, and saying so used to mean
    /// leaving the state and coming back — which takes finished work out of
    /// Queen's review queue and reads as though it restarted. This kind marks
    /// an amendment in place: the note it corrects stays beside it, because
    /// what was believed and when is part of the record.
    Corrected,
    /// A fact appended to a task while the work on it continues.
    ///
    /// Amendments are how a worker records progress without moving the task,
    /// and they used to write only to `task_amendments` -- so the trail a
    /// reader actually reads did not contain them. Two readers saw two
    /// histories of the same task and the authoritative-looking one was the
    /// one missing the entries.
    Amended,
    /// A note a worker attached to work in progress, changing nothing else.
    ///
    /// Distinct from `Amended`, and the distinction is the whole reason this
    /// exists. An amendment is a correction of FACT about the description —
    /// every listing carries it beside the task and tells a reader to believe
    /// it over the description where they disagree. A prediction made before
    /// the code exists is not that: it is a claim about the future that the
    /// outcome may well falsify, and filing it as a standing correction would
    /// tell every later reader to believe something that turned out wrong.
    ///
    /// It also does NOT count as acting on the task. `last_task_action_source!`
    /// lists `corrected`, `details_updated` and `amended`, and this is
    /// deliberately absent, so writing notes cannot hold off the stale-work
    /// flag. A worker that talks instead of working is still reported as
    /// unchanged after the threshold, which is what keeps this from becoming a
    /// way to look busy.
    Noted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityActorKind {
    Operator,
    Worker,
    Jira,
    Email,
    System,
}

impl fmt::Display for TaskActivityActorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Operator => "operator",
            Self::Worker => "worker",
            Self::Jira => "jira",
            Self::Email => "email",
            Self::System => "system",
        })
    }
}

impl FromStr for TaskActivityActorKind {
    type Err = ParseTaskActivityActorKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "operator" => Ok(Self::Operator),
            "worker" => Ok(Self::Worker),
            "jira" => Ok(Self::Jira),
            "email" => Ok(Self::Email),
            "system" => Ok(Self::System),
            _ => Err(ParseTaskActivityActorKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskActivityActorKindError;

impl fmt::Display for ParseTaskActivityActorKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task activity actor kind")
    }
}

impl std::error::Error for ParseTaskActivityActorKindError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskActivityActor {
    pub kind: TaskActivityActorKind,
    pub id: Option<String>,
}

impl TaskActivityActor {
    #[must_use]
    pub fn operator() -> Self {
        Self {
            kind: TaskActivityActorKind::Operator,
            id: None,
        }
    }
    #[must_use]
    pub fn worker(id: WorkerId) -> Self {
        Self {
            kind: TaskActivityActorKind::Worker,
            id: Some(id.to_string()),
        }
    }
    #[must_use]
    pub fn jira() -> Self {
        Self {
            kind: TaskActivityActorKind::Jira,
            id: None,
        }
    }
    #[must_use]
    pub fn email() -> Self {
        Self {
            kind: TaskActivityActorKind::Email,
            id: None,
        }
    }
    #[must_use]
    pub fn system() -> Self {
        Self {
            kind: TaskActivityActorKind::System,
            id: None,
        }
    }
}

impl fmt::Display for TaskActivityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Created => "created",
            Self::DetailsUpdated => "details_updated",
            Self::StateChanged => "state_changed",
            Self::Assigned => "assigned",
            Self::Unassigned => "unassigned",
            Self::Removed => "removed",
            Self::Restored => "restored",
            Self::Corrected => "corrected",
            Self::Amended => "amended",
            Self::Noted => "noted",
        })
    }
}

impl FromStr for TaskActivityKind {
    type Err = ParseTaskActivityKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "created" => Ok(Self::Created),
            "details_updated" => Ok(Self::DetailsUpdated),
            "state_changed" => Ok(Self::StateChanged),
            "assigned" => Ok(Self::Assigned),
            "unassigned" => Ok(Self::Unassigned),
            "removed" => Ok(Self::Removed),
            "restored" => Ok(Self::Restored),
            "corrected" => Ok(Self::Corrected),
            "amended" => Ok(Self::Amended),
            "noted" => Ok(Self::Noted),
            _ => Err(ParseTaskActivityKindError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseTaskActivityKindError;

impl fmt::Display for ParseTaskActivityKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown task activity kind")
    }
}

impl std::error::Error for ParseTaskActivityKindError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskActivity {
    pub sequence: i64,
    pub task_id: TaskId,
    pub kind: TaskActivityKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>,
    pub note: String,
    pub occurred_at: i64,
    pub actor_kind: TaskActivityActorKind,
    pub actor_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskActivityPage {
    pub events: Vec<TaskActivity>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskDetailsUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<TaskPriority>,
    pub workspace: Option<String>,
    pub operator_instruction: Option<String>,
}

/// A correction of FACT appended to a task's description.
///
/// Never replaces the description and never outranks its scope or acceptance —
/// the operator's ruling was "facts govern, scope and acceptance never do". A
/// reader takes an amendment as authoritative about what is TRUE and takes the
/// original as authoritative about what the work is FOR.
///
/// Always attributed. An unattributed amendment to governing text would be worse
/// than the stale text it corrects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskAmendment {
    pub id: String,
    pub author_worker_id: WorkerId,
    /// The author's name at the time it is read, so a reader sees who without a
    /// second lookup. Denormalised deliberately: the id is the durable link.
    pub author_name: String,
    pub body: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent, separately computed fact about the task, and they are not mutually exclusive: work can be untouched by any Swarm worker and also carry a deployment. Collapsing them into one enum would assert an exclusivity the board does not have"
)]
pub struct Task {
    pub id: TaskId,
    pub hive_id: HiveId,
    pub title: String,
    pub description: String,
    /// One line from the operator about how this task should be approached,
    /// rather than about what it contains. "Interview me first" and "analyse
    /// this, do not act on it" govern the work without being part of it, and
    /// putting them in the description makes them read as part of it.
    pub operator_instruction: String,
    /// Whether anyone has recorded where this work is running.
    ///
    /// Computed rather than stored. A completed task without it is finished,
    /// not shown to be live — and calling that COMPLETED claims more than
    /// anyone has established, which is the same distinction between committed
    /// and deployed that this repo draws everywhere else.
    #[serde(default)]
    pub deployment_recorded: bool,
    /// Whether this task has evidence that can close it: a recorded deployment,
    /// or a nothing-to-deploy claim Queen has approved.
    ///
    /// Distinct from `deployment_recorded`, and the board needs both. Work
    /// closed on an approved exemption is properly finished — somebody looked
    /// and agreed there was nothing to ship — but it has no deployment, so
    /// keying "unverified" off `deployment_recorded` alone libelled it. On this
    /// Hive that was 29 of the 67 rows the badge appeared on.
    #[serde(default)]
    pub closed_on_evidence: bool,
    /// Whether the operator has recorded that this work CANNOT now be shown to
    /// be live.
    ///
    /// A third outcome, and deliberately not a kind of evidence. A deployment
    /// says where the work is running; an approved exemption says there was
    /// nothing to ship. This says neither could be established — something may
    /// well have shipped and nobody can prove it now. Collapsing it into either
    /// of the other two would make the board claim something nobody checked,
    /// which is the exact failure the evidence gate exists to prevent.
    ///
    /// It takes a task out of the "waiting on evidence" queue, because nothing
    /// is coming, and it must never read as verified anywhere it is shown.
    #[serde(default)]
    pub closed_unverifiable: bool,
    /// Whether any Swarm worker has ever acted on this task.
    ///
    /// A task imported from Jira and completed there has none: its only
    /// activity is the sync that mirrored it. Swarm did not do that work, so
    /// Swarm has no deployment to record for it, and asking for one is a
    /// category error rather than missing evidence.
    ///
    /// The condition is worker involvement, NOT the presence of a Jira link.
    /// Work a Swarm worker really did against a Jira issue and never deployed
    /// is a genuine gap and must stay visible. On this Hive that case has never
    /// occurred — zero Jira-linked tasks have any worker activity, in any state
    /// — which is exactly why the distinction is drawn in code rather than left
    /// to the current shape of the data.
    #[serde(default)]
    pub worked_here: bool,
    pub priority: TaskPriority,
    pub workspace: String,
    pub state: TaskState,
    pub assigned_worker_id: Option<WorkerId>,
    pub assigned_session_id: Option<WorkerSessionId>,
    pub dispatch_state: Option<TaskDispatchState>,
    pub outcome_delivery_state: Option<TaskOutcomeDeliveryState>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What a single reported commit turned out to be, checked once when reported.
///
/// A SNAPSHOT, never a live query. This repository squash-merges and rebases as
/// a matter of routine, which destroys reported SHAs — re-checking later would
/// turn green evidence red weeks after the fact for work that was perfectly
/// correct, and a check that fails on correct input teaches its reader to
/// ignore it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitVerdict {
    /// The object is a commit and at least one ref reaches it.
    Present,
    /// The object is a commit that no ref reaches — dangling after a rebase.
    Unreachable,
    /// No such object in this repository.
    Missing,
    /// Nothing was asked, because the workspace could not be read as a
    /// repository. Distinct from `Missing`, which is an answer.
    Unchecked,
}

impl fmt::Display for CommitVerdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Present => "present",
            Self::Unreachable => "unreachable",
            Self::Missing => "missing",
            Self::Unchecked => "unchecked",
        })
    }
}

impl FromStr for CommitVerdict {
    type Err = ParseTaskStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "present" => Ok(Self::Present),
            "unreachable" => Ok(Self::Unreachable),
            "missing" => Ok(Self::Missing),
            "unchecked" => Ok(Self::Unchecked),
            _ => Err(ParseTaskStateError),
        }
    }
}

/// Whether the workspace could be read as a repository at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitRepositoryState {
    Read,
    /// The path exists but is not a Git checkout. NOT an error: work in a
    /// workspace nobody put under version control still has to be able to close.
    NotARepository,
}

impl fmt::Display for CommitRepositoryState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Read => "read",
            Self::NotARepository => "not_a_repository",
        })
    }
}

impl FromStr for CommitRepositoryState {
    type Err = ParseTaskStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read" => Ok(Self::Read),
            "not_a_repository" => Ok(Self::NotARepository),
            _ => Err(ParseTaskStateError),
        }
    }
}

/// One commit a worker attributed to its task, and what checking it found.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskCommit {
    pub sha: String,
    pub verdict: CommitVerdict,
    pub subject: String,
    /// The paths this commit touches. Stored as FACT, so that the question of
    /// which paths count as documentation stays a policy someone else applies
    /// rather than a judgement baked into the record.
    pub changed_paths: Vec<String>,
}

/// What a task's worker said it produced, and what the repository said back.
///
/// THE ROW EXISTING IS ITSELF THE FACT. An empty `commits` is a worker saying
/// "nothing was built"; NO REPORT AT ALL is nobody having said anything. Those
/// are different, and collapsing them would let unreported work read as an
/// investigation that produced nothing — closing it automatically on the
/// strength of a question never asked.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskCommitReport {
    pub task_id: TaskId,
    pub workspace: String,
    pub repository_state: CommitRepositoryState,
    pub reported_at: i64,
    pub commits: Vec<TaskCommit>,
}
