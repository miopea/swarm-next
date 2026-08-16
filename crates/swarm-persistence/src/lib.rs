use std::{
    collections::HashSet,
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    Apiary, ApiaryId, ApiaryMemberSummary, ControlRoomEventKind, Hive, HiveId, HiveIdentity,
    LocalApiaryContext, LocalApiaryRole, Operator, OperatorId, SharedWorkBackend,
    StewardCapability, Stewardship, StewardshipId, Task, TaskActivity, TaskActivityActor,
    TaskActivityActorKind, TaskActivityKind, TaskActivityPage, TaskDetailsUpdate,
    TaskDispatchState, TaskId, TaskOutcomeDeliveryState, TaskPriority, TaskState, WorkerId,
    WorkerSessionId,
};
use thiserror::Error;
use uuid::Uuid;

mod apiary;
mod decisions;
mod email;
mod events;
mod federation;
mod federation_jira_claims;
mod federation_stewardships;
mod federation_tasks;
pub use federation_tasks::MAX_FEDERATION_TASK_COMMAND_BATCH;
mod feedback;
pub use federation::{
    MAX_CONNECTION_CARD_LIFETIME_SECONDS, MAX_FEDERATION_INVITATION_LIFETIME_SECONDS,
    MIN_CONNECTION_CARD_LIFETIME_SECONDS, MIN_FEDERATION_INVITATION_LIFETIME_SECONDS,
    verify_apiary_invitation_envelope, verify_federation_catalog_snapshot,
    verify_federation_membership_receipt, verify_hive_connection_card,
};
pub use federation_jira_claims::{
    FederationJiraClaimIntent, FederationJiraClaimPhase, MAX_FEDERATION_JIRA_CLAIM_BATCH,
};
mod jira;
pub use feedback::{DogfoodReport, MAX_DOGFOOD_REPORTS};
pub use jira::{
    JiraCommentDispatch, JiraIssueSnapshot, JiraProjectBindingInput, JiraTransitionDispatch,
    JiraTransitionFailure,
};
mod presence;
pub use decisions::{DecisionDeliveryFailure, DecisionDispatch, NewDecisionRequest};
pub use email::{
    EmailAttachmentSnapshot, EmailImport, EmailMessageSnapshot, EmailReplyDispatch,
    EmailReplyFailure, EmailReplyState, EmailReplyTarget, EmailReplyTargetDispatch,
    EmailTaskAttachment, EmailTaskDraft, EmailTaskLink, TaskDeploymentRecord,
};
pub use presence::PresenceMutation;
mod notifications;
pub use notifications::{
    NotificationDeliveryFailure, NotificationDispatch, NotificationSettings, PushSubscriptionInput,
    VapidKeyMaterial,
};
mod orchestration;
mod presentation;
pub use presentation::{PresentationColorTheme, PresentationDeviceClass, PresentationPreferences};
mod task_dispatches;
pub use task_dispatches::{TaskDispatch, TaskDispatchFailure};
mod task_outcomes;
pub use task_outcomes::{TaskOutcomeDispatch, TaskOutcomeFailure};
mod workers;
use events::insert_control_room_event;
#[cfg(test)]
use events::{MAX_CONTROL_ROOM_EVENT_PAGE, MAX_CONTROL_ROOM_EVENTS};
const MAX_TASK_TITLE_BYTES: usize = 240;
const MAX_TASK_DESCRIPTION_BYTES: usize = 10_000;
const MAX_PUBLIC_IDENTITY_NAME_BYTES: usize = 120;
pub const MAX_TASK_ACTIVITY_NOTE_BYTES: usize = 4_000;
const MAX_WORKSPACE_BYTES: usize = 4096;
const CURRENT_SCHEMA_VERSION: i64 = 52;
pub const MAX_TASK_ACTIVITY_PAGE: usize = 100;
pub const MAX_OPEN_TASKS_PER_ORDER: usize = 1_000;

pub(crate) fn normalize_public_identity_name(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= MAX_PUBLIC_IDENTITY_NAME_BYTES
        && !value.chars().any(char::is_control))
    .then_some(value)
}

#[derive(Clone)]
pub struct TaskStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug, Error)]
pub enum TaskStoreError {
    #[error("task persistence filesystem failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("task persistence failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("task persistence lock was poisoned")]
    LockPoisoned,
    #[error("Apiary invitation is invalid")]
    InvalidApiaryInvitation,
    #[error("Apiary configuration is invalid")]
    InvalidApiary,
    #[error("Hive identity is invalid")]
    InvalidHiveIdentity,
    #[error("this Hive must be personal before it can found an Apiary")]
    ApiaryMembershipConflict,
    #[error("Apiary was not found")]
    ApiaryNotFound,
    #[error("Apiary invitation was not found")]
    ApiaryInvitationNotFound,
    #[error("Apiary invitation is no longer pending")]
    ApiaryInvitationResolved,
    #[error("Apiary join readiness is incomplete")]
    ApiaryJoinNotReady,
    #[error("Apiary cannot collapse until all federation state is clear")]
    ApiaryCollapseNotReady,
    #[error("Jira project is not ready for Apiary promotion")]
    ApiaryProjectPromotionNotReady,
    #[error("Hive connection card is invalid or expired")]
    InvalidFederationConnectionCard,
    #[error("the local federation identity is corrupt")]
    InvalidFederationIdentity,
    #[error("secure entropy is unavailable for the local federation identity")]
    FederationEntropyUnavailable,
    #[error("Only the active Apiary Keeper can pin Hive identities")]
    ApiaryKeeperRequired,
    #[error("The Stewardship scope or capabilities are invalid")]
    InvalidStewardship,
    #[error("The Stewardship was not found")]
    StewardshipNotFound,
    #[error("The Hive identity conflicts with a previously pinned key")]
    HiveCandidateIdentityConflict,
    #[error("The pinned Hive identity was not found")]
    HiveCandidateNotFound,
    #[error("The Apiary invitation envelope is invalid or expired")]
    InvalidFederationInvitation,
    #[error("The Apiary join link is invalid or expired")]
    InvalidApiaryJoinLink,
    #[error("The Apiary join link was not found")]
    ApiaryJoinLinkNotFound,
    #[error("The Apiary join link cannot accept that transition")]
    ApiaryJoinLinkResolved,
    #[error("This Apiary already has the maximum number of active join links")]
    ApiaryJoinLinkLimit,
    #[error("The federation node credential is invalid or expired")]
    InvalidFederationCredential,
    #[error("The federation project catalog is invalid, stale, or misaddressed")]
    InvalidFederationCatalog,
    #[error("The federation shared-work claim is invalid")]
    InvalidFederationClaim,
    #[error("The local federation synchronization state is invalid")]
    InvalidFederationSync,
    #[error("The federated Jira claim state is invalid")]
    InvalidFederationJiraClaim,
    #[error("This Hive already has the maximum number of pending federated Jira claims")]
    FederationJiraClaimQueueFull,
    #[error("The Apiary task or task feed is invalid")]
    InvalidFederationTask,
    #[error("The Jira issue is already claimed by another Hive")]
    FederationClaimConflict,
    #[error("A current invitation already exists for this pinned Hive")]
    FederationInvitationConflict,
    #[error("task was not found")]
    NotFound,
    #[error("decision request was not found")]
    DecisionNotFound,
    #[error("decision request content is invalid")]
    InvalidDecisionContent,
    #[error("decision request must offer 1 to 6 unique actions")]
    InvalidDecisionActions,
    #[error("decision request deadline is invalid")]
    InvalidDecisionDeadline,
    #[error("decision request is already resolved")]
    DecisionAlreadyResolved,
    #[error("decision resolution must use one of the allowed actions")]
    InvalidDecisionResolution,
    #[error("this Hive already has the maximum number of pending decisions")]
    DecisionInboxFull,
    #[error("this Hive already has the maximum number of pending task briefings")]
    TaskDispatchQueueFull,
    #[error("this Hive already tracks the maximum of 16 presence devices")]
    PresenceDeviceLimit,
    #[error("notification subscription material is invalid")]
    InvalidNotificationSubscription,
    #[error("this Hive already has the maximum of 8 notification subscriptions")]
    NotificationSubscriptionLimit,
    #[error("the bounded notification delivery queue is full")]
    NotificationQueueFull,
    #[error("the installation notification signing key is invalid")]
    InvalidVapidKey,
    #[error("task handoff note must not exceed {MAX_TASK_ACTIVITY_NOTE_BYTES} bytes")]
    InvalidTaskActivityNote,
    #[error("this Hive already has the maximum number of pending Queen handoffs")]
    TaskOutcomeQueueFull,
    #[error("Jira comment content is invalid")]
    InvalidJiraComment,
    #[error("this Hive already has the maximum number of pending Jira comments")]
    JiraCommentQueueFull,
    #[error("email message metadata or content is invalid")]
    InvalidEmailMessage,
    #[error("selected email messages belong to different existing tasks")]
    EmailMergeConflict,
    #[error("email attachment metadata exceeds its private bounds")]
    InvalidEmailAttachment,
    #[error("email source was not found")]
    EmailSourceNotFound,
    #[error("task deployment evidence is invalid")]
    InvalidTaskDeployment,
    #[error("email resolution reply content is invalid")]
    InvalidEmailReply,
    #[error("email resolution replies require completed and deployed work")]
    EmailReplyNotReady,
    #[error("this task already has an email resolution reply")]
    EmailReplyAlreadyExists,
    #[error("this Hive already has the maximum number of pending email replies")]
    EmailReplyQueueFull,
    #[error("task title must contain 1 to {MAX_TASK_TITLE_BYTES} bytes")]
    InvalidTitle,
    #[error("task description must not exceed {MAX_TASK_DESCRIPTION_BYTES} bytes")]
    InvalidDescription,
    #[error("task details update must contain at least one field")]
    EmptyTaskDetailsUpdate,
    #[error("workspace must contain 1 to {MAX_WORKSPACE_BYTES} bytes")]
    InvalidWorkspace,
    #[error("task cannot move from {from} to {to}")]
    InvalidTransition { from: TaskState, to: TaskState },
    #[error("completed tasks cannot be assigned")]
    CompletedTask,
    #[error("worker was not found")]
    WorkerNotFound,
    #[error("worker name is invalid")]
    InvalidWorkerName,
    #[error("worker name already exists")]
    DuplicateWorkerName,
    #[error("worker description must not exceed 2000 bytes or contain control characters")]
    InvalidWorkerDescription,
    #[error("worker update must contain a name, description, provider, or startup preference")]
    EmptyWorkerUpdate,
    #[error("the Queen profile is managed by Swarm and cannot be edited")]
    QueenProfileImmutable,
    #[error("Scout is a managed Hive worker and cannot be renamed or removed")]
    ScoutIdentityImmutable,
    #[error("the Queen profile already exists")]
    QueenAlreadyExists,
    #[error("worker already has an active session")]
    WorkerAlreadyRunning,
    #[error("the worker must be sleeping before changing provider or removing it")]
    WorkerMustBeSleeping,
    #[error("reassign or complete this worker's open tasks before removing it")]
    WorkerOwnsOpenTasks,
    #[error("agent credential digest must be exactly 32 bytes")]
    InvalidAgentCredentialDigest,
    #[error("worker session is not active")]
    WorkerSessionNotActive,
    #[error("provider conversation cannot be assigned after worker history exists")]
    ProviderConversationUnavailable,
    #[error("task order must contain every open task exactly once")]
    InvalidTaskOrder,
    #[error("dogfood report notes and evidence are missing or exceed their private bounds")]
    InvalidDogfoodReport,
    #[error("dogfood report attachment identity is invalid")]
    InvalidDogfoodAttachment,
    #[error("dogfood report limit must be from 1 through 50")]
    InvalidDogfoodReportLimit,
    #[error("Jira project metadata is invalid")]
    InvalidJiraProject,
    #[error("Jira workflow mapping is invalid")]
    InvalidJiraWorkflowMapping,
    #[error("Jira project binding was not found")]
    JiraProjectBindingNotFound,
    #[error("this Jira task already has an outbound workflow update pending")]
    JiraTransitionPending,
    #[error("this Hive already has the maximum number of pending Jira updates")]
    JiraTransitionQueueFull,
    #[error("worker order must contain every operator-ordered worker exactly once")]
    InvalidWorkerOrder,
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("database integrity check failed: {0}")]
    IntegrityFailure(String),
}

impl TaskStore {
    /// Opens, migrates, and integrity-checks a file-backed task database.
    ///
    /// # Errors
    /// Returns an error when the path, schema, migration, or integrity check is invalid.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    /// Opens a migrated in-memory store for isolated tests and ephemeral runtimes.
    ///
    /// # Errors
    /// Returns an error when `SQLite` initialization or migration fails.
    pub fn in_memory() -> Result<Self, TaskStoreError> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, TaskStoreError> {
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match schema_version {
            found if found < CURRENT_SCHEMA_VERSION => {
                let transaction = connection.transaction()?;
                if schema_version == 0 {
                    transaction.execute_batch(
                        "
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                workspace TEXT NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed')),
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE IF NOT EXISTS task_assignments (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                worker_session_id TEXT NOT NULL,
                assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                released_at INTEGER
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_active_assignment_per_task
                ON task_assignments(task_id) WHERE released_at IS NULL;
            CREATE TABLE IF NOT EXISTS task_activity (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                from_state TEXT,
                to_state TEXT,
                occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            ",
                    )?;
                }
                migrate_schema(&transaction, schema_version)?;
                transaction.commit()?;
            }
            CURRENT_SCHEMA_VERSION => {}
            found => {
                return Err(TaskStoreError::UnsupportedSchemaVersion {
                    found,
                    supported: CURRENT_SCHEMA_VERSION,
                });
            }
        }
        let integrity: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if integrity != "ok" {
            return Err(TaskStoreError::IntegrityFailure(integrity));
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Returns the durable operator and Hive owned by this local installation.
    ///
    /// # Errors
    /// Returns an error when identity persistence is unavailable or invalid.
    pub fn local_hive_identity(&self) -> Result<HiveIdentity, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT o.id, o.display_name, h.id, h.name, h.apiary_id
                FROM local_hive_identity l
                JOIN hives h ON h.id = l.hive_id
                JOIN operators o ON o.id = h.operator_id
                WHERE l.singleton = 1
                ",
                [],
                |row| {
                    let operator_id = parse_domain_id::<OperatorId>(&row.get::<_, String>(0)?)?;
                    let hive_id = parse_domain_id::<HiveId>(&row.get::<_, String>(2)?)?;
                    let apiary_id = row
                        .get::<_, Option<String>>(4)?
                        .map(|value| parse_domain_id::<ApiaryId>(&value))
                        .transpose()?;
                    Ok(HiveIdentity {
                        operator: Operator {
                            id: operator_id,
                            display_name: row.get(1)?,
                        },
                        hive: Hive {
                            id: hive_id,
                            name: row.get(3)?,
                            operator_id,
                            apiary_id,
                        },
                    })
                },
            )
            .map_err(TaskStoreError::from)
    }

    /// Renames only the Hive owned by this installation. Membership, operator,
    /// federation keys, workers, tasks, and repositories are unchanged.
    ///
    /// # Errors
    /// Rejects blank, oversized, control-character, or invalid-time input and
    /// unavailable persistence.
    pub fn rename_local_hive(&self, name: &str, now: i64) -> Result<HiveIdentity, TaskStoreError> {
        let name =
            normalize_public_identity_name(name).ok_or(TaskStoreError::InvalidHiveIdentity)?;
        if now < 0 {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if transaction.execute(
            "UPDATE hives SET name = ?1, updated_at = ?2
             WHERE id = ?3 AND operator_id = ?4",
            params![
                name,
                now,
                identity.hive.id.to_string(),
                identity.operator.id.to_string()
            ],
        )? != 1
        {
            return Err(TaskStoreError::InvalidHiveIdentity);
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        drop(connection);
        self.local_hive_identity()
    }

    /// Returns the local Hive's optional federation without inferring any
    /// Steward authority that has not been durably granted.
    ///
    /// # Errors
    /// Returns an error when identity or Apiary persistence is unavailable or invalid.
    pub fn local_apiary_context(&self) -> Result<LocalApiaryContext, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT a.id, a.name, a.keeper_operator_id, a.shared_work_backend,
                       h.operator_id, a.policy_revision
                FROM local_hive_identity l
                JOIN hives h ON h.id = l.hive_id
                LEFT JOIN apiaries a ON a.id = h.apiary_id
                WHERE l.singleton = 1
                ",
                [],
                |row| {
                    let Some(apiary_id) = row.get::<_, Option<String>>(0)? else {
                        return Ok(LocalApiaryContext::Personal);
                    };
                    let apiary_id = parse_domain_id::<ApiaryId>(&apiary_id)?;
                    let keeper_operator_id =
                        parse_domain_id::<OperatorId>(&row.get::<_, String>(2)?)?;
                    let local_operator_id =
                        parse_domain_id::<OperatorId>(&row.get::<_, String>(4)?)?;
                    let backend = row
                        .get::<_, String>(3)?
                        .parse::<SharedWorkBackend>()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(LocalApiaryContext::Federated {
                        apiary: Apiary::persisted(
                            apiary_id,
                            row.get::<_, String>(1)?,
                            keeper_operator_id,
                            backend,
                            row.get::<_, u64>(5)?,
                        ),
                        local_role: if keeper_operator_id == local_operator_id {
                            LocalApiaryRole::Keeper
                        } else {
                            LocalApiaryRole::Member
                        },
                    })
                },
            )
            .map_err(TaskStoreError::from)
    }

    /// Lists the durable public identities in the local Apiary view. Both a
    /// Keeper and a joined member can inspect this roster; private federation
    /// material never leaves its dedicated tables.
    ///
    /// # Errors
    /// Rejects personal Hives and invalid or unavailable persistence.
    pub fn list_apiary_members(&self) -> Result<Vec<ApiaryMemberSummary>, TaskStoreError> {
        let LocalApiaryContext::Federated { apiary, .. } = self.local_apiary_context()? else {
            return Err(TaskStoreError::InvalidApiary);
        };
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT h.id, h.name, o.id, o.display_name
             FROM hives h
             JOIN operators o ON o.id = h.operator_id
             WHERE h.apiary_id = ?1
             ORDER BY CASE WHEN o.id = ?2 THEN 0 ELSE 1 END, lower(h.name), h.id",
        )?;
        let rows = statement.query_map(
            [apiary.id.to_string(), apiary.keeper_operator_id.to_string()],
            |row| {
                let hive_id = parse_domain_id::<HiveId>(&row.get::<_, String>(0)?)?;
                let operator_id = parse_domain_id::<OperatorId>(&row.get::<_, String>(2)?)?;
                Ok(ApiaryMemberSummary {
                    hive_id,
                    hive_name: row.get(1)?,
                    operator_id,
                    operator_display_name: row.get(3)?,
                    role: if operator_id == apiary.keeper_operator_id {
                        LocalApiaryRole::Keeper
                    } else {
                        LocalApiaryRole::Member
                    },
                    is_local: hive_id == identity.hive.id,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Loads only active, explicitly persisted Steward grants for one Apiary.
    /// Missing grants return an empty set and never imply authority.
    ///
    /// # Errors
    /// Returns an error when persisted identifiers or capabilities are invalid.
    pub fn stewardships_for_apiary(
        &self,
        apiary_id: ApiaryId,
    ) -> Result<Vec<Stewardship>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, steward_operator_id
             FROM stewardships
             WHERE apiary_id = ?1 AND revoked_at IS NULL
             ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([apiary_id.to_string()], |row| {
            Ok((
                parse_domain_id::<StewardshipId>(&row.get::<_, String>(0)?)?,
                parse_domain_id::<OperatorId>(&row.get::<_, String>(1)?)?,
            ))
        })?;
        let grants = rows.collect::<Result<Vec<_>, _>>()?;
        grants
            .into_iter()
            .map(|(id, steward_operator_id)| {
                let managed_hive_ids = {
                    let mut statement = connection.prepare(
                        "SELECT hive_id FROM stewardship_hive_grants
                         WHERE stewardship_id = ?1 ORDER BY hive_id",
                    )?;
                    statement
                        .query_map([id.to_string()], |row| {
                            parse_domain_id::<HiveId>(&row.get::<_, String>(0)?)
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                let capabilities = {
                    let mut statement = connection.prepare(
                        "SELECT capability FROM stewardship_capability_grants
                         WHERE stewardship_id = ?1 ORDER BY capability",
                    )?;
                    statement
                        .query_map([id.to_string()], |row| {
                            row.get::<_, String>(0)?
                                .parse::<StewardCapability>()
                                .map_err(|_| rusqlite::Error::InvalidQuery)
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                Ok(Stewardship {
                    id,
                    apiary_id,
                    steward_operator_id,
                    managed_hive_ids,
                    capabilities,
                })
            })
            .collect()
    }

    /// Creates a validated draft and its first activity event atomically.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task(&self, title: &str, workspace: &str) -> Result<Task, TaskStoreError> {
        self.create_task_with_details(title, "", TaskPriority::Normal, workspace)
    }

    /// Creates a validated draft with operator-facing context and priority.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task_with_details(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
    ) -> Result<Task, TaskStoreError> {
        self.create_task_with_details_as(
            title,
            description,
            priority,
            workspace,
            &TaskActivityActor::system(),
        )
    }

    /// Creates a validated draft and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn create_task_with_details_as(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        workspace: &str,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let title = title.trim();
        let description = description.trim();
        let workspace = workspace.trim();
        validate_text(title, workspace)?;
        validate_description(description)?;
        let id = TaskId::new();
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO tasks (id, hive_id, title, description, priority, workspace, state, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'draft',
                     COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
            params![
                id.to_string(),
                hive_id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, actor_kind, actor_id)
             VALUES (?1, 'created', 'draft', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Returns an open task to the Hive queue without stopping its former worker.
    ///
    /// # Errors
    ///
    /// Returns an error when the task does not exist, is already completed, or
    /// the unassignment transaction cannot be committed.
    pub fn unassign_task(&self, id: TaskId) -> Result<Task, TaskStoreError> {
        self.unassign_task_as(id, &TaskActivityActor::system())
    }

    /// Returns an open task to the Hive queue and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for an unknown or completed task or unavailable persistence.
    pub fn unassign_task_as(
        &self,
        id: TaskId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let state = state.ok_or(TaskStoreError::NotFound)?;
        if state == TaskState::Completed.to_string() {
            return Err(TaskStoreError::CompletedTask);
        }
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments WHERE task_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE tasks SET assigned_worker_id = NULL, updated_at = unixepoch() WHERE id = ?1",
            [id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'unassigned', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Lists tasks with their current active assignment.
    ///
    /// # Errors
    /// Returns an error when persistence cannot be read safely.
    pub fn list_tasks(&self) -> Result<Vec<Task>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "
            SELECT t.id, t.hive_id, t.title, t.description, t.priority, t.workspace, t.state,
                   t.assigned_worker_id, a.worker_session_id,
                   (SELECT state FROM task_dispatches td WHERE td.assignment_id = a.id),
                   (SELECT state FROM task_outcome_deliveries outcome WHERE outcome.task_id = t.id
                    AND outcome.target_state = t.state
                    ORDER BY outcome.activity_sequence DESC LIMIT 1),
                   t.position, t.created_at, t.updated_at
            FROM tasks t
            LEFT JOIN task_assignments a
              ON a.task_id = t.id AND a.released_at IS NULL
            ORDER BY CASE t.state WHEN 'completed' THEN 1 ELSE 0 END,
                     CASE t.state WHEN 'completed' THEN -t.updated_at ELSE t.position END,
                     t.id
            ",
        )?;
        statement
            .query_map([], task_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Loads one task and its current active assignment.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown task or a persistence error.
    pub fn get_task(&self, id: TaskId) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "
                SELECT t.id, t.hive_id, t.title, t.description, t.priority, t.workspace, t.state,
                       t.assigned_worker_id, a.worker_session_id,
                   (SELECT state FROM task_dispatches td WHERE td.assignment_id = a.id),
                   (SELECT state FROM task_outcome_deliveries outcome WHERE outcome.task_id = t.id
                    AND outcome.target_state = t.state
                    ORDER BY outcome.activity_sequence DESC LIMIT 1),
                       t.position, t.created_at, t.updated_at
                FROM tasks t
                LEFT JOIN task_assignments a
                  ON a.task_id = t.id AND a.released_at IS NULL
                WHERE t.id = ?1
                ",
                [id.to_string()],
                task_from_row,
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)
    }

    /// Lists a bounded, chronological activity history for one task.
    ///
    /// # Errors
    /// Returns `NotFound` for an unknown task or a persistence error.
    pub fn list_task_activity(
        &self,
        id: TaskId,
        limit: usize,
    ) -> Result<TaskActivityPage, TaskStoreError> {
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM tasks WHERE id = ?1",
                [id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        let limit = limit.clamp(1, MAX_TASK_ACTIVITY_PAGE);
        let query_limit = i64::try_from(limit.saturating_add(1))
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT sequence, task_id, kind, from_state, to_state, note, occurred_at,
                    actor_kind, actor_id
             FROM task_activity WHERE task_id = ?1
             ORDER BY sequence DESC LIMIT ?2",
        )?;
        let mut activity = statement
            .query_map(params![id.to_string(), query_limit], task_activity_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let truncated = activity.len() > limit;
        activity.truncate(limit);
        activity.reverse();
        Ok(TaskActivityPage {
            events: activity,
            truncated,
        })
    }

    /// Lists the newest durable task events across the local Hive.
    ///
    /// # Errors
    /// Returns a persistence error when the local Hive identity or activity rows
    /// cannot be read.
    pub fn list_recent_task_activity(
        &self,
        limit: usize,
    ) -> Result<TaskActivityPage, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let connection = self.connection()?;
        let limit = limit.clamp(1, MAX_TASK_ACTIVITY_PAGE);
        let query_limit = i64::try_from(limit.saturating_add(1))
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let mut statement = connection.prepare(
            "SELECT activity.sequence, activity.task_id, activity.kind,
                    activity.from_state, activity.to_state, activity.note,
                    activity.occurred_at, activity.actor_kind, activity.actor_id
             FROM task_activity activity
             JOIN tasks task ON task.id = activity.task_id
             WHERE task.hive_id = ?1
             ORDER BY activity.sequence DESC LIMIT ?2",
        )?;
        let mut activity = statement
            .query_map(
                params![hive_id.to_string(), query_limit],
                task_activity_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let truncated = activity.len() > limit;
        activity.truncate(limit);
        activity.reverse();
        Ok(TaskActivityPage {
            events: activity,
            truncated,
        })
    }

    /// Replaces the complete open-task order for the local Hive atomically.
    ///
    /// # Errors
    /// Rejects incomplete, duplicate, oversized, foreign-Hive, or completed-task input.
    pub fn reorder_open_tasks(&self, task_ids: &[TaskId]) -> Result<Vec<Task>, TaskStoreError> {
        if task_ids.len() > MAX_OPEN_TASKS_PER_ORDER {
            return Err(TaskStoreError::InvalidTaskOrder);
        }
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let expected = {
            let mut statement = transaction.prepare(
                "SELECT id FROM tasks
                 WHERE hive_id = ?1 AND state != 'completed'
                 ORDER BY position, id",
            )?;
            statement
                .query_map([hive_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let supplied = task_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        let unique = supplied.iter().collect::<HashSet<_>>();
        let expected_set = expected.iter().collect::<HashSet<_>>();
        if supplied.len() != expected.len()
            || unique.len() != supplied.len()
            || unique != expected_set
        {
            return Err(TaskStoreError::InvalidTaskOrder);
        }
        for (position, task_id) in supplied.iter().enumerate() {
            let position = i64::try_from(position)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
            transaction.execute(
                "UPDATE tasks SET position = ?2, updated_at = unixepoch() WHERE id = ?1",
                params![task_id, position],
            )?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.list_tasks()
    }

    /// Replaces the supplied task details and records one atomic activity event.
    ///
    /// # Errors
    /// Returns an error for an empty update, invalid content, an unknown task, or unavailable persistence.
    pub fn update_task_details(
        &self,
        id: TaskId,
        update: &TaskDetailsUpdate,
    ) -> Result<Task, TaskStoreError> {
        self.update_task_details_as(id, update, &TaskActivityActor::system())
    }

    /// Replaces supplied task details and records their authenticated origin.
    ///
    /// # Errors
    /// Returns an error for an invalid or empty update, unknown task, or unavailable persistence.
    pub fn update_task_details_as(
        &self,
        id: TaskId,
        update: &TaskDetailsUpdate,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        if update.title.is_none()
            && update.description.is_none()
            && update.priority.is_none()
            && update.workspace.is_none()
        {
            return Err(TaskStoreError::EmptyTaskDetailsUpdate);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = transaction
            .query_row(
                "SELECT title, description, priority, workspace FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        let title = update
            .title
            .as_deref()
            .map_or(current.0.as_str(), str::trim);
        let description = update
            .description
            .as_deref()
            .map_or(current.1.as_str(), str::trim);
        let priority = update.priority.unwrap_or(
            TaskPriority::from_str(&current.2)
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?,
        );
        let workspace = update
            .workspace
            .as_deref()
            .map_or(current.3.as_str(), str::trim);
        validate_text(title, workspace)?;
        validate_description(description)?;
        transaction.execute(
            "UPDATE tasks
             SET title = ?2, description = ?3, priority = ?4, workspace = ?5,
                 updated_at = unixepoch()
             WHERE id = ?1",
            params![
                id.to_string(),
                title,
                description,
                priority.to_string(),
                workspace
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'details_updated', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Writes a consistent online backup to a separate `SQLite` file.
    ///
    /// # Errors
    /// Returns an error when the destination or `SQLite` backup operation fails.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = self.connection()?;
        connection.backup("main", path, None)?;
        Ok(())
    }

    /// Runs `SQLite`'s quick integrity check against the live database.
    ///
    /// # Errors
    /// Returns an integrity or persistence error when the check is not successful.
    pub fn verify_integrity(&self) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        let result: String =
            connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(TaskStoreError::IntegrityFailure(result))
        }
    }

    /// Applies one permitted task transition without a handoff note.
    ///
    /// # Errors
    /// Returns an error for an unknown task, rejected transition, or persistence failure.
    pub fn transition_task(&self, id: TaskId, target: TaskState) -> Result<Task, TaskStoreError> {
        self.transition_task_inner(id, target, "", None, &TaskActivityActor::system())
    }

    /// Applies an operator or Queen transition with a bounded audit note.
    ///
    /// # Errors
    /// Returns an error for invalid content, lifecycle, or persistence.
    pub fn transition_task_with_note(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
    ) -> Result<Task, TaskStoreError> {
        self.transition_task_with_note_as(id, target, note, &TaskActivityActor::system())
    }

    /// Applies a task transition and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for invalid content, lifecycle, or persistence.
    pub fn transition_task_with_note_as(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        self.transition_task_inner(id, target, note, None, actor)
    }

    /// Applies an assigned worker transition and queues Blocked or Review for Queen atomically.
    ///
    /// # Errors
    /// Returns an error for a stale assignment, invalid content, capacity, or persistence.
    pub fn transition_worker_task(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        session_id: WorkerSessionId,
    ) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        let worker_id = connection
            .query_row(
                "SELECT worker_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        drop(connection);
        self.transition_task_inner(
            id,
            target,
            note,
            Some(session_id),
            &TaskActivityActor::worker(worker_id),
        )
    }

    fn transition_task_inner(
        &self,
        id: TaskId,
        target: TaskState,
        note: &str,
        reporting_session_id: Option<WorkerSessionId>,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        if note.len() > MAX_TASK_ACTIVITY_NOTE_BYTES {
            return Err(TaskStoreError::InvalidTaskActivityNote);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current: Option<String> = if let Some(session_id) = reporting_session_id {
            transaction
                .query_row(
                    "SELECT task.state FROM tasks task
                     JOIN task_assignments assignment ON assignment.task_id = task.id
                         AND assignment.released_at IS NULL
                     JOIN worker_sessions session ON session.session_id = assignment.worker_session_id
                         AND session.ended_at IS NULL
                     WHERE task.id = ?1 AND session.session_id = ?2",
                    params![id.to_string(), session_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?
        } else {
            transaction
                .query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .optional()?
        };
        let current = current.ok_or_else(|| {
            if reporting_session_id.is_some() {
                TaskStoreError::WorkerSessionNotActive
            } else {
                TaskStoreError::NotFound
            }
        })?;
        let current = TaskState::from_str(&current)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        if !current.can_transition_to(target) {
            return Err(TaskStoreError::InvalidTransition {
                from: current,
                to: target,
            });
        }
        jira::queue_jira_transition(&transaction, id, target)?;
        transaction.execute(
            "DELETE FROM task_outcome_deliveries WHERE task_id = ?1 AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE tasks SET state = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id.to_string(), target.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (
                 task_id, kind, from_state, to_state, note, actor_kind, actor_id
             ) VALUES (?1, 'state_changed', ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                current.to_string(),
                target.to_string(),
                note,
                actor.kind.to_string(),
                actor.id.as_deref(),
            ],
        )?;
        let activity_sequence = transaction.last_insert_rowid();
        if let Some(session_id) = reporting_session_id
            && matches!(target, TaskState::Blocked | TaskState::Review)
        {
            insert_task_outcome(&transaction, id, target, session_id, activity_sequence)?;
        }
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }
    /// Replaces the current durable worker owner and binds its active session when available.
    ///
    /// # Errors
    /// Returns an error for an unknown or completed task or unavailable persistence.
    pub fn assign_task(
        &self,
        id: TaskId,
        session_id: WorkerSessionId,
    ) -> Result<Task, TaskStoreError> {
        let connection = self.connection()?;
        let worker_id = connection
            .query_row(
                "SELECT worker_id FROM worker_sessions
                 WHERE session_id = ?1 AND ended_at IS NULL",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?
            .ok_or(TaskStoreError::WorkerSessionNotActive)?;
        drop(connection);
        self.assign_task_to_worker(id, worker_id)
    }

    /// Assigns a task to a stable worker profile, including while she is sleeping.
    ///
    /// A running incarnation receives one queued briefing. A sleeping worker is
    /// bound and briefed atomically the next time her profile starts.
    ///
    /// # Errors
    /// Returns an error for unknown workers, completed tasks, exhausted queue
    /// capacity, invalid persisted identities, or unavailable storage.
    pub fn assign_task_to_worker(
        &self,
        id: TaskId,
        worker_id: WorkerId,
    ) -> Result<Task, TaskStoreError> {
        self.assign_task_to_worker_as(id, worker_id, &TaskActivityActor::system())
    }

    /// Assigns a task and records its authenticated origin.
    ///
    /// # Errors
    /// Returns an error for unknown workers, completed work, queue capacity, or persistence.
    pub fn assign_task_to_worker_as(
        &self,
        id: TaskId,
        worker_id: WorkerId,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let state: Option<String> = transaction
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let state = state.ok_or(TaskStoreError::NotFound)?;
        if state == TaskState::Completed.to_string() {
            return Err(TaskStoreError::CompletedTask);
        }
        let worker: Option<Option<String>> = transaction
            .query_row(
                "SELECT session.session_id
                 FROM worker_profiles profile
                 LEFT JOIN worker_sessions session
                   ON session.worker_id = profile.id AND session.ended_at IS NULL
                 WHERE profile.id = ?1 AND profile.role != 'queen'",
                [worker_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let session_id = worker.ok_or(TaskStoreError::WorkerNotFound)?;
        transaction.execute(
            "DELETE FROM task_outcome_deliveries WHERE task_id = ?1 AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments WHERE task_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE task_assignments SET released_at = unixepoch()
             WHERE task_id = ?1 AND released_at IS NULL",
            [id.to_string()],
        )?;
        transaction.execute(
            "UPDATE tasks
             SET assigned_worker_id = ?2,
                 workspace = (SELECT workspace FROM worker_profiles WHERE id = ?2),
                 updated_at = unixepoch()
             WHERE id = ?1",
            params![id.to_string(), worker_id.to_string()],
        )?;
        if let Some(session_id) = session_id {
            let queued: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
                [],
                |row| row.get(0),
            )?;
            if queued >= 256 {
                return Err(TaskStoreError::TaskDispatchQueueFull);
            }
            let assignment_id = Uuid::now_v7().to_string();
            transaction.execute(
                "INSERT INTO task_assignments (id, task_id, worker_session_id)
                 VALUES (?1, ?2, ?3)",
                params![assignment_id, id.to_string(), session_id],
            )?;
            transaction.execute(
                "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
                 VALUES (?1, ?2, ?3, 'queued')",
                params![assignment_id, id.to_string(), worker_id.to_string()],
            )?;
        }
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT assignment_id FROM task_dispatches
                 WHERE state IN ('delivered','uncertain')
                 ORDER BY updated_at DESC, assignment_id DESC LIMIT -1 OFFSET 1024
             )",
            [],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, actor_kind, actor_id)
             VALUES (?1, 'assigned', ?2, ?3)",
            params![id.to_string(), actor.kind.to_string(), actor.id.as_deref()],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// Detaches every process binding owned by one stopped worker session.
    ///
    /// Stable worker ownership remains on the task and is rebound on restart.
    ///
    /// # Errors
    /// Returns an error when the assignment history cannot be updated atomically.
    pub fn release_session_assignments(
        &self,
        session_id: WorkerSessionId,
    ) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut task_ids = {
            let mut statement = transaction.prepare(
                "SELECT task_id FROM task_assignments
                 WHERE worker_session_id = ?1 AND released_at IS NULL",
            )?;
            statement
                .query_map([session_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        task_ids.sort_unstable();
        transaction.execute(
            "DELETE FROM task_dispatches WHERE assignment_id IN (
                 SELECT id FROM task_assignments
                 WHERE worker_session_id = ?1 AND released_at IS NULL
             ) AND state = 'queued'",
            [session_id.to_string()],
        )?;
        for task_id in &task_ids {
            transaction.execute(
                "UPDATE task_assignments SET released_at = unixepoch()
                 WHERE task_id = ?1 AND worker_session_id = ?2 AND released_at IS NULL",
                params![task_id, session_id.to_string()],
            )?;
            transaction.execute(
                "UPDATE tasks SET updated_at = unixepoch() WHERE id = ?1",
                [task_id],
            )?;
        }
        if !task_ids.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(task_ids.len())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
}

fn insert_task_outcome(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    target: TaskState,
    session_id: WorkerSessionId,
    activity_sequence: i64,
) -> Result<(), TaskStoreError> {
    let queued: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM task_outcome_deliveries
         WHERE state IN ('queued','dispatching')",
        [],
        |row| row.get(0),
    )?;
    if queued >= 256 {
        return Err(TaskStoreError::TaskOutcomeQueueFull);
    }
    let inserted = transaction.execute(
        "INSERT INTO task_outcome_deliveries (
             id, task_id, activity_sequence, reporting_worker_id,
             recipient_worker_id, target_state, state
         )
         SELECT ?1, ?2, ?3, reporter.id, queen.id, ?5, 'queued'
         FROM worker_sessions session
         JOIN worker_profiles reporter ON reporter.id = session.worker_id
         JOIN worker_profiles queen ON queen.hive_id = reporter.hive_id
             AND queen.role = 'queen'
         WHERE session.session_id = ?4 AND session.ended_at IS NULL",
        params![
            Uuid::now_v7().to_string(),
            task_id.to_string(),
            activity_sequence,
            session_id.to_string(),
            target.to_string(),
        ],
    )?;
    if inserted != 1 {
        return Err(TaskStoreError::IntegrityFailure(
            "worker outcome could not resolve its Queen".into(),
        ));
    }
    transaction.execute(
        "DELETE FROM task_outcome_deliveries WHERE id IN (
             SELECT id FROM task_outcome_deliveries
             WHERE state IN ('delivered','uncertain')
             ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET 1024
         )",
        [],
    )?;
    Ok(())
}
fn migrate_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < 2 {
        migrate_worker_roster(transaction)?;
    }
    if schema_version < 3 {
        migrate_task_details(transaction)?;
    }
    if schema_version < 4 {
        migrate_hive_identity(transaction)?;
    }
    if schema_version < 5 {
        migrate_control_room_events(transaction)?;
    }
    if schema_version < 6 {
        migrate_task_ordering(transaction)?;
    }
    if schema_version < 7 {
        migrate_provider_conversations(transaction)?;
    }
    if schema_version < 8 {
        migrate_worker_engagements(transaction)?;
    }
    if schema_version < 9 {
        migrate_agent_credentials(transaction)?;
    }
    if schema_version < 10 {
        migrate_decision_requests(transaction)?;
    }
    if schema_version < 11 {
        migrate_decision_deliveries(transaction)?;
    }
    if schema_version < 12 {
        migrate_task_dispatches(transaction)?;
    }
    if schema_version < 13 {
        migrate_task_outcomes(transaction)?;
    }
    if schema_version < 14 {
        migrate_operator_presence(transaction)?;
    }
    if schema_version < 15 {
        migrate_notifications(transaction)?;
    }
    if schema_version < 16 {
        migrate_engagement_ownership(transaction)?;
    }
    if schema_version < 17 {
        migrate_queen_autonomy(transaction)?;
    }
    if schema_version < 18 {
        migrate_presentation_preferences(transaction)?;
    }
    if schema_version < 19 {
        migrate_durable_task_ownership(transaction)?;
    }
    if schema_version < 20 {
        migrate_dogfood_reports(transaction)?;
    }
    if schema_version < 21 {
        migrate_jira_bindings(transaction)?;
    }
    if schema_version < 22 {
        migrate_jira_transition_deliveries(transaction)?;
    }
    if schema_version < 23 {
        migrate_jira_comment_deliveries(transaction)?;
    }
    if schema_version < 24 {
        migrate_jira_assigned_sync_preference(transaction)?;
    }
    if schema_version < 25 {
        migrate_apiary_stewardships(transaction)?;
    }
    if schema_version < 26 {
        apiary::migrate_apiary_invitations(transaction)?;
    }
    if schema_version < 27 {
        apiary::migrate_apiary_jira_projects(transaction)?;
    }
    if schema_version < 28 {
        apiary::migrate_apiary_policy_acceptance(transaction)?;
    }
    if schema_version < 29 {
        apiary::migrate_apiary_lifecycle(transaction)?;
    }
    migrate_recent_schema(transaction, schema_version)
}

fn migrate_recent_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    migrate_federation_schema(transaction, schema_version)?;
    if schema_version < 40 {
        email::migrate_email_intake(transaction)?;
    } else if schema_version < 41 {
        email::migrate_email_multi_source(transaction)?;
    }
    if schema_version < 42 {
        apiary::migrate_apiary_identity_events(transaction)?;
    }
    if schema_version < 43 {
        email::migrate_email_reply_targets(transaction)?;
    }
    if schema_version < 44 {
        migrate_task_activity_actors(transaction)?;
    }
    if schema_version < 45 {
        federation::migrate_apiary_join_links(transaction)?;
    }
    if schema_version < 46 {
        federation::migrate_local_apiary_keeper_links(transaction)?;
    }
    if schema_version < 47 {
        federation_tasks::migrate_federation_tasks(transaction)?;
    }
    if schema_version < 48 {
        federation_tasks::migrate_federation_task_commands(transaction)?;
    }
    if schema_version < 49 {
        migrate_worker_profile_metadata(transaction)?;
    }
    if schema_version < 50 {
        federation_jira_claims::migrate_federation_jira_claims(transaction)?;
    }
    if schema_version < 51 {
        migrate_managed_worker_roles(transaction)?;
    }
    if schema_version < 52 {
        federation_stewardships::migrate_federation_stewardship_projection(transaction)?;
    }
    Ok(())
}

fn migrate_managed_worker_roles(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let worker_profiles_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !worker_profiles_exist {
        return transaction.execute_batch("PRAGMA user_version = 51;");
    }
    let has_system_role = {
        let mut statement = transaction.prepare("PRAGMA table_info(worker_profiles)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "system_role")
    };
    if !has_system_role {
        transaction.execute_batch(
            "ALTER TABLE worker_profiles
             ADD COLUMN system_role TEXT CHECK (system_role IS NULL OR system_role = 'scout');",
        )?;
    }
    transaction.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS one_scout_per_hive
             ON worker_profiles(hive_id) WHERE system_role = 'scout' AND archived_at IS NULL;
         PRAGMA user_version = 51;",
    )
}

fn migrate_worker_profile_metadata(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let worker_profiles_exist = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master
                       WHERE type = 'table' AND name = 'worker_profiles')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if worker_profiles_exist {
        let columns = {
            let mut statement = transaction.prepare("PRAGMA table_info(worker_profiles)")?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()?
        };
        if !columns.contains("description") {
            transaction.execute_batch(
                "ALTER TABLE worker_profiles
                 ADD COLUMN description TEXT NOT NULL DEFAULT '';",
            )?;
        }
        if !columns.contains("archived_at") {
            transaction
                .execute_batch("ALTER TABLE worker_profiles ADD COLUMN archived_at INTEGER;")?;
        }
        let has_roster_columns = ["role", "position", "created_at", "id"]
            .iter()
            .all(|column| columns.contains(*column));
        if has_roster_columns {
            transaction.execute_batch(
                "CREATE INDEX IF NOT EXISTS worker_profiles_active_roster
                     ON worker_profiles(role, position, created_at, id)
                     WHERE archived_at IS NULL;",
            )?;
        }
    }
    transaction.pragma_update(None, "user_version", 49)
}

fn migrate_task_activity_actors(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let activity_exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'task_activity')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if activity_exists {
        let actor_kind_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'actor_kind')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !actor_kind_exists {
            transaction.execute_batch(
                "ALTER TABLE task_activity ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'system'
                     CHECK (actor_kind IN ('operator','worker','jira','email','system'));",
            )?;
        }
        let actor_id_exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'actor_id')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !actor_id_exists {
            transaction.execute_batch("ALTER TABLE task_activity ADD COLUMN actor_id TEXT;")?;
        }
    }
    transaction.pragma_update(None, "user_version", 44)
}

fn migrate_federation_schema(
    transaction: &rusqlite::Transaction<'_>,
    schema_version: i64,
) -> rusqlite::Result<()> {
    if schema_version < 30 {
        federation::migrate_federation_identity(transaction)?;
    }
    if schema_version < 31 {
        federation::migrate_federation_candidates(transaction)?;
    }
    if schema_version < 32 {
        federation::migrate_federation_invitations(transaction)?;
    }
    if schema_version < 33 {
        federation::migrate_federation_join_invitations(transaction)?;
    }
    if schema_version < 34 {
        federation::migrate_federation_join_invitation_projects(transaction)?;
    }
    if schema_version < 35 {
        federation::migrate_federation_memberships(transaction)?;
    }
    if schema_version < 36 {
        federation::migrate_local_federation_membership(transaction)?;
    }
    if schema_version < 37 {
        federation::migrate_local_federation_catalog(transaction)?;
    }
    if schema_version < 38 {
        federation::migrate_federation_claims(transaction)?;
    }
    if schema_version < 39 {
        federation::migrate_local_federation_sync(transaction)?;
    }
    Ok(())
}

fn migrate_apiary_stewardships(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS stewardships (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             steward_operator_id TEXT NOT NULL REFERENCES operators(id),
             created_by_operator_id TEXT NOT NULL REFERENCES operators(id),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             revoked_at INTEGER
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_active_stewardship_per_operator
             ON stewardships(apiary_id, steward_operator_id) WHERE revoked_at IS NULL;
         CREATE TABLE IF NOT EXISTS stewardship_hive_grants (
             stewardship_id TEXT NOT NULL REFERENCES stewardships(id) ON DELETE CASCADE,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             PRIMARY KEY (stewardship_id, hive_id)
         );
         CREATE TABLE IF NOT EXISTS stewardship_capability_grants (
             stewardship_id TEXT NOT NULL REFERENCES stewardships(id) ON DELETE CASCADE,
             capability TEXT NOT NULL CHECK (capability IN (
                 'observe','assign','assist','takeover','manage_projects','manage_members'
             )),
             PRIMARY KEY (stewardship_id, capability)
         );
         CREATE TRIGGER IF NOT EXISTS stewardship_creator_is_keeper
             BEFORE INSERT ON stewardships
             WHEN NOT EXISTS (
                 SELECT 1 FROM apiaries a
                 WHERE a.id = NEW.apiary_id
                   AND a.keeper_operator_id = NEW.created_by_operator_id
             )
             BEGIN SELECT RAISE(ABORT, 'Only the Apiary Keeper can grant Stewardship'); END;
         CREATE TRIGGER IF NOT EXISTS immutable_stewardship_identity
             BEFORE UPDATE OF id, apiary_id, steward_operator_id, created_by_operator_id
             ON stewardships
             BEGIN SELECT RAISE(ABORT, 'Stewardship identity is immutable'); END;
         CREATE TRIGGER IF NOT EXISTS stewardship_hive_scope_insert
             BEFORE INSERT ON stewardship_hive_grants
             WHEN NOT EXISTS (
                 SELECT 1 FROM stewardships s
                 JOIN hives h ON h.id = NEW.hive_id
                 WHERE s.id = NEW.stewardship_id AND h.apiary_id = s.apiary_id
             )
             BEGIN SELECT RAISE(ABORT, 'Steward Hive grant must stay inside its Apiary'); END;
         CREATE TRIGGER IF NOT EXISTS stewardship_hive_scope_update
             BEFORE UPDATE OF stewardship_id, hive_id ON stewardship_hive_grants
             WHEN NOT EXISTS (
                 SELECT 1 FROM stewardships s
                 JOIN hives h ON h.id = NEW.hive_id
                 WHERE s.id = NEW.stewardship_id AND h.apiary_id = s.apiary_id
             )
             BEGIN SELECT RAISE(ABORT, 'Steward Hive grant must stay inside its Apiary'); END;
         PRAGMA user_version = 25;",
    )
}

fn migrate_jira_assigned_sync_preference(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    let column_exists: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('jira_project_bindings')
             WHERE name = 'auto_sync_assigned'
         )",
        [],
        |row| row.get(0),
    )?;
    if !column_exists {
        transaction.execute_batch(
            "ALTER TABLE jira_project_bindings
                 ADD COLUMN auto_sync_assigned INTEGER NOT NULL DEFAULT 0
                 CHECK (auto_sync_assigned IN (0,1));",
        )?;
    }
    transaction.pragma_update(None, "user_version", 24)
}

fn migrate_jira_comment_deliveries(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_comment_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             body TEXT NOT NULL,
             state TEXT NOT NULL CHECK (
                 state IN ('queued','dispatching','delivered','conflict','uncertain')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS jira_comment_delivery_queue
             ON jira_comment_deliveries(state, available_at, created_at);
         PRAGMA user_version = 23;",
    )
}

fn migrate_jira_transition_deliveries(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_transition_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             target_task_state TEXT NOT NULL CHECK (
                 target_task_state IN ('draft','ready','active','blocked','review','completed')
             ),
             state TEXT NOT NULL CHECK (
                 state IN ('queued','dispatching','delivered','conflict','uncertain')
             ),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 3),
             available_at INTEGER NOT NULL DEFAULT (unixepoch()),
             attempted_at INTEGER,
             delivered_at INTEGER,
             last_error TEXT,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE UNIQUE INDEX IF NOT EXISTS jira_transition_one_active_per_task
             ON jira_transition_deliveries(task_id)
             WHERE state IN ('queued','dispatching');
         CREATE INDEX IF NOT EXISTS jira_transition_delivery_queue
             ON jira_transition_deliveries(state, available_at, updated_at);
         PRAGMA user_version = 22;",
    )
}

fn migrate_jira_bindings(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS jira_project_bindings (
             id TEXT PRIMARY KEY,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             project_id TEXT NOT NULL,
             project_key TEXT NOT NULL,
             project_name TEXT NOT NULL,
             scope TEXT NOT NULL CHECK (scope IN ('hive','apiary')),
             apiary_id TEXT REFERENCES apiaries(id),
             default_worker_id TEXT REFERENCES worker_profiles(id),
             access_verified INTEGER NOT NULL DEFAULT 0 CHECK (access_verified IN (0,1)),
             workflow_mapped INTEGER NOT NULL DEFAULT 0 CHECK (workflow_mapped IN (0,1)),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE(hive_id, project_id),
             UNIQUE(hive_id, project_key),
             CHECK ((scope = 'hive' AND apiary_id IS NULL) OR
                    (scope = 'apiary' AND apiary_id IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS jira_project_bindings_by_hive
             ON jira_project_bindings(hive_id, project_name, project_key);
         CREATE TABLE IF NOT EXISTS jira_status_mappings (
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id) ON DELETE CASCADE,
             jira_status_id TEXT NOT NULL,
             jira_status_name TEXT NOT NULL,
             task_state TEXT NOT NULL CHECK (
                 task_state IN ('draft','ready','active','blocked','review','completed')
             ),
             PRIMARY KEY(binding_id, jira_status_id)
         );
         CREATE TABLE IF NOT EXISTS jira_issue_links (
             issue_id TEXT PRIMARY KEY,
             issue_key TEXT NOT NULL UNIQUE,
             binding_id TEXT NOT NULL REFERENCES jira_project_bindings(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             jira_status_id TEXT NOT NULL,
             jira_status_name TEXT NOT NULL,
             jira_assignee_account_id TEXT,
             jira_assignee_name TEXT,
             remote_updated_at TEXT NOT NULL,
             last_synced_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS jira_issue_links_by_binding
             ON jira_issue_links(binding_id, issue_key);
         PRAGMA user_version = 21;",
    )
}

fn migrate_dogfood_reports(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS dogfood_reports (
            id TEXT PRIMARY KEY,
            expectation TEXT NOT NULL,
            observation TEXT NOT NULL,
            diagnostic_bundle TEXT NOT NULL,
            attachment_name TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE INDEX IF NOT EXISTS dogfood_reports_newest
            ON dogfood_reports(created_at DESC, id DESC);
         PRAGMA user_version = 20;",
    )
}

fn migrate_durable_task_ownership(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let has_owner = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'assigned_worker_id')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_owner {
        transaction.execute(
            "ALTER TABLE tasks ADD COLUMN assigned_worker_id TEXT REFERENCES worker_profiles(id)",
            [],
        )?;
    }
    let has_assignments = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'task_assignments')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if has_assignments {
        transaction.execute_batch(
            "UPDATE tasks
             SET assigned_worker_id = (
                 SELECT session.worker_id
                 FROM task_assignments assignment
                 JOIN worker_sessions session ON session.session_id = assignment.worker_session_id
                 WHERE assignment.task_id = tasks.id AND assignment.released_at IS NULL
                 LIMIT 1
             )
             WHERE assigned_worker_id IS NULL;",
        )?;
    }
    transaction.execute_batch(
        "CREATE INDEX IF NOT EXISTS task_owner_queue
             ON tasks(assigned_worker_id, state)
             WHERE assigned_worker_id IS NOT NULL AND state != 'completed';
         PRAGMA user_version = 19;",
    )
}

fn migrate_queen_autonomy(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS queen_autonomy_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             at_hive TEXT NOT NULL CHECK (at_hive IN ('advisory','coordinate','local_execution')),
             away TEXT NOT NULL CHECK (away IN ('advisory','coordinate','local_execution')),
             night_watch TEXT NOT NULL CHECK (night_watch IN ('advisory','coordinate','local_execution')),
             updated_at INTEGER NOT NULL
         );
         PRAGMA user_version = 17;",
    )
}

fn migrate_presentation_preferences(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS presentation_preferences (
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             color_theme TEXT NOT NULL CHECK (color_theme IN ('light','dark')),
             terminal_keys_visible INTEGER NOT NULL CHECK (terminal_keys_visible IN (0,1)),
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (operator_id, device_class)
         );
         PRAGMA user_version = 18;",
    )
}

fn migrate_worker_roster(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS worker_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE UNIQUE,
            role TEXT NOT NULL CHECK (role IN ('queen','worker')),
            provider TEXT NOT NULL CHECK (provider IN ('claude_code','codex')),
            workspace TEXT NOT NULL,
            autostart INTEGER NOT NULL CHECK (autostart IN (0,1)),
            position INTEGER NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_queen_profile
            ON worker_profiles(role) WHERE role = 'queen';
        CREATE TABLE IF NOT EXISTS worker_sessions (
            session_id TEXT PRIMARY KEY,
            worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
            started_at INTEGER NOT NULL DEFAULT (unixepoch()),
            ended_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_active_session_per_worker
            ON worker_sessions(worker_id) WHERE ended_at IS NULL;
        PRAGMA user_version = 2;
        ",
    )
}

fn migrate_task_details(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
         ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal'
             CHECK (priority IN ('low','normal','high','urgent'));
         PRAGMA user_version = 3;",
    )
}

fn migrate_hive_identity(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let operator_id = OperatorId::new();
    let hive_id = HiveId::new();
    transaction.execute_batch(
        "
        CREATE TABLE operators (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE apiaries (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            keeper_operator_id TEXT NOT NULL REFERENCES operators(id),
            shared_work_backend TEXT NOT NULL
                CHECK (shared_work_backend IN ('jira','native')),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE hives (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            operator_id TEXT NOT NULL UNIQUE REFERENCES operators(id),
            apiary_id TEXT REFERENCES apiaries(id),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE local_hive_identity (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            hive_id TEXT NOT NULL UNIQUE REFERENCES hives(id)
        );
        ",
    )?;
    transaction.execute(
        "INSERT INTO operators (id, display_name) VALUES (?1, 'Operator')",
        [operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'My Hive', ?2)",
        params![hive_id.to_string(), operator_id.to_string()],
    )?;
    transaction.execute(
        "INSERT INTO local_hive_identity (singleton, hive_id) VALUES (1, ?1)",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        ALTER TABLE tasks ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ALTER TABLE worker_profiles ADD COLUMN hive_id TEXT REFERENCES hives(id);
        ",
    )?;
    transaction.execute(
        "UPDATE tasks SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute(
        "UPDATE worker_profiles SET hive_id = ?1 WHERE hive_id IS NULL",
        [hive_id.to_string()],
    )?;
    transaction.execute_batch(
        "
        CREATE INDEX tasks_by_hive ON tasks(hive_id);
        CREATE INDEX worker_profiles_by_hive ON worker_profiles(hive_id);
        CREATE TRIGGER tasks_require_hive_insert
            BEFORE INSERT ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER tasks_require_hive_update
            BEFORE UPDATE OF hive_id ON tasks WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'task hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_insert
            BEFORE INSERT ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER worker_profiles_require_hive_update
            BEFORE UPDATE OF hive_id ON worker_profiles WHEN NEW.hive_id IS NULL
            BEGIN SELECT RAISE(ABORT, 'worker hive_id is required'); END;
        CREATE TRIGGER immutable_apiary_backend
            BEFORE UPDATE OF shared_work_backend ON apiaries
            BEGIN SELECT RAISE(ABORT, 'Apiary shared-work backend is immutable'); END;
        PRAGMA user_version = 4;
        ",
    )
}

fn migrate_control_room_events(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE control_room_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            hive_id TEXT NOT NULL REFERENCES hives(id),
            kind TEXT NOT NULL CHECK (
                kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed')
            ),
            occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE INDEX control_room_events_by_hive_sequence
            ON control_room_events(hive_id, sequence);
        PRAGMA user_version = 5;
        ",
    )
}

fn migrate_task_ordering(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
         WITH ranked AS (
             SELECT id, ROW_NUMBER() OVER (
                 PARTITION BY hive_id ORDER BY created_at, id
             ) - 1 AS new_position
             FROM tasks
         )
         UPDATE tasks SET position = (
             SELECT new_position FROM ranked WHERE ranked.id = tasks.id
         );
         CREATE INDEX tasks_by_hive_position ON tasks(hive_id, position);
         PRAGMA user_version = 6;",
    )
}

fn migrate_provider_conversations(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE worker_profiles ADD COLUMN provider_conversation_id TEXT;
         PRAGMA user_version = 7;",
    )
}

fn migrate_worker_engagements(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE worker_engagements (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             session_id TEXT NOT NULL UNIQUE REFERENCES worker_sessions(session_id) ON DELETE CASCADE,
             engaged_at INTEGER NOT NULL,
             renewed_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL CHECK (expires_at > renewed_at)
         );
         PRAGMA user_version = 8;",
    )
}

fn migrate_engagement_ownership(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let already_owned = {
        let mut statement = transaction.prepare("PRAGMA table_info(worker_engagements)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "owner_device_id")
    };
    if !already_owned {
        transaction
            .execute_batch("ALTER TABLE worker_engagements ADD COLUMN owner_device_id TEXT;")?;
    }
    transaction.pragma_update(None, "user_version", 16)
}

fn migrate_agent_credentials(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_agent_credentials (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             token_digest BLOB NOT NULL UNIQUE CHECK (length(token_digest) = 32),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             rotated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         PRAGMA user_version = 9;",
    )
}
fn migrate_decision_requests(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v9;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v9;
         DROP TABLE control_room_events_v9;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS decision_requests (
             id TEXT PRIMARY KEY,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             requesting_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             task_id TEXT REFERENCES tasks(id),
             kind TEXT NOT NULL CHECK (kind IN ('input','approval','credentials','conflict','help')),
             urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
             title TEXT NOT NULL,
             reason TEXT NOT NULL,
             risk TEXT NOT NULL,
             evidence TEXT NOT NULL,
             suggested_action TEXT NOT NULL,
             allowed_actions TEXT NOT NULL,
             deadline INTEGER,
             state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','resolved')),
             resolution_action TEXT,
             resolution_note TEXT NOT NULL DEFAULT '',
             resolved_by_operator_id TEXT REFERENCES operators(id),
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             resolved_at INTEGER,
             CHECK ((state = 'pending' AND resolution_action IS NULL AND resolved_at IS NULL)
                 OR (state = 'resolved' AND resolution_action IS NOT NULL AND resolved_at IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS decision_requests_inbox
             ON decision_requests(hive_id, state, urgency, deadline, created_at DESC);
         CREATE INDEX IF NOT EXISTS decision_requests_by_worker
             ON decision_requests(requesting_worker_id, state, created_at DESC);
         PRAGMA user_version = 10;",
    )
}
fn migrate_decision_deliveries(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS decision_deliveries (
             decision_id TEXT PRIMARY KEY REFERENCES decision_requests(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             session_id TEXT REFERENCES worker_sessions(session_id),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS decision_deliveries_queue
             ON decision_deliveries(state, updated_at, decision_id);
         PRAGMA user_version = 11;",
    )
}
fn migrate_task_dispatches(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_dispatches (
             assignment_id TEXT PRIMARY KEY REFERENCES task_assignments(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS task_dispatches_queue
             ON task_dispatches(state, updated_at, assignment_id);
         PRAGMA user_version = 12;",
    )
}
fn migrate_task_outcomes(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_activity (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             kind TEXT NOT NULL,
             from_state TEXT,
             to_state TEXT,
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );",
    )?;
    let has_note = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('task_activity') WHERE name = 'note'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !has_note {
        transaction.execute(
            "ALTER TABLE task_activity ADD COLUMN note TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_outcome_deliveries (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             activity_sequence INTEGER NOT NULL UNIQUE REFERENCES task_activity(sequence) ON DELETE CASCADE,
             reporting_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             recipient_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             target_state TEXT NOT NULL CHECK (target_state IN ('blocked','review')),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching','delivered','uncertain')),
             session_id TEXT REFERENCES worker_sessions(session_id),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
             attempted_at INTEGER,
             delivered_at INTEGER,
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             CHECK ((state = 'delivered' AND delivered_at IS NOT NULL)
                 OR (state <> 'delivered' AND delivered_at IS NULL))
         );
         CREATE INDEX IF NOT EXISTS task_outcome_deliveries_queue
             ON task_outcome_deliveries(state, updated_at, id);
         PRAGMA user_version = 13;",
    )
}
fn migrate_operator_presence(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v13;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed','presence_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v13;
         DROP TABLE control_room_events_v13;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS operator_presence_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             manual_mode TEXT CHECK (manual_mode IS NULL OR manual_mode IN ('at_hive','away','night_watch')),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         CREATE TABLE IF NOT EXISTS operator_presence_devices (
             id TEXT PRIMARY KEY,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             state TEXT NOT NULL CHECK (state IN ('active','idle','locked','hidden')),
             expires_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             CHECK (expires_at > updated_at)
         );
         CREATE INDEX IF NOT EXISTS operator_presence_devices_current
             ON operator_presence_devices(operator_id, expires_at, state);
         PRAGMA user_version = 14;",
    )
}
fn migrate_notifications(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE control_room_events RENAME TO control_room_events_v14;
         CREATE TABLE control_room_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             hive_id TEXT NOT NULL REFERENCES hives(id),
             kind TEXT NOT NULL CHECK (
                 kind IN ('tasks_changed','workers_changed','sessions_changed','runtime_changed','decisions_changed','presence_changed','notifications_changed')
             ),
             occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
         );
         INSERT INTO control_room_events (sequence, hive_id, kind, occurred_at)
             SELECT sequence, hive_id, kind, occurred_at FROM control_room_events_v14;
         DROP TABLE control_room_events_v14;
         CREATE INDEX control_room_events_by_hive_sequence
             ON control_room_events(hive_id, sequence);
         CREATE TABLE IF NOT EXISTS notification_vapid_keys (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             private_key BLOB NOT NULL CHECK (length(private_key) = 32),
             public_key BLOB NOT NULL CHECK (length(public_key) = 65),
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS notification_preferences (
             operator_id TEXT PRIMARY KEY REFERENCES operators(id) ON DELETE CASCADE,
             policy TEXT NOT NULL CHECK (policy IN ('important_only','all_decisions','off')),
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS notification_subscriptions (
             device_id TEXT PRIMARY KEY,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             device_class TEXT NOT NULL CHECK (device_class IN ('desktop','mobile')),
             endpoint TEXT NOT NULL UNIQUE CHECK (length(endpoint) BETWEEN 1 AND 4096),
             p256dh BLOB NOT NULL CHECK (length(p256dh) = 65),
             auth BLOB NOT NULL CHECK (length(auth) = 16),
             failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS notification_subscriptions_by_operator
             ON notification_subscriptions(operator_id, updated_at);
         CREATE TABLE IF NOT EXISTS notification_deliveries (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
             subscription_id TEXT NOT NULL REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
             decision_id TEXT REFERENCES decision_requests(id) ON DELETE CASCADE,
             urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
             kind TEXT NOT NULL CHECK (kind IN ('decision','test')),
             state TEXT NOT NULL CHECK (state IN ('queued','dispatching')),
             attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
             available_at INTEGER NOT NULL,
             created_at INTEGER NOT NULL,
             CHECK ((kind = 'decision' AND decision_id IS NOT NULL) OR (kind = 'test' AND decision_id IS NULL)),
             UNIQUE(decision_id, subscription_id)
         );
         CREATE INDEX IF NOT EXISTS notification_deliveries_ready
             ON notification_deliveries(state, available_at, urgency, id);
         PRAGMA user_version = 15;",
    )
}
fn validate_text(title: &str, workspace: &str) -> Result<(), TaskStoreError> {
    if title.is_empty() || title.len() > MAX_TASK_TITLE_BYTES {
        return Err(TaskStoreError::InvalidTitle);
    }
    if workspace.is_empty() || workspace.len() > MAX_WORKSPACE_BYTES {
        return Err(TaskStoreError::InvalidWorkspace);
    }
    Ok(())
}

fn validate_description(description: &str) -> Result<(), TaskStoreError> {
    if description.len() > MAX_TASK_DESCRIPTION_BYTES {
        return Err(TaskStoreError::InvalidDescription);
    }
    Ok(())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get(0)?;
    let hive_id: String = row.get(1)?;
    let priority: String = row.get(4)?;
    let state: String = row.get(6)?;
    let assigned_worker_id: Option<String> = row.get(7)?;
    let assigned_session_id: Option<String> = row.get(8)?;
    let dispatch_state: Option<String> = row.get(9)?;
    let outcome_delivery_state: Option<String> = row.get(10)?;
    Ok(Task {
        id: TaskId::from_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        hive_id: parse_domain_id::<HiveId>(&hive_id)?,
        title: row.get(2)?,
        description: row.get(3)?,
        priority: TaskPriority::from_str(&priority).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        workspace: row.get(5)?,
        state: TaskState::from_str(&state).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        assigned_worker_id: assigned_worker_id
            .map(|value| WorkerId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        assigned_session_id: assigned_session_id
            .map(|value| WorkerSessionId::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        dispatch_state: dispatch_state
            .map(|value| TaskDispatchState::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        outcome_delivery_state: outcome_delivery_state
            .map(|value| TaskOutcomeDeliveryState::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        position: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn task_activity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskActivity> {
    let kind = TaskActivityKind::from_str(&row.get::<_, String>(2)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let from_state = row
        .get::<_, Option<String>>(3)?
        .map(|value| TaskState::from_str(&value))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let to_state = row
        .get::<_, Option<String>>(4)?
        .map(|value| TaskState::from_str(&value))
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let actor_kind = TaskActivityActorKind::from_str(&row.get::<_, String>(7)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(TaskActivity {
        sequence: row.get(0)?,
        task_id: TaskId::from_str(&row.get::<_, String>(1)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        kind,
        from_state,
        to_state,
        note: row.get(5)?,
        occurred_at: row.get(6)?,
        actor_kind,
        actor_id: row.get(8)?,
    })
}

fn parse_domain_id<T: FromStr>(value: &str) -> rusqlite::Result<T> {
    T::from_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_task_lifecycle_and_assignment() {
        let store = TaskStore::in_memory().unwrap();
        let created = store.create_task("Fix reload", "/workspace").unwrap();
        assert_eq!(created.state, TaskState::Draft);

        let ready = store.transition_task(created.id, TaskState::Ready).unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let assigned = store.assign_task(ready.id, session_id).unwrap();
        assert_eq!(assigned.assigned_worker_id, Some(worker.id));
        assert_eq!(assigned.assigned_session_id, Some(session_id));
        assert_eq!(store.list_tasks().unwrap().len(), 1);

        store.transition_task(ready.id, TaskState::Active).unwrap();
        store.transition_task(ready.id, TaskState::Review).unwrap();
        let completed = store
            .transition_task(ready.id, TaskState::Completed)
            .unwrap();
        assert_eq!(completed.state, TaskState::Completed);
        assert!(matches!(
            store.assign_task_to_worker(ready.id, worker.id),
            Err(TaskStoreError::CompletedTask)
        ));

        let activity = store.list_task_activity(created.id, 100).unwrap();
        assert!(!activity.truncated);
        assert_eq!(activity.events.len(), 6);
        assert_eq!(activity.events[0].kind, TaskActivityKind::Created);
        assert_eq!(activity.events[1].from_state, Some(TaskState::Draft));
        assert_eq!(activity.events[1].to_state, Some(TaskState::Ready));
        assert_eq!(activity.events[2].kind, TaskActivityKind::Assigned);
        assert_eq!(activity.events[5].to_state, Some(TaskState::Completed));
    }

    #[test]
    fn recent_task_activity_is_bounded_across_the_local_hive() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First task", "/workspace/first").unwrap();
        let second = store
            .create_task("Second task", "/workspace/second")
            .unwrap();
        store.transition_task(first.id, TaskState::Ready).unwrap();
        store.transition_task(second.id, TaskState::Ready).unwrap();

        let recent = store.list_recent_task_activity(3).unwrap();

        assert!(recent.truncated);
        assert_eq!(recent.events.len(), 3);
        assert!(
            recent
                .events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(recent.events.last().unwrap().task_id, second.id);
        assert_eq!(
            recent.events.last().unwrap().to_state,
            Some(TaskState::Ready)
        );
    }

    #[test]
    fn task_activity_preserves_authenticated_actor_provenance() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task_with_details_as(
                "Trace the work",
                "",
                TaskPriority::Normal,
                "/workspace",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        store
            .transition_task_with_note_as(
                task.id,
                TaskState::Ready,
                "Prepared by Daisy",
                &TaskActivityActor::worker(worker.id),
            )
            .unwrap();

        let activity = store.list_task_activity(task.id, 10).unwrap().events;
        assert_eq!(activity[0].actor_kind, TaskActivityActorKind::Operator);
        assert_eq!(activity[0].actor_id, None);
        assert_eq!(activity[1].actor_kind, TaskActivityActorKind::Worker);
        assert_eq!(activity[1].actor_id, Some(worker.id.to_string()));
    }

    #[test]
    fn unassigning_releases_worker_ownership_and_cancels_a_queued_brief() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Daisy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        let task = store.create_task("Return this work", "/workspace").unwrap();
        let assigned = store.assign_task_to_worker(task.id, worker.id).unwrap();
        assert_eq!(assigned.dispatch_state, Some(TaskDispatchState::Queued));

        let unassigned = store.unassign_task(task.id).unwrap();

        assert_eq!(unassigned.assigned_worker_id, None);
        assert_eq!(unassigned.assigned_session_id, None);
        assert_eq!(unassigned.dispatch_state, None);
        assert!(store.claim_task_dispatches(i64::MAX).unwrap().is_empty());
        let activity = store.list_task_activity(task.id, 100).unwrap();
        assert_eq!(
            activity.events.last().unwrap().kind,
            TaskActivityKind::Unassigned
        );
    }

    #[test]
    fn sleeping_worker_owns_task_and_rebinds_it_after_restart() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Clover",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/clover",
                false,
                1,
            )
            .unwrap();
        let task = store
            .create_task("Resume durable work", "/workspace/clover")
            .unwrap();

        let sleeping = store.assign_task_to_worker(task.id, worker.id).unwrap();
        assert_eq!(sleeping.assigned_worker_id, Some(worker.id));
        assert_eq!(sleeping.assigned_session_id, None);
        assert_eq!(sleeping.dispatch_state, None);

        let first = WorkerSessionId::new();
        store.bind_worker_session(worker.id, first).unwrap();
        let started = store.get_task(task.id).unwrap();
        assert_eq!(started.assigned_worker_id, Some(worker.id));
        assert_eq!(started.assigned_session_id, Some(first));
        assert_eq!(started.dispatch_state, Some(TaskDispatchState::Queued));

        store.release_worker_session(first).unwrap();
        store.release_session_assignments(first).unwrap();
        let stopped = store.get_task(task.id).unwrap();
        assert_eq!(stopped.assigned_worker_id, Some(worker.id));
        assert_eq!(stopped.assigned_session_id, None);

        let second = WorkerSessionId::new();
        store.bind_worker_session(worker.id, second).unwrap();
        let resumed = store.get_task(task.id).unwrap();
        assert_eq!(resumed.assigned_worker_id, Some(worker.id));
        assert_eq!(resumed.assigned_session_id, Some(second));
        assert_eq!(resumed.dispatch_state, Some(TaskDispatchState::Queued));

        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE task_dispatches
                 SET state = 'delivered', delivered_at = unixepoch(), updated_at = unixepoch()
                 WHERE task_id = ?1 AND state = 'queued'",
                [task.id.to_string()],
            )
            .unwrap();
        store.release_worker_session(second).unwrap();
        store.release_session_assignments(second).unwrap();
        let third = WorkerSessionId::new();
        store.bind_worker_session(worker.id, third).unwrap();
        let continued = store.get_task(task.id).unwrap();
        assert_eq!(continued.assigned_session_id, Some(third));
        assert_eq!(continued.dispatch_state, None);
    }

    #[test]
    fn task_activity_is_bounded_and_unknown_tasks_fail_closed() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Bound history", "/workspace").unwrap();
        for _ in 0..(MAX_TASK_ACTIVITY_PAGE + 10) {
            store
                .update_task_details(
                    task.id,
                    &TaskDetailsUpdate {
                        description: Some("same durable detail".into()),
                        ..TaskDetailsUpdate::default()
                    },
                )
                .unwrap();
        }

        let activity = store.list_task_activity(task.id, usize::MAX).unwrap();
        assert!(activity.truncated);
        assert_eq!(activity.events.len(), MAX_TASK_ACTIVITY_PAGE);
        assert!(
            activity
                .events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert!(matches!(
            store.list_task_activity(TaskId::new(), 30),
            Err(TaskStoreError::NotFound)
        ));
    }

    #[test]
    fn open_task_order_is_complete_atomic_and_durable() {
        let store = TaskStore::in_memory().unwrap();
        let first = store.create_task("First", "/workspace").unwrap();
        let second = store.create_task("Second", "/workspace").unwrap();
        let third = store.create_task("Third", "/workspace").unwrap();
        assert_eq!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .map(|task| task.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        let reordered = store
            .reorder_open_tasks(&[third.id, first.id, second.id])
            .unwrap();
        assert_eq!(
            reordered.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![third.id, first.id, second.id]
        );
        assert_eq!(
            reordered
                .iter()
                .map(|task| task.position)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        assert!(matches!(
            store.reorder_open_tasks(&[first.id, second.id]),
            Err(TaskStoreError::InvalidTaskOrder)
        ));
        assert_eq!(
            store
                .list_tasks()
                .unwrap()
                .iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec![third.id, first.id, second.id]
        );
    }

    #[test]
    fn updates_only_supplied_task_details_and_records_activity() {
        let store = TaskStore::in_memory().unwrap();
        let task = store
            .create_task_with_details(
                "Polish task cards",
                "Make priority visible",
                TaskPriority::High,
                "/workspace",
            )
            .unwrap();
        let updated = store
            .update_task_details(
                task.id,
                &TaskDetailsUpdate {
                    title: Some("Polish the task board".into()),
                    priority: Some(TaskPriority::Urgent),
                    ..TaskDetailsUpdate::default()
                },
            )
            .unwrap();

        assert_eq!(updated.title, "Polish the task board");
        assert_eq!(updated.description, "Make priority visible");
        assert_eq!(updated.priority, TaskPriority::Urgent);
        assert_eq!(updated.workspace, "/workspace");
        assert!(matches!(
            store.update_task_details(task.id, &TaskDetailsUpdate::default()),
            Err(TaskStoreError::EmptyTaskDetailsUpdate)
        ));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM task_activity WHERE task_id = ?1 AND kind = 'details_updated'",
                    [task.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn stopping_a_session_releases_its_assignments() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Assigned work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        let worker = store
            .create_worker(
                "Poppy",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session_id = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session_id).unwrap();
        store.assign_task(task.id, session_id).unwrap();

        assert_eq!(store.release_session_assignments(session_id).unwrap(), 1);
        let stopped = store.get_task(task.id).unwrap();
        assert_eq!(stopped.assigned_worker_id, Some(worker.id));
        assert_eq!(stopped.assigned_session_id, None);
        assert_eq!(store.release_session_assignments(session_id).unwrap(), 0);
    }

    #[test]
    fn rejects_skipped_transitions_and_invalid_content() {
        let store = TaskStore::in_memory().unwrap();
        assert!(matches!(
            store.create_task("", "/workspace"),
            Err(TaskStoreError::InvalidTitle)
        ));
        let task = store.create_task("A task", "/workspace").unwrap();
        assert!(matches!(
            store.transition_task(task.id, TaskState::Completed),
            Err(TaskStoreError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn reopens_file_database_without_losing_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let id = {
            let store = TaskStore::open(&path).unwrap();
            store
                .create_task("Persistent task", "/workspace")
                .unwrap()
                .id
        };
        let reopened = TaskStore::open(path).unwrap();
        assert_eq!(reopened.get_task(id).unwrap().title, "Persistent task");
    }

    #[test]
    fn migrates_the_task_only_schema_to_the_worker_roster() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                PRAGMA user_version = 1;
                ",
            )
            .unwrap();
        let store = TaskStore::from_connection(connection).unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        assert_eq!(queen.role, swarm_domain::WorkerRole::Queen);
        let columns = store
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"description".to_owned()));
        assert!(columns.contains(&"priority".to_owned()));
        let worker_columns = store
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(worker_profiles)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(worker_columns.contains(&"provider_conversation_id".to_owned()));
        assert!(worker_columns.contains(&"description".to_owned()));
        assert!(worker_columns.contains(&"archived_at".to_owned()));
        assert_eq!(
            store
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn reopens_current_schema_without_replacing_hive_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v21_to_durable_jira_transition_deliveries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE jira_transition_deliveries;
                     PRAGMA user_version = 21;",
                )
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'jira_transition_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v22_to_durable_jira_comment_deliveries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP TABLE jira_comment_deliveries;
                     PRAGMA user_version = 22;",
                )
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'jira_comment_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        reopened.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v23_to_opt_in_assigned_jira_sync() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE jira_project_bindings (
                     id TEXT PRIMARY KEY,
                     project_name TEXT NOT NULL
                 );
                 INSERT INTO jira_project_bindings (id, project_name)
                 VALUES ('binding-1', 'Website Services');
                 PRAGMA user_version = 23;",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        migrate_schema(&transaction, 23).unwrap();
        transaction.commit().unwrap();

        assert_eq!(
            connection
                .query_row(
                    "SELECT auto_sync_assigned FROM jira_project_bindings WHERE id = 'binding-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrates_v10_decisions_to_the_guarded_delivery_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            let connection = store.connection().unwrap();
            connection
                .execute_batch("DROP TABLE decision_deliveries; PRAGMA user_version = 10;")
                .unwrap();
        }
        let reopened = TaskStore::open(path).unwrap();
        let table_exists = reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decision_deliveries'",
                [],
                |_| Ok(()),
            )
            .optional()
            .unwrap()
            .is_some();
        assert!(table_exists);
        assert_eq!(
            reopened
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_v3_tasks_and_workers_into_one_durable_hive() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    description TEXT NOT NULL DEFAULT '',
                    priority TEXT NOT NULL DEFAULT 'normal'
                );
                CREATE TABLE task_assignments (
                    id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    worker_session_id TEXT NOT NULL,
                    assigned_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    released_at INTEGER
                );
                CREATE TABLE task_activity (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL,
                    from_state TEXT,
                    to_state TEXT,
                    occurred_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_profiles (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    role TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    autostart INTEGER NOT NULL,
                    position INTEGER NOT NULL,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE worker_sessions (
                    session_id TEXT PRIMARY KEY,
                    worker_id TEXT NOT NULL REFERENCES worker_profiles(id) ON DELETE CASCADE,
                    started_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    ended_at INTEGER
                );
                INSERT INTO tasks (id, title, workspace, state)
                    VALUES ('018f0000-0000-7000-8000-000000000001', 'Existing task', '/repo', 'ready');
                INSERT INTO worker_profiles
                    (id, name, role, provider, workspace, autostart, position)
                    VALUES ('018f0000-0000-7000-8000-000000000002', 'Existing worker', 'worker', 'claude_code', '/repo', 0, 1);
                PRAGMA user_version = 3;
                ",
            )
            .unwrap();

        let store = TaskStore::from_connection(connection).unwrap();
        let identity = store.local_hive_identity().unwrap();
        assert_eq!(store.list_tasks().unwrap()[0].hive_id, identity.hive.id);
        assert_eq!(
            store.list_worker_profiles().unwrap()[0].hive_id,
            identity.hive.id
        );
        store.verify_integrity().unwrap();
    }

    #[test]
    fn hive_ownership_and_apiary_backend_constraints_fail_closed() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let connection = store.connection().unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO tasks (id, title, workspace, state, description, priority)
                     VALUES (?1, 'Orphan', '/repo', 'draft', '', 'normal')",
                    [TaskId::new().to_string()],
                )
                .is_err()
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO worker_profiles
                     (id, name, role, provider, workspace, autostart, position)
                     VALUES (?1, 'Orphan', 'worker', 'claude_code', '/repo', 0, 1)",
                    [swarm_domain::WorkerId::new().to_string()],
                )
                .is_err()
        );

        let apiary_id = ApiaryId::new();
        connection
            .execute(
                "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                 VALUES (?1, 'Test Apiary', ?2, 'jira')",
                params![apiary_id.to_string(), identity.operator.id.to_string()],
            )
            .unwrap();
        assert!(
            connection
                .execute(
                    "UPDATE apiaries SET shared_work_backend = 'native' WHERE id = ?1",
                    [apiary_id.to_string()],
                )
                .is_err()
        );
    }

    #[test]
    fn local_apiary_context_is_personal_until_durable_membership_exists() {
        let store = TaskStore::in_memory().unwrap();
        assert_eq!(
            store.local_apiary_context().unwrap(),
            LocalApiaryContext::Personal
        );

        let identity = store.local_hive_identity().unwrap();
        let apiary_id = ApiaryId::new();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                     VALUES (?1, 'Garden', ?2, 'jira')",
                    params![apiary_id.to_string(), identity.operator.id.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE hives SET apiary_id = ?1 WHERE id = ?2",
                    params![apiary_id.to_string(), identity.hive.id.to_string()],
                )
                .unwrap();
        }

        assert!(matches!(
            store.local_apiary_context().unwrap(),
            LocalApiaryContext::Federated {
                apiary,
                local_role: LocalApiaryRole::Keeper,
            } if apiary.id == apiary_id && apiary.shared_work_backend() == SharedWorkBackend::Jira
        ));
    }

    #[test]
    fn apiary_member_roster_is_role_oriented_and_excludes_personal_hives() {
        let personal = TaskStore::in_memory().unwrap();
        assert!(matches!(
            personal.list_apiary_members(),
            Err(TaskStoreError::InvalidApiary)
        ));

        let store = TaskStore::in_memory().unwrap();
        let keeper = store.local_hive_identity().unwrap();
        let context = store
            .create_apiary_for_local_hive("Wildflower Garden", SharedWorkBackend::Jira, 10)
            .unwrap();
        let LocalApiaryContext::Federated { apiary, .. } = context else {
            panic!("expected federated context");
        };
        let member_operator_id = OperatorId::new();
        let member_hive_id = HiveId::new();
        {
            let connection = store.connection().unwrap();
            insert_test_operator(&connection, member_operator_id, "Cora");
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id, apiary_id)
                     VALUES (?1, 'Clover Hive', ?2, ?3)",
                    params![
                        member_hive_id.to_string(),
                        member_operator_id.to_string(),
                        apiary.id.to_string()
                    ],
                )
                .unwrap();
        }

        let members = store.list_apiary_members().unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].hive_id, keeper.hive.id);
        assert_eq!(members[0].role, LocalApiaryRole::Keeper);
        assert!(members[0].is_local);
        assert_eq!(members[1].hive_id, member_hive_id);
        assert_eq!(members[1].operator_display_name, "Cora");
        assert_eq!(members[1].role, LocalApiaryRole::Member);
        assert!(!members[1].is_local);
    }

    fn insert_test_operator(connection: &Connection, operator_id: OperatorId, name: &str) {
        connection
            .execute(
                "INSERT INTO operators (id, display_name) VALUES (?1, ?2)",
                params![operator_id.to_string(), name],
            )
            .unwrap();
    }

    #[test]
    fn stewardship_scope_is_explicit_durable_and_apiary_bounded() {
        let store = TaskStore::in_memory().unwrap();
        let identity = store.local_hive_identity().unwrap();
        let apiary_id = ApiaryId::new();
        let steward_operator_id = OperatorId::new();
        let stewardship_id = StewardshipId::new();
        let managed_hive_id = HiveId::new();
        let outside_operator_id = OperatorId::new();
        let outside_hive_id = HiveId::new();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO apiaries (id, name, keeper_operator_id, shared_work_backend)
                     VALUES (?1, 'Garden', ?2, 'jira')",
                    params![apiary_id.to_string(), identity.operator.id.to_string()],
                )
                .unwrap();
            insert_test_operator(&connection, steward_operator_id, "Steward");
            insert_test_operator(&connection, outside_operator_id, "Outside");
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id, apiary_id)
                     VALUES (?1, 'Managed Hive', ?2, ?3)",
                    params![
                        managed_hive_id.to_string(),
                        steward_operator_id.to_string(),
                        apiary_id.to_string()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id)
                     VALUES (?1, 'Outside Hive', ?2)",
                    params![outside_hive_id.to_string(), outside_operator_id.to_string()],
                )
                .unwrap();
            assert!(
                connection
                    .execute(
                        "INSERT INTO stewardships
                            (id, apiary_id, steward_operator_id, created_by_operator_id)
                         VALUES (?1, ?2, ?3, ?3)",
                        params![
                            StewardshipId::new().to_string(),
                            apiary_id.to_string(),
                            steward_operator_id.to_string()
                        ],
                    )
                    .is_err()
            );
            connection
                .execute(
                    "INSERT INTO stewardships
                        (id, apiary_id, steward_operator_id, created_by_operator_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        stewardship_id.to_string(),
                        apiary_id.to_string(),
                        steward_operator_id.to_string(),
                        identity.operator.id.to_string()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO stewardship_hive_grants (stewardship_id, hive_id)
                     VALUES (?1, ?2)",
                    params![stewardship_id.to_string(), managed_hive_id.to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO stewardship_capability_grants (stewardship_id, capability)
                     VALUES (?1, 'observe'), (?1, 'takeover')",
                    [stewardship_id.to_string()],
                )
                .unwrap();
            assert!(
                connection
                    .execute(
                        "INSERT INTO stewardship_hive_grants (stewardship_id, hive_id)
                         VALUES (?1, ?2)",
                        params![stewardship_id.to_string(), outside_hive_id.to_string()],
                    )
                    .is_err()
            );
        }

        assert_eq!(
            store.stewardships_for_apiary(apiary_id).unwrap(),
            vec![Stewardship {
                id: stewardship_id,
                apiary_id,
                steward_operator_id,
                managed_hive_ids: vec![managed_hive_id],
                capabilities: vec![StewardCapability::Observe, StewardCapability::Takeover],
            }]
        );
    }

    #[test]
    fn migrates_schema_v24_to_explicit_stewardship_grants() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TRIGGER stewardship_hive_scope_update;
                     DROP TRIGGER stewardship_hive_scope_insert;
                     DROP TABLE stewardship_capability_grants;
                     DROP TABLE stewardship_hive_grants;
                     DROP TABLE stewardships;
                     PRAGMA user_version = 24;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in [
            "stewardships",
            "stewardship_hive_grants",
            "stewardship_capability_grants",
        ] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn backup_is_consistent_and_reopenable() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.sqlite3");
        let backup = directory.path().join("backups").join("snapshot.sqlite3");
        let store = TaskStore::open(source).unwrap();
        let task = store.create_task("Backed up", "/workspace").unwrap();
        store.backup_to(&backup).unwrap();

        let restored = TaskStore::open(backup).unwrap();
        restored.verify_integrity().unwrap();
        assert_eq!(restored.get_task(task.id).unwrap().title, "Backed up");
    }

    #[test]
    fn task_and_worker_mutations_emit_typed_content_free_events() {
        let store = TaskStore::in_memory().unwrap();
        assert!(store.list_control_room_events(0).unwrap().events.is_empty());

        let task = store.create_task("Secret task text", "/workspace").unwrap();
        let worker = store
            .create_worker(
                "Private worker name",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        store
            .bind_worker_session(worker.id, WorkerSessionId::new())
            .unwrap();

        let page = store.list_control_room_events(0).unwrap();
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                ControlRoomEventKind::TasksChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::WorkersChanged,
                ControlRoomEventKind::SessionsChanged,
            ]
        );
        assert!(
            page.events
                .iter()
                .all(|event| event.hive_id == task.hive_id)
        );
        let serialized = serde_json::to_string(&page).unwrap();
        assert!(!serialized.contains("Secret task text"));
        assert!(!serialized.contains("Private worker name"));
    }

    #[test]
    fn control_room_event_log_is_bounded_and_stale_cursors_reset() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
            .unwrap();
        for _ in 0..=MAX_CONTROL_ROOM_EVENTS {
            store
                .record_control_room_event(ControlRoomEventKind::RuntimeChanged)
                .unwrap();
        }

        let connection = store.connection().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM control_room_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            MAX_CONTROL_ROOM_EVENTS
        );
        drop(connection);

        let stale = store.list_control_room_events(first.sequence).unwrap();
        assert!(stale.reset_required);
        assert_eq!(stale.events.len(), MAX_CONTROL_ROOM_EVENT_PAGE);
        let future = store.list_control_room_events(i64::MAX).unwrap();
        assert!(future.reset_required);
    }

    #[test]
    fn migrates_schema_v4_without_losing_existing_hive_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let (task_id, hive_id) = {
            let store = TaskStore::open(&path).unwrap();
            let task = store.create_task("Existing v4 task", "/workspace").unwrap();
            let hive_id = store.local_hive_identity().unwrap().hive.id;
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "DROP INDEX tasks_by_hive_position;
                     ALTER TABLE tasks DROP COLUMN position;
                     DROP TABLE worker_engagements;
                     ALTER TABLE worker_profiles DROP COLUMN provider_conversation_id;
                     DROP TABLE control_room_events;
                     PRAGMA user_version = 4;",
                )
                .unwrap();
            (task.id, hive_id)
        };

        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(migrated.get_task(task_id).unwrap().hive_id, hive_id);
        assert_eq!(migrated.get_task(task_id).unwrap().position, 0);
        assert!(
            migrated
                .list_control_room_events(0)
                .unwrap()
                .events
                .is_empty()
        );
        migrated.verify_integrity().unwrap();
    }

    #[test]
    fn migrates_schema_v6_without_assigning_ambiguous_existing_conversations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let worker_id = {
            let store = TaskStore::open(&path).unwrap();
            let worker = store
                .create_worker(
                    "Existing worker",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace",
                    false,
                    1,
                )
                .unwrap();
            let session = WorkerSessionId::new();
            store.bind_worker_session(worker.id, session).unwrap();
            store.release_worker_session(session).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE worker_engagements;
                     ALTER TABLE worker_profiles DROP COLUMN provider_conversation_id;
                     PRAGMA user_version = 6;",
                )
                .unwrap();
            worker.id
        };

        let migrated = TaskStore::open(path).unwrap();
        let worker = migrated.get_worker_profile(worker_id).unwrap();
        assert!(worker.has_session_history);
        assert_eq!(worker.provider_conversation_id, None);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn migrates_schema_v7_to_bounded_worker_engagements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE worker_engagements; PRAGMA user_version = 7;")
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let tables = migrated
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'worker_engagements'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }

    #[test]
    fn migrates_schema_v11_to_durable_task_dispatches() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch("DROP TABLE task_dispatches; PRAGMA user_version = 11;")
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let tables = migrated
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'task_dispatches'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v12_to_task_handoff_notes_and_outbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE task_outcome_deliveries;
                     ALTER TABLE task_activity DROP COLUMN note;
                     PRAGMA user_version = 12;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type = 'table' AND name = 'task_outcome_deliveries'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('task_activity') WHERE name = 'note'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v13_to_bounded_operator_presence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE operator_presence_devices;
                     DROP TABLE operator_presence_preferences;
                     PRAGMA user_version = 13;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in ["operator_presence_preferences", "operator_presence_devices"] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v14_to_bounded_mobile_attention() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE notification_deliveries;
                     DROP TABLE notification_subscriptions;
                     DROP TABLE notification_preferences;
                     DROP TABLE notification_vapid_keys;
                     PRAGMA user_version = 14;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        for table in [
            "notification_vapid_keys",
            "notification_preferences",
            "notification_subscriptions",
            "notification_deliveries",
        ] {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        drop(connection);
        migrated.verify_integrity().unwrap();
    }
    #[test]
    fn migrates_schema_v15_to_device_owned_engagements() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "ALTER TABLE worker_engagements DROP COLUMN owner_device_id;
                     PRAGMA user_version = 15;",
                )
                .unwrap();
        }

        let migrated = TaskStore::open(path).unwrap();
        let connection = migrated.connection().unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(worker_engagements)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.iter().any(|column| column == "owner_device_id"));
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v16_to_queen_autonomy_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE queen_autonomy_preferences;
                     PRAGMA user_version = 16;",
                )
                .unwrap();
        }
        let migrated = TaskStore::open(path).unwrap();
        assert_eq!(
            migrated.queen_autonomy_policy().unwrap(),
            swarm_domain::QueenAutonomyPolicy::default()
        );
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn migrates_schema_v17_to_device_presentation_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        {
            let store = TaskStore::open(&path).unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "DROP TABLE presentation_preferences;
                     PRAGMA user_version = 17;",
                )
                .unwrap();
        }
        let migrated = TaskStore::open(path).unwrap();
        assert!(
            !migrated
                .presentation_preferences(PresentationDeviceClass::Desktop)
                .unwrap()
                .configured
        );
        assert_eq!(
            migrated
                .connection()
                .unwrap()
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }
    #[test]
    fn fresh_store_owns_tasks_and_workers_in_one_durable_hive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let (hive_id, operator_id) = {
            let store = TaskStore::open(&path).unwrap();
            let identity = store.local_hive_identity().unwrap();
            assert_eq!(identity.operator.display_name, "Operator");
            assert_eq!(identity.hive.name, "My Hive");
            assert_eq!(identity.hive.operator_id, identity.operator.id);

            let task = store.create_task("Hive-owned task", "/workspace").unwrap();
            let worker = store
                .create_worker(
                    "Violet",
                    swarm_domain::ProviderKind::ClaudeCode,
                    "/workspace",
                    false,
                    1,
                )
                .unwrap();
            assert_eq!(task.hive_id, identity.hive.id);
            assert_eq!(worker.hive_id, identity.hive.id);
            (identity.hive.id, identity.operator.id)
        };

        let reopened = TaskStore::open(path).unwrap();
        let identity = reopened.local_hive_identity().unwrap();
        assert_eq!(identity.hive.id, hive_id);
        assert_eq!(identity.operator.id, operator_id);
        assert_eq!(reopened.list_tasks().unwrap()[0].hive_id, hive_id);
        assert_eq!(reopened.list_worker_profiles().unwrap()[0].hive_id, hive_id);
    }

    #[test]
    fn current_schema_requires_hive_ownership_columns() {
        let store = TaskStore::in_memory().unwrap();
        let connection = store.connection().unwrap();
        for table in ["tasks", "worker_profiles"] {
            let sql =
                format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = 'hive_id'");
            assert_eq!(
                connection
                    .query_row(&sql, [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn migrates_schema_v43_to_durable_task_activity_actors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm-next.sqlite3");
        let task_id = {
            let store = TaskStore::open(&path).unwrap();
            let task = store
                .create_task("Existing activity", "/workspace")
                .unwrap();
            store
                .connection()
                .unwrap()
                .execute_batch(
                    "ALTER TABLE task_activity DROP COLUMN actor_id;
                     ALTER TABLE task_activity DROP COLUMN actor_kind;
                     PRAGMA user_version = 43;",
                )
                .unwrap();
            task.id
        };

        let migrated = TaskStore::open(path).unwrap();
        let columns = migrated
            .connection()
            .unwrap()
            .prepare("PRAGMA table_info(task_activity)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.contains(&"actor_kind".to_owned()));
        assert!(columns.contains(&"actor_id".to_owned()));
        let existing = migrated.list_task_activity(task_id, 10).unwrap();
        assert_eq!(existing.events[0].actor_kind, TaskActivityActorKind::System);
        assert_eq!(existing.events[0].actor_id, None);
        migrated.verify_integrity().unwrap();
    }
}
