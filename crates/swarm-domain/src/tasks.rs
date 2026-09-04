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
    /// Finished and accepted, waiting only for the work to ship.
    ///
    /// A no-deployment exemption means "this ships nothing, EVER". An open pull
    /// request means "this ships LATER". Conflating them is what closed work on
    /// a false claim: the reason given was true about deployment and wrong as
    /// grounds to close, because the work was not finished-with-nothing-to-ship,
    /// it was finished-and-waiting.
    ///
    /// Like [`TaskState::Abandoned`], this removes a question rather than making
    /// it cheaper. Completed asks what evidence shows the work is running, and
    /// for work whose commits have not landed yet that question has no honest
    /// answer — so it was being answered with an exemption somebody then had to
    /// approve.
    ///
    /// It settles itself. Nothing waits on a person: when the commits reach a
    /// ref and a deployment is recorded, the task completes on that evidence.
    AwaitingRelease,
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
                | (
                    Self::Review,
                    Self::Active | Self::Ready | Self::AwaitingRelease | Self::Completed
                )
                // AWAITING RELEASE IS A RESTING STATE, NOT A TERMINAL ONE. It
                // settles itself into Completed when the commits land, and it
                // falls back to Active because a release can reveal the work was
                // not finished after all.
                | (Self::AwaitingRelease, Self::Active | Self::Completed)
                // ABANDONED IS REACHABLE FROM EVERY UNFINISHED STATE, including
                // Draft. Forcing a detour through Blocked to abandon something
                // would be clicking, which is the thing this state exists to
                // delete. Draft is included because a draft Queen decides
                // against is a decision worth keeping: `remove_task` disposes
                // of mistakes and duplicates and destroys the record, which is
                // a different act from declining work on purpose.
                | (
                    Self::Draft
                        | Self::Ready
                        | Self::Active
                        | Self::Blocked
                        | Self::Review
                        | Self::AwaitingRelease,
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
            Self::AwaitingRelease => "awaiting_release",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
        };
        formatter.write_str(value)
    }
}

/// Why Queen is contacting a worker through the governed task channel.
///
/// The distinction is an admission rule, not presentation metadata. Ordinary
/// questions stay with the task's current assignee. The one deliberate
/// exception is a second opinion from the managed Scout, whose availability is
/// checked before the message is recorded and again by guarded delivery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerMessagePurpose {
    #[default]
    AssignedTask,
    ScoutSecondOpinion,
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
            "awaiting_release" => Ok(Self::AwaitingRelease),
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

/// Who owes the next move on a task.
///
/// Every waiting state answers the same question — who is holding this up —
/// and the board could not answer it. That is one gap behind three symptoms:
/// a queue nobody could attribute, thirty blocked tasks reading as one
/// undifferentiated pile, and the operator's attention surface filling with
/// other actors' backlogs.
///
/// DERIVED, NOT STORED, except for the one case that is genuinely a decision:
/// Queen handing reviewed work back to its worker. Everything else follows
/// from the state and the assignment, and deriving it means it cannot drift
/// away from them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextMoveOwner {
    /// The assigned worker. It is theirs to progress or to answer.
    Worker,
    /// Queen: unassigned work to route, or finished work to judge.
    Queen,
    /// Nothing in the Hive can move this — a hard block, by design.
    ///
    /// Distinct from Queen owing a move on blocked work. The operator drew the
    /// line: blocked is "a harder reason than back and forth with worker or
    /// queen", such as a task waiting on another task.
    Blocked,
    /// The operator, because a decision they have not answered is open on it.
    ///
    /// REVIEWED WORK WAITING ON A RULING HAD NO HONEST OWNER. It sat as Queen's
    /// to judge while Queen was the one who could not judge it — she was waiting
    /// too. Twice in one day she tried to move such a task to Blocked and could
    /// not: Review has no Blocked exit, and the detour through Active is refused
    /// whenever the assignee holds any other task.
    ///
    /// DERIVED FROM THE DECISION, NOT STORED. An unresolved decision request
    /// against the task already IS the fact, so nothing new is written down and
    /// the two cannot drift. It also means this UNSETS ITSELF: the moment the
    /// operator answers, ownership returns to Queen with no transition, no
    /// second trip through Active, and nothing for anyone to remember.
    Operator,
    /// An event, not a person: the work is waiting to ship and settles itself.
    Release,
    /// Nobody. The work is closed.
    Nobody,
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
    /// Who owes the next move. See [`NextMoveOwner`].
    ///
    /// Computed on read like `deployment_recorded`, so it can never disagree
    /// with the state and assignment it is derived from.
    #[serde(default = "default_next_move_owner")]
    pub next_move_owner: NextMoveOwner,
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

/// What a task's commit report entitles a deterministic pass to conclude.
///
/// A POLICY, kept pure and in one place so it can be read and argued with.
/// Everything that acts on it — the coordinator that closes work, the refusal
/// that catches a contradicted claim — reads the same function, so the two can
/// never disagree about the same task.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitSettlement {
    /// The worker reported, and reported nothing. There is nothing to deploy.
    NothingBuilt,
    /// Everything reported is documentation, checked and reachable.
    DocumentationOnly,
    /// At least one reachable commit touches something that is not
    /// documentation. A claim of "nothing to deploy" contradicts this.
    BuiltCode,
    /// ⚠️ NOBODY REPORTED AT ALL. Distinct from `Unestablished`, and the split
    /// is the whole point of this enum's 2026-09-04 revision.
    ///
    /// `TaskCommitReport`'s own doc comment above says "THE ROW EXISTING IS
    /// ITSELF THE FACT... NO REPORT AT ALL is nobody having said anything".
    /// This arm honours that. Folded together with `Unestablished`, as it was,
    /// the incentive ran backwards: reporting your commits was the ONLY way to
    /// get a no-deployment claim refused, and saying nothing passed. Two
    /// workers made the same comment-only `.ts` change minutes apart; the one
    /// who reported was refused and the one who stayed silent was approved.
    ///
    /// A worker with nothing built is one call from `NothingBuilt` — an empty
    /// list is a documented answer — so refusing here costs honesty nothing.
    NotReported,
    /// Reported, and the report does not settle the question: a commit nobody
    /// could check, or one whose paths came back empty. NOT a synonym for
    /// `NothingBuilt`, and no automatic close may rest on it.
    ///
    /// A CLAIM IS STILL ALLOWED ON THIS. It is the case that keeps the refusal
    /// narrow — a workspace that is not a checkout can never establish
    /// anything, and refusing here would block those workers permanently.
    Unestablished,
}

/// Whether one path is documentation.
///
/// Deliberately narrow, and it fails toward asking a person. A path this does
/// not recognise is treated as code, which costs one human decision; the
/// opposite error closes real shipped work without anyone seeing it.
#[must_use]
pub fn documentation_path(path: &str) -> bool {
    let path = path.trim();
    if path.is_empty() {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("docs/") || lower.contains("/docs/") {
        return true;
    }
    // Through `Path` rather than a suffix match: the string form is
    // case-sensitive and a path is not a string, which is the distinction
    // clippy's lint is about. `lower` is already folded, so this only has to be
    // right about where the extension ends.
    let extension = std::path::Path::new(&lower)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    if matches!(extension, "md" | "txt") {
        return true;
    }
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = basename.split('.').next().unwrap_or(basename);
    matches!(
        stem,
        "readme" | "license" | "licence" | "changelog" | "notice"
    )
}

/// Reads a report as a settlement, or `Unknown` when it does not establish one.
///
/// THE EMPTY-PATH CASE IS `Unknown`, NOT DOCUMENTATION. A commit that reports no
/// paths at all is most often a MERGE, which `--name-only` summarises as
/// nothing — so "every path is documentation" is vacuously true of a commit
/// that may carry the entire release. Vacuous truth is exactly how a check ends
/// up confidently answering a question it never asked.
#[must_use]
pub fn commit_settlement(report: Option<&TaskCommitReport>) -> CommitSettlement {
    let Some(report) = report else {
        return CommitSettlement::NotReported;
    };
    if report.commits.is_empty() {
        return CommitSettlement::NothingBuilt;
    }
    // Anything not checked leaves the whole report unsettled: a report is read
    // as a set, and one commit nobody could look at is enough to mean the set
    // has not been established.
    if report
        .commits
        .iter()
        .any(|commit| commit.verdict != CommitVerdict::Present)
    {
        return CommitSettlement::Unestablished;
    }
    if report
        .commits
        .iter()
        .any(|commit| commit.changed_paths.is_empty())
    {
        return CommitSettlement::Unestablished;
    }
    if report
        .commits
        .iter()
        .flat_map(|commit| commit.changed_paths.iter())
        .all(|path| documentation_path(path))
    {
        CommitSettlement::DocumentationOnly
    } else {
        CommitSettlement::BuiltCode
    }
}

/// Older peers omit the field; unknown ownership reads as nobody's move rather
/// than inventing an owner that a reader might act on.
const fn default_next_move_owner() -> NextMoveOwner {
    NextMoveOwner::Nobody
}

impl NextMoveOwner {
    /// Derives who owes the next move from the facts already on the task.
    ///
    /// `review_returned` is the one stored input: Queen has handed reviewed
    /// work back and named what is missing, so the worker owes an answer. It is
    /// stored because it is a decision somebody made, not a consequence of the
    /// state — everything else here follows from state and assignment, and
    /// deriving those means they cannot drift apart.
    ///
    /// `awaiting_operator_decision` is NOT a second stored input. It is read
    /// from whether an unresolved decision request names this task, which is a
    /// fact the board already holds. Storing it again would create exactly the
    /// drift this function exists to prevent.
    #[must_use]
    pub const fn derive(
        state: TaskState,
        assigned: bool,
        review_returned: bool,
        awaiting_operator_decision: bool,
    ) -> Self {
        match state {
            TaskState::Active => Self::Worker,
            // Assigned work is the worker's to start, so "N tasks ready" was
            // never the useful number: half of it was never waiting on Queen.
            TaskState::Ready if assigned => Self::Worker,
            // Queen has handed this back and named what is missing. The task
            // did not move; the debt did.
            TaskState::Review if review_returned => Self::Worker,
            // AFTER the hand-back, because that is an explicit act by Queen
            // naming what the worker owes, and a decision left open beside it
            // does not cancel the debt she named.
            TaskState::Review if awaiting_operator_decision => Self::Operator,
            // AFTER the hand-back, because that is an explicit act by Queen
            // naming what the worker owes, and a decision left open beside it
            // does not cancel the debt she named.
            // NOT Queen. The operator drew this line: blocked is a harder
            // reason than back-and-forth, such as a task waiting on another
            // task. Naming Queen here would bury the hard cases in her queue.
            TaskState::Blocked => Self::Blocked,
            // An event, not a person. It settles itself when the work ships.
            TaskState::AwaitingRelease => Self::Release,
            TaskState::Completed | TaskState::Abandoned => Self::Nobody,
            // Everything left is Queen's: unfiled work to ready, unassigned
            // work to route, finished work to judge.
            TaskState::Draft | TaskState::Ready | TaskState::Review => Self::Queen,
        }
    }
}
