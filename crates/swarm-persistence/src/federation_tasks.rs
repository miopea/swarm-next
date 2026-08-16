use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ApiaryId, ApiaryTask, ApiaryTaskEvent, ApiaryTaskId, ApiaryTaskSource,
    FEDERATION_PROTOCOL_VERSION, FEDERATION_TASK_FEED_SCHEMA_VERSION, FederationNodeId,
    FederationTaskCommand, FederationTaskCommandId, FederationTaskCommandKind,
    FederationTaskCommandOutcome, FederationTaskCommandReceipt, FederationTaskOutboxEntry,
    FederationTaskOutboxState, FederationTaskOutboxStatus, FederationTaskPage,
    FederationTaskSyncStatus, HiveId, LocalApiaryContext, LocalApiaryRole,
    LocalApiaryTaskExecution, TaskActivityActorKind, TaskId, TaskPriority, TaskState, WorkerId,
};

use crate::{
    ControlRoomEventKind, MAX_TASK_DESCRIPTION_BYTES, MAX_TASK_TITLE_BYTES, TaskStore,
    TaskStoreError,
    federation::{authenticate_member_credential, decode_node_credential},
    insert_control_room_event, parse_domain_id,
};

const MAX_FEDERATION_TASK_PAGE: usize = 100;
pub(crate) const MAX_APIARY_TASKS: usize = 10_000;
const MAX_APIARY_TASK_COMMANDS: usize = 10_000;
const MAX_LOCAL_TASK_OUTBOX: usize = 1_024;
pub const MAX_FEDERATION_TASK_COMMAND_BATCH: usize = 20;

impl TaskStore {
    /// Creates one Keeper-canonical Swarm task and appends its first ordered
    /// federation event atomically. Jira is not read or mutated.
    ///
    /// # Errors
    /// Rejects non-Keepers, invalid or over-capacity content, and persistence failures.
    pub fn create_apiary_task(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        now: i64,
    ) -> Result<ApiaryTask, TaskStoreError> {
        self.create_apiary_task_for_hive(title, description, priority, None, now)
    }

    /// Creates one Keeper-canonical Swarm task, optionally routing its durable
    /// home to one active Member Hive. The Keeper never selects a private
    /// worker, repository, terminal, or provider session.
    ///
    /// # Errors
    /// Rejects non-Keepers, unknown or departed target Hives, invalid content,
    /// capacity exhaustion, and persistence failures.
    pub fn create_apiary_task_for_hive(
        &self,
        title: &str,
        description: &str,
        priority: TaskPriority,
        home_hive_id: Option<HiveId>,
        now: i64,
    ) -> Result<ApiaryTask, TaskStoreError> {
        let title = title.trim();
        if title.is_empty()
            || title.len() > MAX_TASK_TITLE_BYTES
            || description.len() > MAX_TASK_DESCRIPTION_BYTES
            || now < 0
        {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        };
        if local_role != LocalApiaryRole::Keeper
            || apiary.keeper_operator_id != identity.operator.id
        {
            return Err(TaskStoreError::ApiaryKeeperRequired);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task = insert_apiary_task_for_hive(
            &transaction,
            apiary.id,
            title,
            description,
            priority,
            home_hive_id,
            now,
        )?;
        transaction.commit()?;
        Ok(task)
    }

    /// Returns one bounded ordered page to the exact authenticated member.
    /// The feed contains Swarm tasks only and never Jira issue content.
    ///
    /// # Errors
    /// Rejects invalid credentials/cursors, non-Keepers, corrupt records, and storage failures.
    pub fn federation_task_page(
        &self,
        node_credential: &str,
        after: i64,
        now: i64,
    ) -> Result<FederationTaskPage, TaskStoreError> {
        if after < 0 || now < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let credential = decode_node_credential(node_credential)?;
        let identity = self.local_hive_identity()?;
        let connection = self.connection()?;
        let member = authenticate_member_credential(&connection, &identity, &credential, now)?;
        let mut statement = connection.prepare(
            "SELECT sequence, snapshot_json FROM apiary_task_events
             WHERE apiary_id = ?1 AND sequence > ?2
             ORDER BY sequence ASC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![
                member.apiary.to_string(),
                after,
                MAX_FEDERATION_TASK_PAGE + 1
            ],
            apiary_task_event_from_row,
        )?;
        let mut events = rows.collect::<Result<Vec<_>, _>>()?;
        let has_more = events.len() > MAX_FEDERATION_TASK_PAGE;
        events.truncate(MAX_FEDERATION_TASK_PAGE);
        let next_cursor = events.last().map_or(after, |event| event.sequence);
        Ok(FederationTaskPage {
            schema_version: FEDERATION_TASK_FEED_SCHEMA_VERSION,
            protocol_version: FEDERATION_PROTOCOL_VERSION,
            apiary_id: member.apiary,
            member_node_id: member.node,
            events,
            next_cursor,
            has_more,
            generated_at: now,
        })
    }

    /// Atomically applies one Keeper page to the member projection and advances
    /// its durable cursor. Exact retries are harmless; gaps fail closed.
    ///
    /// # Errors
    /// Rejects non-Members, foreign, malformed, oversized, or gapped pages and storage failures.
    pub fn apply_federation_task_page(
        &self,
        page: &FederationTaskPage,
        now: i64,
    ) -> Result<FederationTaskSyncStatus, TaskStoreError> {
        self.require_local_federation_member()?;
        if page.schema_version != FEDERATION_TASK_FEED_SCHEMA_VERSION
            || page.protocol_version != FEDERATION_PROTOCOL_VERSION
            || page.next_cursor < 0
            || page.generated_at < 0
            || now < 0
            || page.events.len() > MAX_FEDERATION_TASK_PAGE
        {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationTask);
        };
        if local_role != LocalApiaryRole::Member || apiary.id != page.apiary_id {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let connection = self.connection()?;
        let receipt_json = connection.query_row(
            "SELECT receipt_json FROM local_federation_membership WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let receipt: swarm_domain::FederationMembershipReceipt =
            serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
        if receipt.payload.member_node_id != page.member_node_id {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        drop(connection);

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior_cursor = transaction
            .query_row(
                "SELECT cursor FROM local_apiary_task_sync WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        if page.next_cursor <= prior_cursor {
            transaction.commit()?;
            drop(connection);
            return self.federation_task_sync_status();
        }
        let first_new = page
            .events
            .iter()
            .find(|event| event.sequence > prior_cursor)
            .ok_or(TaskStoreError::InvalidFederationTask)?;
        if first_new.sequence != prior_cursor.saturating_add(1)
            || page.events.last().map(|event| event.sequence) != Some(page.next_cursor)
            || page
                .events
                .windows(2)
                .any(|pair| pair[1].sequence != pair[0].sequence + 1)
        {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        for event in page
            .events
            .iter()
            .filter(|event| event.sequence > prior_cursor)
        {
            validate_apiary_task(&event.task, page.apiary_id)?;
            let serialized = serde_json::to_string(&event.task)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
            transaction.execute(
                "INSERT INTO local_apiary_tasks
                    (task_id, apiary_id, revision, snapshot_json, last_event_sequence, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(task_id) DO UPDATE SET
                    revision = excluded.revision,
                    snapshot_json = excluded.snapshot_json,
                    last_event_sequence = excluded.last_event_sequence,
                    updated_at = excluded.updated_at
                 WHERE excluded.revision >= local_apiary_tasks.revision",
                params![
                    event.task.id.to_string(),
                    event.task.apiary_id.to_string(),
                    event.task.revision,
                    serialized,
                    event.sequence,
                    now,
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO local_apiary_task_sync (singleton, cursor, last_applied_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
                cursor = excluded.cursor, last_applied_at = excluded.last_applied_at",
            params![page.next_cursor, now],
        )?;
        transaction.commit()?;
        drop(connection);
        self.federation_task_sync_status()
    }

    /// Returns content-free durable member projection evidence.
    ///
    /// # Errors
    /// Returns an error when projection state is corrupt or unavailable.
    pub fn federation_task_sync_status(&self) -> Result<FederationTaskSyncStatus, TaskStoreError> {
        let connection = self.connection()?;
        let (cursor, last_applied_at) = connection
            .query_row(
                "SELECT cursor, last_applied_at FROM local_apiary_task_sync WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .unwrap_or((0, None));
        let task_count =
            connection.query_row("SELECT COUNT(*) FROM local_apiary_tasks", [], |row| {
                row.get::<_, usize>(0)
            })?;
        Ok(FederationTaskSyncStatus {
            cursor,
            task_count,
            last_applied_at,
        })
    }

    /// Lists the last verified member-local task snapshots.
    ///
    /// # Errors
    /// Returns an error when a snapshot is corrupt or storage is unavailable.
    pub fn list_local_apiary_tasks(&self) -> Result<Vec<ApiaryTask>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT snapshot_json FROM local_apiary_tasks ORDER BY updated_at DESC, task_id",
        )?;
        statement
            .query_map([], |row| {
                let value = row.get::<_, String>(0)?;
                serde_json::from_str(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Lists the canonical Keeper tasks or the Member's last durable projection,
    /// depending on the local role. No network or Jira operation occurs.
    ///
    /// # Errors
    /// Returns an error for invalid membership, corrupt snapshots, or unavailable storage.
    pub fn list_visible_apiary_tasks(&self) -> Result<Vec<ApiaryTask>, TaskStoreError> {
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Ok(Vec::new());
        };
        if local_role == LocalApiaryRole::Member {
            return self.list_local_apiary_tasks();
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, apiary_id, source, title, description, priority, state,
                    home_node_id, home_hive_id, revision, created_at, updated_at
             FROM apiary_tasks WHERE apiary_id = ?1
             ORDER BY CASE state WHEN 'completed' THEN 1 ELSE 0 END, updated_at DESC, id",
        )?;
        statement
            .query_map([apiary.id.to_string()], apiary_task_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Materializes one Keeper-canonical task as private executable work in
    /// the owning Member Hive and assigns it to one local non-Queen worker.
    /// Exact retries return the original bridge and never duplicate work.
    ///
    /// # Errors
    /// Rejects non-Members, work owned by another Hive, work that is no longer
    /// Ready, unsettled shared commands, unknown/private-Queen workers, invalid
    /// time, or unavailable storage.
    #[allow(clippy::too_many_lines)] // Validation, task creation, assignment, and the durable bridge must commit atomically.
    pub fn materialize_local_apiary_task_execution(
        &self,
        apiary_task_id: ApiaryTaskId,
        worker_id: WorkerId,
        now: i64,
    ) -> Result<LocalApiaryTaskExecution, TaskStoreError> {
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { local_role, .. } = self.local_apiary_context()? else {
            return Err(TaskStoreError::InvalidFederationTask);
        };
        if local_role != LocalApiaryRole::Member {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(existing) = local_apiary_task_execution(&transaction, apiary_task_id)? {
            transaction.commit()?;
            return Ok(existing);
        }
        let receipt_json = transaction.query_row(
            "SELECT receipt_json FROM local_federation_membership
             WHERE singleton = 1 AND state = 'active'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let membership: swarm_domain::FederationMembershipReceipt =
            serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let snapshot_json = transaction
            .query_row(
                "SELECT snapshot_json FROM local_apiary_tasks WHERE task_id = ?1",
                [apiary_task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationTask)?;
        let shared: ApiaryTask = serde_json::from_str(&snapshot_json)
            .map_err(|_| TaskStoreError::InvalidFederationTask)?;
        if shared.home_hive_id != Some(identity.hive.id)
            || shared.home_node_id != Some(membership.payload.member_node_id)
            || shared.state != TaskState::Ready
        {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let unsettled_command = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM local_apiary_task_commands
                 WHERE task_id = ?1 AND (
                     state IN ('queued','conflict','rejected') OR
                     (state = 'applied' AND expected_revision >= ?2)
                 )
             )",
            params![apiary_task_id.to_string(), shared.revision],
            |row| row.get::<_, bool>(0),
        )?;
        if unsettled_command {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let (workspace, active_session): (String, Option<String>) = transaction
            .query_row(
                "SELECT profile.workspace, session.session_id
                 FROM worker_profiles profile
                 LEFT JOIN worker_sessions session
                   ON session.worker_id = profile.id AND session.ended_at IS NULL
                 WHERE profile.id = ?1 AND profile.hive_id = ?2
                   AND profile.role != 'queen' AND profile.archived_at IS NULL",
                params![worker_id.to_string(), identity.hive.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(TaskStoreError::WorkerNotFound)?;
        let local_task_id = TaskId::new();
        transaction.execute(
            "INSERT INTO tasks
                (id, hive_id, title, description, priority, workspace, state,
                 assigned_worker_id, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ready', ?7,
                     COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0),
                     ?8, ?8)",
            params![
                local_task_id.to_string(),
                identity.hive.id.to_string(),
                shared.title,
                shared.description,
                shared.priority.to_string(),
                workspace,
                worker_id.to_string(),
                now,
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity
                (task_id, kind, to_state, actor_kind, actor_id, occurred_at)
             VALUES (?1, 'created', 'ready', ?2, NULL, ?3)",
            params![
                local_task_id.to_string(),
                TaskActivityActorKind::System.to_string(),
                now,
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_activity
                (task_id, kind, actor_kind, actor_id, occurred_at)
             VALUES (?1, 'assigned', ?2, NULL, ?3)",
            params![
                local_task_id.to_string(),
                TaskActivityActorKind::System.to_string(),
                now,
            ],
        )?;
        if let Some(session_id) = active_session {
            let queued: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
                [],
                |row| row.get(0),
            )?;
            if queued >= 256 {
                return Err(TaskStoreError::TaskDispatchQueueFull);
            }
            let assignment_id = uuid::Uuid::now_v7().to_string();
            transaction.execute(
                "INSERT INTO task_assignments (id, task_id, worker_session_id, assigned_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![assignment_id, local_task_id.to_string(), session_id, now],
            )?;
            transaction.execute(
                "INSERT INTO task_dispatches
                    (assignment_id, task_id, worker_id, state, updated_at)
                 VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![
                    assignment_id,
                    local_task_id.to_string(),
                    worker_id.to_string(),
                    now,
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO local_apiary_task_executions
                (apiary_task_id, local_task_id, worker_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                apiary_task_id.to_string(),
                local_task_id.to_string(),
                worker_id.to_string(),
                now,
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(LocalApiaryTaskExecution {
            apiary_task_id,
            local_task_id,
            worker_id,
            state: TaskState::Ready,
            created_at: now,
        })
    }

    /// Lists only the private execution bridges stored by this Hive.
    ///
    /// # Errors
    /// Returns an error when local state is corrupt or unavailable.
    pub fn list_local_apiary_task_executions(
        &self,
    ) -> Result<Vec<LocalApiaryTaskExecution>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT execution.apiary_task_id, execution.local_task_id,
                    execution.worker_id, task.state, execution.created_at
             FROM local_apiary_task_executions execution
             JOIN tasks task ON task.id = execution.local_task_id
             ORDER BY execution.created_at DESC, execution.apiary_task_id",
        )?;
        statement
            .query_map([], local_apiary_task_execution_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Applies one authenticated, revision-checked Member command on Keeper.
    /// Exact retries return the original durable receipt; altered replays fail.
    /// Jira is never read or mutated on this path.
    ///
    /// # Errors
    /// Rejects invalid credentials, foreign/malformed commands, altered replay,
    /// capacity exhaustion, corrupt state, and unavailable persistence.
    #[allow(clippy::too_many_lines)] // One transaction deliberately owns authentication, idempotency, mutation, event, and receipt.
    pub fn apply_federation_task_command(
        &self,
        node_credential: &str,
        command: &FederationTaskCommand,
        now: i64,
    ) -> Result<FederationTaskCommandReceipt, TaskStoreError> {
        validate_federation_task_command(command, now)?;
        let identity = self.local_hive_identity()?;
        let credential = decode_node_credential(node_credential)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let member = authenticate_member_credential(&transaction, &identity, &credential, now)?;
        if command.apiary_id != member.apiary {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let command_json =
            serde_json::to_string(command).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let existing = transaction
            .query_row(
                "SELECT member_node_id, command_json, receipt_json
                 FROM apiary_task_commands WHERE command_id = ?1",
                [command.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((member_node_id, prior_command, receipt_json)) = existing {
            if member_node_id != member.node.to_string() || prior_command != command_json {
                return Err(TaskStoreError::InvalidFederationTask);
            }
            return serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask);
        }
        let command_count = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_task_commands WHERE apiary_id = ?1",
            [member.apiary.to_string()],
            |row| row.get::<_, usize>(0),
        )?;
        if command_count >= MAX_APIARY_TASK_COMMANDS {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let mut task = transaction
            .query_row(
                "SELECT id, apiary_id, source, title, description, priority, state,
                        home_node_id, home_hive_id, revision, created_at, updated_at
                 FROM apiary_tasks WHERE apiary_id = ?1 AND id = ?2",
                params![member.apiary.to_string(), command.task_id.to_string()],
                apiary_task_from_row,
            )
            .optional()?;
        let outcome = match task.as_mut() {
            None => FederationTaskCommandOutcome::Rejected,
            Some(task) if task.revision != command.expected_revision => {
                FederationTaskCommandOutcome::Conflict
            }
            Some(task) => match command.kind {
                FederationTaskCommandKind::Claim if command.target_state.is_none() => {
                    if task.home_node_id.is_some() || task.state == TaskState::Completed {
                        FederationTaskCommandOutcome::Conflict
                    } else {
                        task.home_node_id = Some(member.node);
                        task.home_hive_id = Some(member.hive);
                        FederationTaskCommandOutcome::Applied
                    }
                }
                FederationTaskCommandKind::Transition => {
                    let Some(target) = command.target_state else {
                        return Err(TaskStoreError::InvalidFederationTask);
                    };
                    if task.home_node_id != Some(member.node)
                        || task.home_hive_id != Some(member.hive)
                    {
                        FederationTaskCommandOutcome::Conflict
                    } else if !task.state.can_transition_to(target) {
                        FederationTaskCommandOutcome::Rejected
                    } else {
                        task.state = target;
                        FederationTaskCommandOutcome::Applied
                    }
                }
                FederationTaskCommandKind::Claim => FederationTaskCommandOutcome::Rejected,
            },
        };
        if outcome == FederationTaskCommandOutcome::Applied {
            let task = task.as_mut().ok_or(TaskStoreError::InvalidFederationTask)?;
            task.revision = task
                .revision
                .checked_add(1)
                .ok_or(TaskStoreError::InvalidFederationTask)?;
            task.updated_at = now;
            let changed = transaction.execute(
                "UPDATE apiary_tasks SET state = ?1, home_node_id = ?2,
                        home_hive_id = ?3, revision = ?4, updated_at = ?5
                 WHERE id = ?6 AND apiary_id = ?7 AND revision = ?8",
                params![
                    task.state.to_string(),
                    task.home_node_id.map(|id| id.to_string()),
                    task.home_hive_id.map(|id| id.to_string()),
                    task.revision,
                    now,
                    task.id.to_string(),
                    task.apiary_id.to_string(),
                    command.expected_revision,
                ],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::InvalidFederationTask);
            }
            insert_task_event(&transaction, task, now)?;
        }
        let task_revision = task.as_ref().map(|task| task.revision);
        let receipt = insert_task_command_receipt(
            &transaction,
            &member,
            command,
            &command_json,
            outcome,
            task_revision,
            now,
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    /// Queues an offline-safe claim of one unassigned Keeper task for this
    /// Member Hive. Repeated clicks reuse the same queued command.
    ///
    /// # Errors
    /// Rejects non-Members, stale/owned/completed tasks, capacity exhaustion,
    /// and unavailable persistence.
    pub fn queue_federation_task_claim(
        &self,
        task_id: ApiaryTaskId,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, TaskStoreError> {
        self.queue_federation_task_command(task_id, FederationTaskCommandKind::Claim, None, now)
    }

    /// Queues an offline-safe lifecycle transition for one task already owned
    /// by this Member Hive.
    ///
    /// # Errors
    /// Rejects non-Members, foreign/stale tasks, invalid transitions, capacity
    /// exhaustion, and unavailable persistence.
    pub fn queue_federation_task_transition(
        &self,
        task_id: ApiaryTaskId,
        target_state: TaskState,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, TaskStoreError> {
        self.queue_federation_task_command(
            task_id,
            FederationTaskCommandKind::Transition,
            Some(target_state),
            now,
        )
    }

    /// Converts durable local task intent into at most one next legal Keeper
    /// transition per linked task. A newer transition is not staged until the
    /// prior receipt has appeared in the canonical Member projection.
    ///
    /// # Errors
    /// Rejects invalid Member state, corrupt projections, exhausted capacity,
    /// and unavailable persistence.
    pub fn prepare_local_apiary_task_lifecycle_commands(
        &self,
        now: i64,
    ) -> Result<usize, TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationTask);
        };
        if local_role != LocalApiaryRole::Member {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let receipt_json = transaction.query_row(
            "SELECT receipt_json FROM local_federation_membership
             WHERE singleton = 1 AND state = 'active'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let membership: swarm_domain::FederationMembershipReceipt =
            serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT intent.apiary_task_id, intent.desired_state, task.snapshot_json
                 FROM local_apiary_task_lifecycle_intents intent
                 JOIN local_apiary_tasks task ON task.task_id = intent.apiary_task_id
                 ORDER BY intent.updated_at, intent.apiary_task_id LIMIT 100",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut staged = 0;
        for (task_id, desired_state, task_json) in candidates {
            let task_id = ApiaryTaskId::from_str(&task_id)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
            let desired_state = TaskState::from_str(&desired_state)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
            let task: ApiaryTask = serde_json::from_str(&task_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
            if task.apiary_id != apiary.id
                || task.home_hive_id != Some(identity.hive.id)
                || task.home_node_id != Some(membership.payload.member_node_id)
            {
                return Err(TaskStoreError::InvalidFederationTask);
            }
            if task.state == desired_state {
                transaction.execute(
                    "DELETE FROM local_apiary_task_lifecycle_intents WHERE apiary_task_id = ?1",
                    [task_id.to_string()],
                )?;
                continue;
            }
            let blocked = transaction.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM local_apiary_task_commands
                     WHERE task_id = ?1 AND (
                         state IN ('queued','conflict','rejected') OR
                         (state = 'applied' AND expected_revision >= ?2)
                     )
                 )",
                params![task_id.to_string(), task.revision],
                |row| row.get::<_, bool>(0),
            )?;
            if blocked {
                continue;
            }
            let Some(target_state) = next_lifecycle_transition(task.state, desired_state) else {
                continue;
            };
            insert_local_lifecycle_command(
                &transaction,
                apiary.id,
                task_id,
                task.revision,
                target_state,
                now,
            )?;
            staged += 1;
        }
        if staged > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(staged)
    }

    #[allow(clippy::too_many_lines)] // Validation and insertion stay in one transaction to preserve the offline command contract.
    fn queue_federation_task_command(
        &self,
        task_id: ApiaryTaskId,
        kind: FederationTaskCommandKind,
        target_state: Option<TaskState>,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let identity = self.local_hive_identity()?;
        let LocalApiaryContext::Federated { apiary, local_role } = self.local_apiary_context()?
        else {
            return Err(TaskStoreError::InvalidFederationTask);
        };
        if local_role != LocalApiaryRole::Member {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if !transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_federation_membership
             WHERE singleton = 1 AND state = 'active')",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(TaskStoreError::InvalidFederationSync);
        }
        let receipt_json = transaction.query_row(
            "SELECT receipt_json FROM local_federation_membership WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let membership: swarm_domain::FederationMembershipReceipt =
            serde_json::from_str(&receipt_json)
                .map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let task_json = transaction
            .query_row(
                "SELECT snapshot_json FROM local_apiary_tasks
                 WHERE task_id = ?1 AND apiary_id = ?2",
                params![task_id.to_string(), apiary.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationTask)?;
        let task: ApiaryTask =
            serde_json::from_str(&task_json).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let has_local_execution = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_apiary_task_executions WHERE apiary_task_id = ?1)",
            [task_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        match kind {
            FederationTaskCommandKind::Claim
                if target_state.is_none()
                    && task.home_node_id.is_none()
                    && task.state != TaskState::Completed => {}
            FederationTaskCommandKind::Transition
                if !has_local_execution
                    && task.home_hive_id == Some(identity.hive.id)
                    && task.home_node_id == Some(membership.payload.member_node_id)
                    && target_state.is_some_and(|target| task.state.can_transition_to(target)) => {}
            _ => return Err(TaskStoreError::InvalidFederationTask),
        }
        let target_value = target_state.map(|state| state.to_string());
        if let Some(existing) = transaction
            .query_row(
                "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
                 FROM local_apiary_task_commands
                 WHERE task_id = ?1 AND kind = ?2
                   AND COALESCE(target_state, '') = COALESCE(?3, '') AND state = 'queued'
                 ORDER BY created_at ASC LIMIT 1",
                params![task_id.to_string(), kind.to_string(), target_value],
                federation_task_outbox_entry_from_row,
            )
            .optional()?
        {
            return Ok(existing);
        }
        let queued_count = transaction.query_row(
            "SELECT COUNT(*) FROM local_apiary_task_commands WHERE state = 'queued'",
            [],
            |row| row.get::<_, usize>(0),
        )?;
        if queued_count >= MAX_LOCAL_TASK_OUTBOX {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        transaction.execute(
            "DELETE FROM local_apiary_task_commands
             WHERE task_id = ?1 AND kind = ?2
               AND COALESCE(target_state, '') = COALESCE(?3, '')
               AND state IN ('conflict','rejected')",
            params![task_id.to_string(), kind.to_string(), target_value],
        )?;
        let command = FederationTaskCommand {
            id: FederationTaskCommandId::new(),
            apiary_id: apiary.id,
            task_id,
            expected_revision: task.revision,
            kind,
            target_state,
            created_at: now,
        };
        let command_json =
            serde_json::to_string(&command).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        transaction.execute(
            "INSERT INTO local_apiary_task_commands
                (command_id, apiary_id, task_id, expected_revision, kind,
                 target_state, command_json, state, attempt_count, last_attempt_at,
                 receipt_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', 0, NULL, NULL, ?8, ?8)",
            params![
                command.id.to_string(),
                command.apiary_id.to_string(),
                command.task_id.to_string(),
                command.expected_revision,
                command.kind.to_string(),
                command.target_state.map(|state| state.to_string()),
                command_json,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(FederationTaskOutboxEntry {
            command,
            state: FederationTaskOutboxState::Queued,
            attempt_count: 0,
            last_attempt_at: None,
            receipt: None,
        })
    }

    /// Returns the oldest bounded batch waiting to be sent to Keeper.
    ///
    /// # Errors
    /// Returns an error for invalid limits, corrupt state, or unavailable storage.
    pub fn pending_federation_task_commands(
        &self,
        limit: usize,
    ) -> Result<Vec<FederationTaskOutboxEntry>, TaskStoreError> {
        if limit == 0 || limit > MAX_FEDERATION_TASK_COMMAND_BATCH {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_apiary_task_commands WHERE state = 'queued'
             ORDER BY created_at ASC, command_id ASC LIMIT ?1",
        )?;
        let limit = i64::try_from(limit).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        statement
            .query_map([limit], federation_task_outbox_entry_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Records an outbound attempt before network I/O so a crash cannot erase
    /// evidence that the Keeper may have received the command.
    ///
    /// # Errors
    /// Rejects unknown/non-queued commands, invalid time, and persistence failures.
    pub fn record_federation_task_command_attempt(
        &self,
        command_id: FederationTaskCommandId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE local_apiary_task_commands
             SET attempt_count = attempt_count + 1, last_attempt_at = ?1, updated_at = ?1
             WHERE command_id = ?2 AND state = 'queued'",
            params![now, command_id.to_string()],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        Ok(())
    }

    /// Applies one Keeper receipt to the exact queued command. Exact receipt
    /// retries are harmless; mismatched receipts fail closed.
    ///
    /// # Errors
    /// Rejects unknown commands, altered receipts, invalid time, and storage failures.
    pub fn apply_federation_task_command_receipt(
        &self,
        receipt: &FederationTaskCommandReceipt,
        now: i64,
    ) -> Result<FederationTaskOutboxEntry, TaskStoreError> {
        self.require_local_federation_member()?;
        if now < 0 || receipt.processed_at < 0 {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        let receipt_json =
            serde_json::to_string(receipt).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior = transaction
            .query_row(
                "SELECT receipt_json FROM local_apiary_task_commands WHERE command_id = ?1",
                [receipt.command_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .ok_or(TaskStoreError::InvalidFederationTask)?;
        if let Some(prior) = prior {
            if prior != receipt_json {
                return Err(TaskStoreError::InvalidFederationTask);
            }
        } else {
            let state = match receipt.outcome {
                FederationTaskCommandOutcome::Applied => FederationTaskOutboxState::Applied,
                FederationTaskCommandOutcome::Conflict => FederationTaskOutboxState::Conflict,
                FederationTaskCommandOutcome::Rejected => FederationTaskOutboxState::Rejected,
            };
            transaction.execute(
                "UPDATE local_apiary_task_commands
                 SET state = ?1, receipt_json = ?2, updated_at = ?3
                 WHERE command_id = ?4 AND state = 'queued'",
                params![
                    state.to_string(),
                    receipt_json,
                    now,
                    receipt.command_id.to_string(),
                ],
            )?;
        }
        let entry = transaction.query_row(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_apiary_task_commands WHERE command_id = ?1",
            [receipt.command_id.to_string()],
            federation_task_outbox_entry_from_row,
        )?;
        transaction.execute(
            "DELETE FROM local_apiary_task_commands WHERE command_id IN (
                 SELECT command_id FROM local_apiary_task_commands WHERE state = 'applied'
                 ORDER BY updated_at DESC, command_id DESC LIMIT -1 OFFSET 512
             )",
            [],
        )?;
        transaction.commit()?;
        Ok(entry)
    }

    /// Returns a bounded operator-visible outbox with queued and attention
    /// records first. No transport secrets are present.
    ///
    /// # Errors
    /// Returns an error for corrupt or unavailable persistence.
    pub fn list_federation_task_outbox(
        &self,
    ) -> Result<Vec<FederationTaskOutboxEntry>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT command_json, state, attempt_count, last_attempt_at, receipt_json
             FROM local_apiary_task_commands
             ORDER BY CASE state WHEN 'queued' THEN 0 WHEN 'conflict' THEN 1
                       WHEN 'rejected' THEN 2 ELSE 3 END,
                      updated_at DESC, command_id DESC LIMIT 100",
        )?;
        statement
            .query_map([], federation_task_outbox_entry_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Returns content-free queue and attention counts for Member UI.
    ///
    /// # Errors
    /// Returns an error for unavailable persistence.
    pub fn federation_task_outbox_status(
        &self,
    ) -> Result<FederationTaskOutboxStatus, TaskStoreError> {
        let connection = self.connection()?;
        let (queued_count, conflict_count, rejected_count, last_attempt_at) = connection
            .query_row(
                "SELECT
                SUM(CASE WHEN state = 'queued' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'conflict' THEN 1 ELSE 0 END),
                SUM(CASE WHEN state = 'rejected' THEN 1 ELSE 0 END),
                MAX(last_attempt_at)
             FROM local_apiary_task_commands",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<usize>>(0)?.unwrap_or(0),
                        row.get::<_, Option<usize>>(1)?.unwrap_or(0),
                        row.get::<_, Option<usize>>(2)?.unwrap_or(0),
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )?;
        Ok(FederationTaskOutboxStatus {
            queued_count,
            conflict_count,
            rejected_count,
            last_attempt_at,
        })
    }
}

/// Inserts one already-authorized Keeper task and its first ordered event in
/// the caller's transaction. Authorization remains the caller's responsibility.
pub(crate) fn insert_apiary_task_for_hive(
    transaction: &rusqlite::Transaction<'_>,
    apiary_id: ApiaryId,
    title: &str,
    description: &str,
    priority: TaskPriority,
    home_hive_id: Option<HiveId>,
    now: i64,
) -> Result<ApiaryTask, TaskStoreError> {
    let home_node_id = home_hive_id
        .map(|hive_id| {
            let raw_node_id = transaction
                .query_row(
                    "SELECT member_node_id FROM apiary_federation_memberships
                     WHERE apiary_id = ?1 AND member_hive_id = ?2 AND state = 'active'",
                    params![apiary_id.to_string(), hive_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(TaskStoreError::InvalidFederationTask)?;
            parse_domain_id::<FederationNodeId>(&raw_node_id).map_err(TaskStoreError::from)
        })
        .transpose()?;
    let task = ApiaryTask {
        id: ApiaryTaskId::new(),
        apiary_id,
        source: ApiaryTaskSource::Swarm,
        title: title.to_owned(),
        description: description.to_owned(),
        priority,
        state: TaskState::Ready,
        home_node_id,
        home_hive_id,
        revision: 1,
        created_at: now,
        updated_at: now,
    };
    let count = transaction.query_row(
        "SELECT COUNT(*) FROM apiary_tasks WHERE apiary_id = ?1",
        [apiary_id.to_string()],
        |row| row.get::<_, usize>(0),
    )?;
    if count >= MAX_APIARY_TASKS {
        return Err(TaskStoreError::InvalidFederationTask);
    }
    transaction.execute(
        "INSERT INTO apiary_tasks
            (id, apiary_id, source, title, description, priority, state,
             home_node_id, home_hive_id, revision, created_at, updated_at)
         VALUES (?1, ?2, 'swarm', ?3, ?4, ?5, 'ready', ?6, ?7, 1, ?8, ?8)",
        params![
            task.id.to_string(),
            task.apiary_id.to_string(),
            task.title,
            task.description,
            task.priority.to_string(),
            task.home_node_id.map(|id| id.to_string()),
            task.home_hive_id.map(|id| id.to_string()),
            now,
        ],
    )?;
    insert_task_event(transaction, &task, now)?;
    Ok(task)
}

fn validate_apiary_task(task: &ApiaryTask, apiary_id: ApiaryId) -> Result<(), TaskStoreError> {
    if task.apiary_id != apiary_id
        || task.source != ApiaryTaskSource::Swarm
        || task.title.trim().is_empty()
        || task.title.len() > MAX_TASK_TITLE_BYTES
        || task.description.len() > MAX_TASK_DESCRIPTION_BYTES
        || task.revision == 0
        || task.created_at < 0
        || task.updated_at < task.created_at
        || task.home_node_id.is_some() != task.home_hive_id.is_some()
    {
        return Err(TaskStoreError::InvalidFederationTask);
    }
    Ok(())
}

fn validate_federation_task_command(
    command: &FederationTaskCommand,
    now: i64,
) -> Result<(), TaskStoreError> {
    if now < 0
        || command.created_at < 0
        || command.created_at > now
        || command.expected_revision == 0
        || matches!(command.kind, FederationTaskCommandKind::Claim)
            && command.target_state.is_some()
        || matches!(command.kind, FederationTaskCommandKind::Transition)
            && command.target_state.is_none()
    {
        return Err(TaskStoreError::InvalidFederationTask);
    }
    Ok(())
}

fn insert_task_event(
    transaction: &rusqlite::Transaction<'_>,
    task: &ApiaryTask,
    now: i64,
) -> Result<(), TaskStoreError> {
    let snapshot_json =
        serde_json::to_string(task).map_err(|_| TaskStoreError::InvalidFederationTask)?;
    transaction.execute(
        "INSERT INTO apiary_task_events
            (apiary_id, sequence, task_id, task_revision, snapshot_json, occurred_at)
         VALUES (
            ?1,
            (SELECT COALESCE(MAX(sequence), 0) + 1
             FROM apiary_task_events WHERE apiary_id = ?1),
            ?2, ?3, ?4, ?5
         )",
        params![
            task.apiary_id.to_string(),
            task.id.to_string(),
            task.revision,
            snapshot_json,
            now,
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_task_command_receipt(
    transaction: &rusqlite::Transaction<'_>,
    member: &crate::federation::MemberCredentialContext,
    command: &FederationTaskCommand,
    command_json: &str,
    outcome: FederationTaskCommandOutcome,
    task_revision: Option<u64>,
    now: i64,
) -> Result<FederationTaskCommandReceipt, TaskStoreError> {
    let receipt = FederationTaskCommandReceipt {
        command_id: command.id,
        outcome,
        task_revision,
        processed_at: now,
    };
    let receipt_json =
        serde_json::to_string(&receipt).map_err(|_| TaskStoreError::InvalidFederationTask)?;
    transaction.execute(
        "INSERT INTO apiary_task_commands
            (command_id, apiary_id, task_id, member_node_id, member_hive_id,
             member_operator_id, command_json, outcome, receipt_json, processed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            command.id.to_string(),
            command.apiary_id.to_string(),
            command.task_id.to_string(),
            member.node.to_string(),
            member.hive.to_string(),
            member.operator.to_string(),
            command_json,
            outcome.to_string(),
            receipt_json,
            now,
        ],
    )?;
    Ok(receipt)
}

fn next_lifecycle_transition(current: TaskState, desired: TaskState) -> Option<TaskState> {
    if current == desired {
        return None;
    }
    if current.can_transition_to(desired) {
        return Some(desired);
    }
    match desired {
        TaskState::Active => matches!(
            current,
            TaskState::Ready | TaskState::Blocked | TaskState::Review
        )
        .then_some(TaskState::Active),
        TaskState::Blocked => (current == TaskState::Review).then_some(TaskState::Active),
        TaskState::Review | TaskState::Completed => {
            if matches!(current, TaskState::Ready | TaskState::Blocked) {
                Some(TaskState::Active)
            } else if current == TaskState::Active {
                Some(TaskState::Review)
            } else {
                None
            }
        }
        TaskState::Ready => (current == TaskState::Review)
            .then_some(TaskState::Active)
            .or_else(|| (current == TaskState::Active).then_some(TaskState::Blocked)),
        TaskState::Draft => None,
    }
}

fn insert_local_lifecycle_command(
    transaction: &rusqlite::Transaction<'_>,
    apiary_id: ApiaryId,
    task_id: ApiaryTaskId,
    expected_revision: u64,
    target_state: TaskState,
    now: i64,
) -> Result<FederationTaskOutboxEntry, TaskStoreError> {
    let queued_count = transaction.query_row(
        "SELECT COUNT(*) FROM local_apiary_task_commands WHERE state = 'queued'",
        [],
        |row| row.get::<_, usize>(0),
    )?;
    if queued_count >= MAX_LOCAL_TASK_OUTBOX {
        return Err(TaskStoreError::InvalidFederationTask);
    }
    let command = FederationTaskCommand {
        id: FederationTaskCommandId::new(),
        apiary_id,
        task_id,
        expected_revision,
        kind: FederationTaskCommandKind::Transition,
        target_state: Some(target_state),
        created_at: now,
    };
    let command_json =
        serde_json::to_string(&command).map_err(|_| TaskStoreError::InvalidFederationTask)?;
    transaction.execute(
        "INSERT INTO local_apiary_task_commands
            (command_id, apiary_id, task_id, expected_revision, kind,
             target_state, command_json, state, attempt_count, last_attempt_at,
             receipt_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'transition', ?5, ?6, 'queued', 0, NULL, NULL, ?7, ?7)",
        params![
            command.id.to_string(),
            command.apiary_id.to_string(),
            command.task_id.to_string(),
            command.expected_revision,
            target_state.to_string(),
            command_json,
            now,
        ],
    )?;
    Ok(FederationTaskOutboxEntry {
        command,
        state: FederationTaskOutboxState::Queued,
        attempt_count: 0,
        last_attempt_at: None,
        receipt: None,
    })
}

fn federation_task_outbox_entry_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<FederationTaskOutboxEntry> {
    let command_json = row.get::<_, String>(0)?;
    let command = serde_json::from_str(&command_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let state = FederationTaskOutboxState::from_str(&row.get::<_, String>(1)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let receipt = row
        .get::<_, Option<String>>(4)?
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(FederationTaskOutboxEntry {
        command,
        state,
        attempt_count: row.get(2)?,
        last_attempt_at: row.get(3)?,
        receipt,
    })
}

fn apiary_task_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiaryTaskEvent> {
    let snapshot = row.get::<_, String>(1)?;
    let task = serde_json::from_str(&snapshot).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ApiaryTaskEvent {
        sequence: row.get(0)?,
        task,
    })
}

fn apiary_task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiaryTask> {
    let source = match row.get::<_, String>(2)?.as_str() {
        "swarm" => ApiaryTaskSource::Swarm,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(ApiaryTask {
        id: ApiaryTaskId::from_str(&row.get::<_, String>(0)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        apiary_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        source,
        title: row.get(3)?,
        description: row.get(4)?,
        priority: TaskPriority::from_str(&row.get::<_, String>(5)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        state: TaskState::from_str(&row.get::<_, String>(6)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        home_node_id: row
            .get::<_, Option<String>>(7)?
            .map(|value| parse_domain_id(&value))
            .transpose()?,
        home_hive_id: row
            .get::<_, Option<String>>(8)?
            .map(|value| parse_domain_id(&value))
            .transpose()?,
        revision: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn local_apiary_task_execution(
    transaction: &rusqlite::Transaction<'_>,
    apiary_task_id: ApiaryTaskId,
) -> Result<Option<LocalApiaryTaskExecution>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT execution.apiary_task_id, execution.local_task_id,
                    execution.worker_id, task.state, execution.created_at
             FROM local_apiary_task_executions execution
             JOIN tasks task ON task.id = execution.local_task_id
             WHERE execution.apiary_task_id = ?1",
            [apiary_task_id.to_string()],
            local_apiary_task_execution_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn local_apiary_task_execution_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<LocalApiaryTaskExecution> {
    Ok(LocalApiaryTaskExecution {
        apiary_task_id: parse_domain_id(&row.get::<_, String>(0)?)?,
        local_task_id: parse_domain_id(&row.get::<_, String>(1)?)?,
        worker_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        state: TaskState::from_str(&row.get::<_, String>(3)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(4)?,
    })
}

pub(super) fn migrate_federation_tasks(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_tasks (
             id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             source TEXT NOT NULL CHECK (source = 'swarm'),
             title TEXT NOT NULL,
             description TEXT NOT NULL DEFAULT '',
             priority TEXT NOT NULL CHECK (priority IN ('low','normal','high','urgent')),
             state TEXT NOT NULL CHECK (state IN ('draft','ready','active','blocked','review','completed')),
             home_node_id TEXT,
             home_hive_id TEXT REFERENCES hives(id),
             revision INTEGER NOT NULL CHECK (revision > 0),
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
             CHECK ((home_node_id IS NULL) = (home_hive_id IS NULL))
         );
         CREATE INDEX IF NOT EXISTS apiary_tasks_by_apiary_state
             ON apiary_tasks(apiary_id, state, updated_at DESC);
         CREATE TABLE IF NOT EXISTS apiary_task_events (
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             sequence INTEGER NOT NULL CHECK (sequence > 0),
             task_id TEXT NOT NULL REFERENCES apiary_tasks(id),
             task_revision INTEGER NOT NULL CHECK (task_revision > 0),
             snapshot_json TEXT NOT NULL,
             occurred_at INTEGER NOT NULL CHECK (occurred_at >= 0),
             PRIMARY KEY (apiary_id, sequence),
             UNIQUE (task_id, task_revision)
         );
         CREATE INDEX IF NOT EXISTS apiary_task_events_feed
             ON apiary_task_events(apiary_id, sequence);
         CREATE TABLE IF NOT EXISTS local_apiary_tasks (
             task_id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             revision INTEGER NOT NULL CHECK (revision > 0),
             snapshot_json TEXT NOT NULL,
             last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence > 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
         );
         CREATE TABLE IF NOT EXISTS local_apiary_task_sync (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             cursor INTEGER NOT NULL CHECK (cursor >= 0),
             last_applied_at INTEGER CHECK (last_applied_at >= 0)
         );
         PRAGMA user_version = 47;",
    )
}

pub(super) fn migrate_federation_task_commands(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS apiary_task_commands (
             command_id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             task_id TEXT NOT NULL REFERENCES apiary_tasks(id),
             member_node_id TEXT NOT NULL,
             member_hive_id TEXT NOT NULL REFERENCES hives(id),
             member_operator_id TEXT NOT NULL,
             command_json TEXT NOT NULL,
             outcome TEXT NOT NULL CHECK (outcome IN ('applied','conflict','rejected')),
             receipt_json TEXT NOT NULL,
             processed_at INTEGER NOT NULL CHECK (processed_at >= 0)
         );
         CREATE INDEX IF NOT EXISTS apiary_task_commands_by_apiary
             ON apiary_task_commands(apiary_id, processed_at DESC);
         CREATE TABLE IF NOT EXISTS local_apiary_task_commands (
             command_id TEXT PRIMARY KEY,
             apiary_id TEXT NOT NULL REFERENCES apiaries(id),
             task_id TEXT NOT NULL,
             expected_revision INTEGER NOT NULL CHECK (expected_revision > 0),
             kind TEXT NOT NULL CHECK (kind IN ('claim','transition')),
             target_state TEXT CHECK (target_state IN ('draft','ready','active','blocked','review','completed')),
             command_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK (state IN ('queued','applied','conflict','rejected')),
             attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
             last_attempt_at INTEGER CHECK (last_attempt_at >= 0),
             receipt_json TEXT,
             created_at INTEGER NOT NULL CHECK (created_at >= 0),
             updated_at INTEGER NOT NULL CHECK (updated_at >= created_at),
             CHECK ((kind = 'claim' AND target_state IS NULL) OR
                    (kind = 'transition' AND target_state IS NOT NULL))
         );
         CREATE INDEX IF NOT EXISTS local_apiary_task_commands_queue
             ON local_apiary_task_commands(state, created_at, command_id);
         PRAGMA user_version = 48;",
    )
}

pub(super) fn migrate_local_apiary_task_executions(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_apiary_task_executions (
             apiary_task_id TEXT PRIMARY KEY,
             local_task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             created_at INTEGER NOT NULL CHECK (created_at >= 0)
         );
         CREATE INDEX IF NOT EXISTS local_apiary_task_executions_by_worker
             ON local_apiary_task_executions(worker_id, created_at DESC);
         PRAGMA user_version = 56;",
    )
}

pub(super) fn migrate_local_apiary_task_lifecycle_intents(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS local_apiary_task_lifecycle_intents (
             apiary_task_id TEXT PRIMARY KEY
                 REFERENCES local_apiary_task_executions(apiary_task_id) ON DELETE CASCADE,
             desired_state TEXT NOT NULL
                 CHECK (desired_state IN ('ready','active','blocked','review','completed')),
             updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
         );
         PRAGMA user_version = 57;",
    )
}

pub(super) fn record_local_apiary_task_lifecycle_intent(
    transaction: &rusqlite::Transaction<'_>,
    local_task_id: TaskId,
    desired_state: TaskState,
) -> rusqlite::Result<()> {
    if desired_state == TaskState::Draft {
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO local_apiary_task_lifecycle_intents
             (apiary_task_id, desired_state, updated_at)
         SELECT apiary_task_id, ?2, unixepoch()
         FROM local_apiary_task_executions WHERE local_task_id = ?1
         ON CONFLICT(apiary_task_id) DO UPDATE SET
             desired_state = excluded.desired_state,
             updated_at = excluded.updated_at",
        params![local_task_id.to_string(), desired_state.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        FederationJoinAcceptance, FederationJoinReadiness, JiraConnectionState, ProviderKind,
        SharedWorkBackend,
    };

    fn joined_member(now: i64) -> (TaskStore, TaskStore, FederationJoinAcceptance) {
        let keeper = TaskStore::in_memory().expect("keeper");
        keeper
            .create_apiary_for_local_hive("Garden", SharedWorkBackend::Jira, now)
            .expect("apiary");
        let member = TaskStore::in_memory().expect("member");
        let identity = member.local_hive_identity().expect("identity");
        let card = member
            .issue_hive_connection_card(now + 1, 3_600)
            .expect("card");
        keeper.pin_hive_candidate(&card, now + 1).expect("pin");
        let bundle = keeper
            .issue_apiary_invitation_bundle(
                identity.hive.id,
                "https://keeper.example.test/swarm",
                now + 1,
                3_600,
            )
            .expect("invitation");
        let invitation = member
            .import_apiary_invitation_bundle(&bundle, now + 2)
            .expect("import");
        member
            .accept_federation_join_policy(invitation.invitation_id, 1, now + 3)
            .expect("policy");
        let submission = member
            .prepare_federation_join_submission(
                invitation.invitation_id,
                &FederationJoinReadiness {
                    jira_connection: JiraConnectionState::Ready,
                    projects: Vec::new(),
                    blockers: Vec::new(),
                },
                now + 4,
            )
            .expect("submission");
        let acceptance = keeper
            .consume_federation_join_submission(&submission, now + 5)
            .expect("acceptance");
        member
            .apply_federation_join_acceptance(
                acceptance.receipt.payload.invitation_id,
                &acceptance,
                now + 6,
            )
            .expect("join");
        (keeper, member, acceptance)
    }

    #[test]
    fn migration_creates_bounded_task_feed_tables() {
        let store = TaskStore::in_memory().expect("store");
        let connection = store.connection().expect("connection");
        let version = connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("version");
        assert_eq!(version, super::super::CURRENT_SCHEMA_VERSION);
        for table in [
            "apiary_tasks",
            "apiary_task_events",
            "local_apiary_tasks",
            "local_apiary_task_sync",
            "apiary_task_commands",
            "local_apiary_task_commands",
            "local_apiary_task_executions",
            "local_apiary_task_lifecycle_intents",
        ] {
            let exists = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get::<_, bool>(0),
                )
                .expect("table lookup");
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn member_claim_is_durable_idempotent_and_projects_keeper_event() {
        let now = 100_000;
        let (keeper, member, acceptance) = joined_member(now);
        let task = keeper
            .create_apiary_task("Coordinate release", "", TaskPriority::Normal, now + 10)
            .expect("task");
        let first_page = keeper
            .federation_task_page(&acceptance.node_credential, 0, now + 11)
            .expect("page");
        member
            .apply_federation_task_page(&first_page, now + 11)
            .expect("projection");

        let queued = member
            .queue_federation_task_claim(task.id, now + 12)
            .expect("queue");
        assert_eq!(queued.state, FederationTaskOutboxState::Queued);
        assert_eq!(
            member.federation_task_outbox_status().unwrap().queued_count,
            1
        );
        member
            .record_federation_task_command_attempt(queued.command.id, now + 13)
            .expect("attempt");
        let receipt = keeper
            .apply_federation_task_command(&acceptance.node_credential, &queued.command, now + 14)
            .expect("apply");
        assert_eq!(receipt.outcome, FederationTaskCommandOutcome::Applied);
        assert_eq!(receipt.task_revision, Some(2));
        assert_eq!(
            keeper
                .apply_federation_task_command(
                    &acceptance.node_credential,
                    &queued.command,
                    now + 15,
                )
                .expect("exact retry"),
            receipt
        );
        member
            .apply_federation_task_command_receipt(&receipt, now + 15)
            .expect("receipt");
        let next_page = keeper
            .federation_task_page(
                &acceptance.node_credential,
                first_page.next_cursor,
                now + 16,
            )
            .expect("next page");
        member
            .apply_federation_task_page(&next_page, now + 16)
            .expect("apply event");
        let projected = member.list_local_apiary_tasks().expect("tasks");
        assert_eq!(
            projected[0].home_hive_id,
            Some(acceptance.receipt.payload.member_hive_id)
        );
        assert_eq!(projected[0].revision, 2);
        assert_eq!(
            member.federation_task_outbox_status().unwrap().queued_count,
            0
        );
    }

    #[test]
    fn keeper_routes_shared_work_to_one_active_member_hive_without_private_worker_data() {
        let now = 150_000;
        let (keeper, member, acceptance) = joined_member(now);
        let target_hive = acceptance.receipt.payload.member_hive_id;
        let target_node = acceptance.receipt.payload.member_node_id;
        let task = keeper
            .create_apiary_task_for_hive(
                "Prepare the Member release",
                "The receiving Queen decides which private worker should handle it.",
                TaskPriority::High,
                Some(target_hive),
                now + 10,
            )
            .expect("routed task");
        assert_eq!(task.home_hive_id, Some(target_hive));
        assert_eq!(task.home_node_id, Some(target_node));

        let page = keeper
            .federation_task_page(&acceptance.node_credential, 0, now + 11)
            .expect("page");
        member
            .apply_federation_task_page(&page, now + 11)
            .expect("projection");
        let projected = member.list_local_apiary_tasks().expect("tasks");
        assert_eq!(projected[0].home_hive_id, Some(target_hive));
        assert_eq!(projected[0].title, "Prepare the Member release");
        assert!(
            member
                .queue_federation_task_transition(projected[0].id, TaskState::Active, now + 12)
                .is_ok()
        );

        assert!(matches!(
            keeper.create_apiary_task_for_hive(
                "Unknown destination",
                "",
                TaskPriority::Normal,
                Some(HiveId::new()),
                now + 13,
            ),
            Err(TaskStoreError::InvalidFederationTask)
        ));
    }

    #[test]
    fn member_materializes_owned_keeper_work_once_for_one_private_worker() {
        let now = 175_000;
        let (keeper, member, acceptance) = joined_member(now);
        let worker = member
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/projects/clover",
                false,
                1,
            )
            .expect("worker");
        let shared = keeper
            .create_apiary_task_for_hive(
                "Prepare the shared release",
                "Verify the Member-owned outcome.",
                TaskPriority::High,
                Some(acceptance.receipt.payload.member_hive_id),
                now + 10,
            )
            .expect("routed task");
        let page = keeper
            .federation_task_page(&acceptance.node_credential, 0, now + 11)
            .expect("page");
        member
            .apply_federation_task_page(&page, now + 11)
            .expect("projection");

        let first = member
            .materialize_local_apiary_task_execution(shared.id, worker.id, now + 12)
            .expect("execution");
        let retry = member
            .materialize_local_apiary_task_execution(shared.id, WorkerId::new(), now + 13)
            .expect("idempotent retry");
        assert_eq!(retry, first);
        assert_eq!(first.worker_id, worker.id);
        assert_eq!(first.state, TaskState::Ready);

        let tasks = member.list_tasks().expect("local tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, first.local_task_id);
        assert_eq!(tasks[0].title, "Prepare the shared release");
        assert_eq!(tasks[0].workspace, "/projects/clover");
        assert_eq!(tasks[0].assigned_worker_id, Some(worker.id));
        assert_eq!(
            member.list_local_apiary_task_executions().expect("links"),
            vec![first]
        );
        assert!(matches!(
            member.queue_federation_task_transition(shared.id, TaskState::Active, now + 14),
            Err(TaskStoreError::InvalidFederationTask)
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)] // The end-to-end phase proof keeps every persisted receipt and projection boundary explicit.
    fn local_worker_progress_serializes_one_keeper_transition_at_a_time() {
        let now = 180_000;
        let (keeper, member, acceptance) = joined_member(now);
        let worker = member
            .create_worker(
                "Clover",
                ProviderKind::ClaudeCode,
                "/projects/clover",
                false,
                1,
            )
            .expect("worker");
        let shared = keeper
            .create_apiary_task_for_hive(
                "Prepare the shared release",
                "Worker progress must survive an offline Keeper.",
                TaskPriority::Normal,
                Some(acceptance.receipt.payload.member_hive_id),
                now + 10,
            )
            .expect("routed task");
        let first_page = keeper
            .federation_task_page(&acceptance.node_credential, 0, now + 11)
            .expect("page");
        member
            .apply_federation_task_page(&first_page, now + 11)
            .expect("projection");
        let execution = member
            .materialize_local_apiary_task_execution(shared.id, worker.id, now + 12)
            .expect("execution");

        member
            .transition_task(execution.local_task_id, TaskState::Active)
            .expect("active locally");
        member
            .transition_task(execution.local_task_id, TaskState::Review)
            .expect("review locally");
        assert_eq!(
            member
                .prepare_local_apiary_task_lifecycle_commands(now + 13)
                .expect("stage active"),
            1
        );
        assert_eq!(
            member
                .prepare_local_apiary_task_lifecycle_commands(now + 14)
                .expect("no duplicate"),
            0
        );
        let active = member
            .pending_federation_task_commands(20)
            .expect("active command");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].command.target_state, Some(TaskState::Active));
        let active_receipt = keeper
            .apply_federation_task_command(
                &acceptance.node_credential,
                &active[0].command,
                now + 15,
            )
            .expect("apply active");
        member
            .apply_federation_task_command_receipt(&active_receipt, now + 15)
            .expect("active receipt");
        assert_eq!(
            member
                .prepare_local_apiary_task_lifecycle_commands(now + 16)
                .expect("wait for event"),
            0
        );

        let active_page = keeper
            .federation_task_page(
                &acceptance.node_credential,
                first_page.next_cursor,
                now + 17,
            )
            .expect("active page");
        member
            .apply_federation_task_page(&active_page, now + 17)
            .expect("active projection");
        assert_eq!(
            member
                .prepare_local_apiary_task_lifecycle_commands(now + 18)
                .expect("stage review"),
            1
        );
        let review = member
            .pending_federation_task_commands(20)
            .expect("review command");
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].command.target_state, Some(TaskState::Review));
        assert_eq!(review[0].command.expected_revision, 2);
        let review_receipt = keeper
            .apply_federation_task_command(
                &acceptance.node_credential,
                &review[0].command,
                now + 19,
            )
            .expect("apply review");
        member
            .apply_federation_task_command_receipt(&review_receipt, now + 19)
            .expect("review receipt");
        let review_page = keeper
            .federation_task_page(
                &acceptance.node_credential,
                active_page.next_cursor,
                now + 20,
            )
            .expect("review page");
        member
            .apply_federation_task_page(&review_page, now + 20)
            .expect("review projection");
        assert_eq!(
            member
                .prepare_local_apiary_task_lifecycle_commands(now + 21)
                .expect("converged"),
            0
        );
        assert_eq!(
            member.list_local_apiary_tasks().unwrap()[0].state,
            TaskState::Review
        );
        assert_eq!(
            member.list_local_apiary_task_executions().unwrap()[0].state,
            TaskState::Review
        );
    }

    #[test]
    fn competing_stale_claim_receives_durable_conflict() {
        let now = 200_000;
        let (keeper, member, acceptance) = joined_member(now);
        let task = keeper
            .create_apiary_task("One owner", "", TaskPriority::High, now + 10)
            .expect("task");
        let page = keeper
            .federation_task_page(&acceptance.node_credential, 0, now + 11)
            .expect("page");
        member
            .apply_federation_task_page(&page, now + 11)
            .expect("projection");
        let queued = member
            .queue_federation_task_claim(task.id, now + 12)
            .expect("queue");
        let first = keeper
            .apply_federation_task_command(&acceptance.node_credential, &queued.command, now + 13)
            .expect("first");
        assert_eq!(first.outcome, FederationTaskCommandOutcome::Applied);
        let mut stale = queued.command.clone();
        stale.id = FederationTaskCommandId::new();
        let conflict = keeper
            .apply_federation_task_command(&acceptance.node_credential, &stale, now + 14)
            .expect("conflict receipt");
        assert_eq!(conflict.outcome, FederationTaskCommandOutcome::Conflict);
        member
            .apply_federation_task_command_receipt(&conflict, now + 15)
            .expect_err("foreign command cannot enter local outbox");
    }
}
