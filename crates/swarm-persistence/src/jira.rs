use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ApiaryId, ControlRoomEventKind, JiraIssueLink, JiraProjectBinding, JiraProjectBindingId,
    JiraProjectScope, JiraStatusMapping, Task, TaskId, TaskState,
};

use super::{TaskStore, TaskStoreError, insert_control_room_event, parse_domain_id};

const MAX_PROJECT_ID_BYTES: usize = 128;
const MAX_PROJECT_KEY_BYTES: usize = 64;
const MAX_PROJECT_NAME_BYTES: usize = 240;
const MAX_JIRA_STATUSES: usize = 128;
const MAX_STATUS_ID_BYTES: usize = 128;
const MAX_STATUS_NAME_BYTES: usize = 240;
const MAX_ISSUES_PER_SYNC: usize = 100;
const MAX_ISSUE_ID_BYTES: usize = 128;
const MAX_ISSUE_KEY_BYTES: usize = 128;
const MAX_REMOTE_TIMESTAMP_BYTES: usize = 128;
const MAX_TRANSITION_CLAIMS: i64 = 16;
const MAX_TRANSITION_ATTEMPTS: i64 = 3;
const MAX_PENDING_TRANSITIONS: i64 = 256;
const MAX_PENDING_COMMENTS: i64 = 256;
const MAX_COMMENT_BYTES: usize = 4_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JiraCommentDispatch {
    pub id: String,
    pub task_id: TaskId,
    pub issue_key: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JiraTransitionDispatch {
    pub id: String,
    pub task_id: TaskId,
    pub issue_key: String,
    pub target_task_state: TaskState,
    pub target_status_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JiraTransitionFailure {
    Retryable,
    Conflict,
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JiraProjectBindingInput<'a> {
    pub project_id: &'a str,
    pub project_key: &'a str,
    pub project_name: &'a str,
    pub scope: JiraProjectScope,
    pub apiary_id: Option<ApiaryId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JiraIssueSnapshot<'a> {
    pub issue_id: &'a str,
    pub issue_key: &'a str,
    pub summary: &'a str,
    pub description: &'a str,
    pub status_id: &'a str,
    pub status_name: &'a str,
    pub assignee_account_id: Option<&'a str>,
    pub assignee_name: Option<&'a str>,
    pub remote_updated_at: &'a str,
}

impl TaskStore {
    /// Creates or refreshes one project binding using Jira's immutable project id.
    /// Existing workflow mappings survive project key or name changes.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is invalid or persistence fails.
    pub fn upsert_jira_project_binding(
        &self,
        input: &JiraProjectBindingInput<'_>,
    ) -> Result<JiraProjectBinding, TaskStoreError> {
        let project_id = input.project_id.trim();
        let project_key = input.project_key.trim();
        let project_name = input.project_name.trim();
        validate_project(
            project_id,
            project_key,
            project_name,
            input.scope,
            input.apiary_id,
        )?;
        let identity = self.local_hive_identity()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing_id = transaction
            .query_row(
                "SELECT id FROM jira_project_bindings WHERE hive_id = ?1 AND project_id = ?2",
                params![identity.hive.id.to_string(), project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let id = existing_id
            .as_deref()
            .map(parse_domain_id::<JiraProjectBindingId>)
            .transpose()?
            .unwrap_or_default();
        transaction.execute(
            "INSERT INTO jira_project_bindings (
                 id, hive_id, project_id, project_key, project_name, scope, apiary_id,
                 access_verified
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
             ON CONFLICT(hive_id, project_id) DO UPDATE SET
                 project_key = excluded.project_key,
                 project_name = excluded.project_name,
                 scope = excluded.scope,
                 apiary_id = excluded.apiary_id,
                 default_worker_id = NULL,
                 access_verified = 1,
                 updated_at = unixepoch()",
            params![
                id.to_string(),
                identity.hive.id.to_string(),
                project_id,
                project_key,
                project_name,
                input.scope.to_string(),
                input.apiary_id.map(|value| value.to_string()),
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_jira_project_binding(id)
    }

    /// Lists the local Hive's Jira project bindings.
    ///
    /// # Errors
    ///
    /// Returns an error when Hive identity or persistence cannot be read.
    pub fn list_jira_project_bindings(&self) -> Result<Vec<JiraProjectBinding>, TaskStoreError> {
        let hive_id = self.local_hive_identity()?.hive.id;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, project_key, project_name, scope, hive_id, apiary_id,
                    access_verified, workflow_mapped, auto_sync_assigned
             FROM jira_project_bindings WHERE hive_id = ?1
             ORDER BY project_name COLLATE NOCASE, project_key",
        )?;
        statement
            .query_map([hive_id.to_string()], jira_binding_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Gets one Jira project binding by durable local identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding does not exist or persistence fails.
    pub fn get_jira_project_binding(
        &self,
        id: JiraProjectBindingId,
    ) -> Result<JiraProjectBinding, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, project_id, project_key, project_name, scope, hive_id, apiary_id,
                        access_verified, workflow_mapped, auto_sync_assigned
                 FROM jira_project_bindings WHERE id = ?1",
                [id.to_string()],
                jira_binding_from_row,
            )
            .optional()?
            .ok_or(TaskStoreError::JiraProjectBindingNotFound)
    }

    /// Controls whether open Jira issues assigned to the connected operator are
    /// automatically synchronized into this Hive.
    ///
    /// # Errors
    ///
    /// Returns an error when the project binding does not exist or the database
    /// cannot persist or reload the setting.
    pub fn set_jira_auto_sync_assigned(
        &self,
        id: JiraProjectBindingId,
        enabled: bool,
    ) -> Result<JiraProjectBinding, TaskStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jira_project_bindings
             SET auto_sync_assigned = ?2, updated_at = unixepoch()
             WHERE id = ?1",
            params![id.to_string(), enabled],
        )?;
        drop(connection);
        if changed != 1 {
            return Err(TaskStoreError::JiraProjectBindingNotFound);
        }
        self.get_jira_project_binding(id)
    }

    /// Atomically replaces the complete workflow mapping for a project.
    ///
    /// # Errors
    ///
    /// Returns an error when the mapping or binding is invalid or persistence fails.
    pub fn replace_jira_status_mappings(
        &self,
        binding_id: JiraProjectBindingId,
        mappings: &[JiraStatusMapping],
    ) -> Result<Vec<JiraStatusMapping>, TaskStoreError> {
        validate_mappings(mappings)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM jira_project_bindings WHERE id = ?1)",
            [binding_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(TaskStoreError::JiraProjectBindingNotFound);
        }
        transaction.execute(
            "DELETE FROM jira_status_mappings WHERE binding_id = ?1",
            [binding_id.to_string()],
        )?;
        for mapping in mappings {
            transaction.execute(
                "INSERT INTO jira_status_mappings (
                     binding_id, jira_status_id, jira_status_name, task_state
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    binding_id.to_string(),
                    mapping.jira_status_id.trim(),
                    mapping.jira_status_name.trim(),
                    mapping.task_state.to_string(),
                ],
            )?;
        }
        transaction.execute(
            "UPDATE jira_project_bindings
             SET workflow_mapped = 1, updated_at = unixepoch() WHERE id = ?1",
            [binding_id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.list_jira_status_mappings(binding_id)
    }

    /// Lists the explicit status mapping for one Jira project binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding does not exist or persistence fails.
    pub fn list_jira_status_mappings(
        &self,
        binding_id: JiraProjectBindingId,
    ) -> Result<Vec<JiraStatusMapping>, TaskStoreError> {
        self.get_jira_project_binding(binding_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT jira_status_id, jira_status_name, task_state
             FROM jira_status_mappings WHERE binding_id = ?1
             ORDER BY jira_status_name COLLATE NOCASE, jira_status_id",
        )?;
        statement
            .query_map([binding_id.to_string()], |row| {
                let state = TaskState::from_str(&row.get::<_, String>(2)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(JiraStatusMapping {
                    jira_status_id: row.get(0)?,
                    jira_status_name: row.get(1)?,
                    task_state: state,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Applies one bounded Jira snapshot atomically. Remote issue identity and mapped
    /// workflow state are canonical; Swarm-owned assignment, notes, and evidence survive.
    ///
    /// # Errors
    ///
    /// Returns an error when the snapshot, workflow mapping, or worker is invalid, or
    /// when the atomic persistence operation fails.
    #[allow(clippy::too_many_lines)]
    pub fn sync_jira_issues(
        &self,
        binding_id: JiraProjectBindingId,
        issues: &[JiraIssueSnapshot<'_>],
    ) -> Result<Vec<Task>, TaskStoreError> {
        if issues.len() > MAX_ISSUES_PER_SYNC || issues.iter().any(|issue| !valid_issue(issue)) {
            return Err(TaskStoreError::InvalidJiraProject);
        }
        if issues.is_empty() {
            return Ok(Vec::new());
        }
        let binding = self.get_jira_project_binding(binding_id)?;
        if !binding.access_verified || !binding.workflow_mapped {
            return Err(TaskStoreError::InvalidJiraWorkflowMapping);
        }
        let states = self
            .list_jira_status_mappings(binding_id)?
            .into_iter()
            .map(|mapping| (mapping.jira_status_id, mapping.task_state))
            .collect::<HashMap<_, _>>();
        let mut issue_ids = HashSet::new();
        if !issues.iter().all(|issue| {
            issue_ids.insert(issue.issue_id.trim().to_owned())
                && states.contains_key(issue.status_id.trim())
        }) {
            return Err(TaskStoreError::InvalidJiraWorkflowMapping);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut task_ids = Vec::with_capacity(issues.len());
        let mut tasks_changed = false;
        // The task schema still requires a non-empty execution location. Jira
        // intake has no repository yet, so keep an internal non-filesystem scope
        // until assignment replaces it with the selected worker's workspace.
        let unassigned_scope = format!("jira://project/{}", binding.project_id);
        for issue in issues {
            let issue_id = issue.issue_id.trim();
            let issue_key = issue.issue_key.trim();
            let summary = issue.summary.trim();
            let status_id = issue.status_id.trim();
            let status_name = issue.status_name.trim();
            let target_state = states[status_id];
            let existing = transaction
                .query_row(
                    "SELECT link.task_id, task.state, delivery.id, delivery.target_task_state
                     FROM jira_issue_links link
                     JOIN tasks task ON task.id = link.task_id
                     LEFT JOIN jira_transition_deliveries delivery
                       ON delivery.task_id = link.task_id AND delivery.state <> 'delivered'
                     WHERE link.issue_id = ?1",
                    [issue_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .optional()?;
            let task_id =
                if let Some((task_id, previous_state, delivery_id, delivery_target)) = existing {
                    let task_id = parse_domain_id::<TaskId>(&task_id)?;
                    let previous_state = TaskState::from_str(&previous_state)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?;
                    let delivery_target = delivery_target
                        .map(|state| {
                            TaskState::from_str(&state).map_err(|_| rusqlite::Error::InvalidQuery)
                        })
                        .transpose()?;
                    let remote_acknowledged = delivery_target == Some(target_state);
                    if remote_acknowledged && let Some(delivery_id) = delivery_id {
                        transaction.execute(
                            "UPDATE jira_transition_deliveries
                         SET state = 'delivered', delivered_at = unixepoch(),
                             last_error = NULL, updated_at = unixepoch()
                         WHERE id = ?1 AND state <> 'delivered'",
                            [delivery_id],
                        )?;
                    }
                    let synchronized_state = if delivery_target.is_some() && !remote_acknowledged {
                        previous_state
                    } else {
                        target_state
                    };
                    let changed = transaction.execute(
                        "UPDATE tasks SET title = ?2, state = ?3, updated_at = unixepoch()
                     WHERE id = ?1 AND (title <> ?2 OR state <> ?3)",
                        params![task_id.to_string(), summary, synchronized_state.to_string()],
                    )?;
                    tasks_changed |= changed > 0;
                    if previous_state != synchronized_state {
                        transaction.execute(
                            "INSERT INTO task_activity (
                                 task_id, kind, from_state, to_state, note, actor_kind
                             ) VALUES (?1, 'state_changed', ?2, ?3, ?4, 'jira')",
                            params![
                                task_id.to_string(),
                                previous_state.to_string(),
                                synchronized_state.to_string(),
                                format!("Synchronized from Jira {issue_key}"),
                            ],
                        )?;
                    }
                    task_id
                } else {
                    let task_id = TaskId::new();
                    transaction.execute(
                        "INSERT INTO tasks (
                         id, hive_id, title, description, priority, workspace, state, position
                     ) VALUES (?1, ?2, ?3, ?4, 'normal', ?5, ?6,
                         COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
                        params![
                            task_id.to_string(),
                            binding.hive_id.to_string(),
                            summary,
                            issue.description.trim(),
                            unassigned_scope,
                            target_state.to_string(),
                        ],
                    )?;
                    transaction.execute(
                        "INSERT INTO task_activity (task_id, kind, to_state, note, actor_kind)
                     VALUES (?1, 'created', ?2, ?3, 'jira')",
                        params![
                            task_id.to_string(),
                            target_state.to_string(),
                            format!("Imported from Jira {issue_key}"),
                        ],
                    )?;
                    tasks_changed = true;
                    task_id
                };
            transaction.execute(
                "INSERT INTO jira_issue_links (
                     issue_id, issue_key, binding_id, task_id, jira_status_id, jira_status_name,
                     jira_assignee_account_id, jira_assignee_name, remote_updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(issue_id) DO UPDATE SET
                     issue_key = excluded.issue_key,
                     binding_id = excluded.binding_id,
                     jira_status_id = excluded.jira_status_id,
                     jira_status_name = excluded.jira_status_name,
                     jira_assignee_account_id = excluded.jira_assignee_account_id,
                     jira_assignee_name = excluded.jira_assignee_name,
                     remote_updated_at = excluded.remote_updated_at,
                     last_synced_at = unixepoch()",
                params![
                    issue_id,
                    issue_key,
                    binding_id.to_string(),
                    task_id.to_string(),
                    status_id,
                    status_name,
                    issue.assignee_account_id.map(str::trim),
                    issue.assignee_name.map(str::trim),
                    issue.remote_updated_at.trim(),
                ],
            )?;
            task_ids.push(task_id);
        }
        if tasks_changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        drop(connection);
        task_ids.into_iter().map(|id| self.get_task(id)).collect()
    }

    /// Lists durable Jira issue identities already linked to local tasks.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding does not exist or persistence fails.
    pub fn list_jira_issue_links(
        &self,
        binding_id: JiraProjectBindingId,
    ) -> Result<Vec<JiraIssueLink>, TaskStoreError> {
        self.get_jira_project_binding(binding_id)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT issue_id, issue_key, binding_id, task_id, jira_status_id, jira_status_name,
                    jira_assignee_account_id, jira_assignee_name, remote_updated_at, last_synced_at
             FROM jira_issue_links WHERE binding_id = ?1 ORDER BY issue_key",
        )?;
        statement
            .query_map([binding_id.to_string()], jira_issue_link_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Finds the durable Jira identity attached to one local task.
    ///
    /// # Errors
    ///
    /// Returns an error when persistence cannot be read.
    pub fn jira_issue_link_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<JiraIssueLink>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT issue_id, issue_key, binding_id, task_id, jira_status_id, jira_status_name,
                        jira_assignee_account_id, jira_assignee_name, remote_updated_at, last_synced_at
                 FROM jira_issue_links WHERE task_id = ?1",
                [task_id.to_string()],
                jira_issue_link_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Records a Jira transition that completed successfully before the local task moved.
    ///
    /// # Errors
    ///
    /// Returns an error when the task is not linked to Jira or status metadata is invalid.
    pub fn update_jira_issue_link_status(
        &self,
        task_id: TaskId,
        status_id: &str,
        status_name: &str,
    ) -> Result<(), TaskStoreError> {
        let status_id = status_id.trim();
        let status_name = status_name.trim();
        if status_id.is_empty()
            || status_id.len() > MAX_STATUS_ID_BYTES
            || status_name.is_empty()
            || status_name.len() > MAX_STATUS_NAME_BYTES
        {
            return Err(TaskStoreError::InvalidJiraWorkflowMapping);
        }
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jira_issue_links
             SET jira_status_id = ?2, jira_status_name = ?3, last_synced_at = unixepoch()
             WHERE task_id = ?1",
            params![task_id.to_string(), status_id, status_name],
        )?;
        if changed == 0 {
            return Err(TaskStoreError::NotFound);
        }
        Ok(())
    }

    /// Atomically claims a bounded batch of durable Jira workflow updates.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn claim_jira_transitions(
        &self,
        now: i64,
    ) -> Result<Vec<JiraTransitionDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT delivery.id, delivery.task_id, link.issue_key,
                        delivery.target_task_state
                 FROM jira_transition_deliveries delivery
                 JOIN jira_issue_links link ON link.task_id = delivery.task_id
                 WHERE delivery.state = 'queued' AND delivery.available_at <= ?1
                   AND delivery.attempts < ?2
                 ORDER BY delivery.updated_at, delivery.id LIMIT ?3",
            )?;
            statement
                .query_map(
                    params![now, MAX_TRANSITION_ATTEMPTS, MAX_TRANSITION_CLAIMS],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            parse_domain_id::<TaskId>(&row.get::<_, String>(1)?)?,
                            row.get::<_, String>(2)?,
                            TaskState::from_str(&row.get::<_, String>(3)?)
                                .map_err(|_| rusqlite::Error::InvalidQuery)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut deliveries = Vec::with_capacity(candidates.len());
        for (id, task_id, issue_key, target_task_state) in candidates {
            let target_status_ids = {
                let mut statement = transaction.prepare(
                    "SELECT mapping.jira_status_id
                     FROM jira_issue_links link
                     JOIN jira_status_mappings mapping ON mapping.binding_id = link.binding_id
                     WHERE link.task_id = ?1 AND mapping.task_state = ?2
                     ORDER BY mapping.jira_status_name COLLATE NOCASE, mapping.jira_status_id",
                )?;
                statement
                    .query_map(
                        params![task_id.to_string(), target_task_state.to_string()],
                        |row| row.get(0),
                    )?
                    .collect::<Result<Vec<String>, _>>()?
            };
            if target_status_ids.is_empty() {
                return Err(TaskStoreError::IntegrityFailure(
                    "queued Jira transition lost its workflow mapping".into(),
                ));
            }
            let changed = transaction.execute(
                "UPDATE jira_transition_deliveries
                 SET state = 'dispatching', attempts = attempts + 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND state = 'queued' AND attempts < ?3",
                params![id, now, MAX_TRANSITION_ATTEMPTS],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "Jira transition claim lost atomic ownership".into(),
                ));
            }
            deliveries.push(JiraTransitionDispatch {
                id,
                task_id,
                issue_key,
                target_task_state,
                target_status_ids,
            });
        }
        if !deliveries.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(deliveries)
    }

    /// Records a Jira workflow update that Jira acknowledged.
    ///
    /// # Errors
    /// Returns an error for invalid status metadata, a lost claim, or persistence failure.
    pub fn complete_jira_transition(
        &self,
        id: &str,
        status_id: &str,
        status_name: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let status_id = status_id.trim();
        let status_name = status_name.trim();
        if status_id.is_empty()
            || status_id.len() > MAX_STATUS_ID_BYTES
            || status_name.is_empty()
            || status_name.len() > MAX_STATUS_NAME_BYTES
        {
            return Err(TaskStoreError::InvalidJiraWorkflowMapping);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task_id = transaction
            .query_row(
                "SELECT task_id FROM jira_transition_deliveries
                 WHERE id = ?1 AND state = 'dispatching'",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(task_id) = task_id else {
            return Ok(false);
        };
        transaction.execute(
            "UPDATE jira_issue_links SET jira_status_id = ?2, jira_status_name = ?3,
                 last_synced_at = ?4 WHERE task_id = ?1",
            params![task_id, status_id, status_name, now],
        )?;
        let changed = transaction.execute(
            "UPDATE jira_transition_deliveries
             SET state = 'delivered', delivered_at = ?2, last_error = NULL, updated_at = ?2
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Records a retryable, conflicting, or ambiguous Jira delivery outcome.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn fail_jira_transition(
        &self,
        id: &str,
        now: i64,
        failure: JiraTransitionFailure,
        error_code: &str,
    ) -> Result<bool, TaskStoreError> {
        let error_code = error_code.trim();
        if error_code.is_empty()
            || error_code.len() > 128
            || error_code.chars().any(char::is_control)
        {
            return Err(TaskStoreError::InvalidJiraWorkflowMapping);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let attempts = transaction
            .query_row(
                "SELECT attempts FROM jira_transition_deliveries
                 WHERE id = ?1 AND state = 'dispatching'",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(attempts) = attempts else {
            return Ok(false);
        };
        let (state, available_at) = match failure {
            JiraTransitionFailure::Retryable if attempts < MAX_TRANSITION_ATTEMPTS => (
                "queued",
                now.saturating_add(30_i64.saturating_mul(attempts.max(1))),
            ),
            JiraTransitionFailure::Retryable | JiraTransitionFailure::Conflict => ("conflict", now),
            JiraTransitionFailure::Uncertain => ("uncertain", now),
        };
        let changed = transaction.execute(
            "UPDATE jira_transition_deliveries
             SET state = ?2, available_at = ?3, last_error = ?4, updated_at = ?5
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, state, available_at, error_code, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Converts crash-interrupted Jira requests into explicit uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_jira_transitions(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE jira_transition_deliveries
             SET state = 'uncertain', last_error = 'delivery_interrupted', updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Requeues one explicitly conflicting or uncertain Jira update for operator retry.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn retry_jira_transition(&self, task_id: TaskId) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE jira_transition_deliveries
             SET state = 'queued', attempts = 0, available_at = unixepoch(),
                 attempted_at = NULL, last_error = NULL, updated_at = unixepoch()
             WHERE id = (
                 SELECT id FROM jira_transition_deliveries
                 WHERE task_id = ?1 AND state IN ('conflict','uncertain')
                 ORDER BY updated_at DESC, id DESC LIMIT 1
             )",
            [task_id.to_string()],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns the latest unresolved outbound Jira state for one task.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn jira_transition_state_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT state FROM jira_transition_deliveries
                 WHERE task_id = ?1 AND state <> 'delivered'
                 ORDER BY updated_at DESC, id DESC LIMIT 1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Queues one bounded Jira comment for durable delivery.
    ///
    /// # Errors
    ///
    /// Returns an error when the comment is invalid, the task has no Jira issue
    /// link, or the database transaction cannot be completed.
    pub fn queue_jira_comment(
        &self,
        task_id: TaskId,
        body: &str,
    ) -> Result<String, TaskStoreError> {
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_COMMENT_BYTES || body.chars().any(|c| c == '\0') {
            return Err(TaskStoreError::InvalidJiraComment);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let linked = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM jira_issue_links WHERE task_id = ?1)",
            [task_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if !linked {
            return Err(TaskStoreError::NotFound);
        }
        let pending = transaction.query_row(
            "SELECT COUNT(*) FROM jira_comment_deliveries
             WHERE state IN ('queued','dispatching')",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if pending >= MAX_PENDING_COMMENTS {
            return Err(TaskStoreError::JiraCommentQueueFull);
        }
        let id = uuid::Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT INTO jira_comment_deliveries (id, task_id, body, state)
             VALUES (?1, ?2, ?3, 'queued')",
            params![id, task_id.to_string(), body],
        )?;
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, note, actor_kind)
             VALUES (?1, 'details_updated', ?2, 'operator')",
            params![task_id.to_string(), "Jira comment queued"],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Atomically claims a bounded batch of queued Jira comments.
    ///
    /// # Errors
    ///
    /// Returns an error when the queued comments cannot be read or atomically
    /// moved into the dispatching state.
    pub fn claim_jira_comments(
        &self,
        now: i64,
    ) -> Result<Vec<JiraCommentDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT delivery.id, delivery.task_id, link.issue_key, delivery.body
                 FROM jira_comment_deliveries delivery
                 JOIN jira_issue_links link ON link.task_id = delivery.task_id
                 WHERE delivery.state = 'queued' AND delivery.available_at <= ?1
                   AND delivery.attempts < 3
                 ORDER BY delivery.created_at, delivery.id LIMIT 16",
            )?;
            statement
                .query_map([now], |row| {
                    Ok(JiraCommentDispatch {
                        id: row.get(0)?,
                        task_id: parse_domain_id(&row.get::<_, String>(1)?)?,
                        issue_key: row.get(2)?,
                        body: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for delivery in &candidates {
            let changed = transaction.execute(
                "UPDATE jira_comment_deliveries
                 SET state = 'dispatching', attempts = attempts + 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND state = 'queued' AND attempts < 3",
                params![delivery.id, now],
            )?;
            if changed != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "Jira comment claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Marks a claimed Jira comment as delivered.
    ///
    /// # Errors
    ///
    /// Returns an error when the delivery record cannot be read or updated.
    pub fn complete_jira_comment(&self, id: &str, now: i64) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task_id = transaction
            .query_row(
                "SELECT task_id FROM jira_comment_deliveries
                 WHERE id = ?1 AND state = 'dispatching'",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(task_id) = task_id else {
            return Ok(false);
        };
        let changed = transaction.execute(
            "UPDATE jira_comment_deliveries
             SET state = 'delivered', delivered_at = ?2, last_error = NULL, updated_at = ?2
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, now],
        )? == 1;
        if changed {
            transaction.execute(
                "INSERT INTO task_activity (task_id, kind, note, actor_kind)
                 VALUES (?1, 'details_updated', 'Comment delivered to Jira', 'jira')",
                [task_id],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Records a failed Jira comment delivery and schedules its next state.
    ///
    /// # Errors
    ///
    /// Returns an error when the delivery record cannot be read or updated.
    pub fn fail_jira_comment(
        &self,
        id: &str,
        now: i64,
        retryable: bool,
        error_code: &str,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let attempts = transaction
            .query_row(
                "SELECT attempts FROM jira_comment_deliveries
                 WHERE id = ?1 AND state = 'dispatching'",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(attempts) = attempts else {
            return Ok(false);
        };
        let (state, available_at) = if retryable && attempts < 3 {
            (
                "queued",
                now.saturating_add(30_i64.saturating_mul(attempts.max(1))),
            )
        } else {
            ("conflict", now)
        };
        let changed = transaction.execute(
            "UPDATE jira_comment_deliveries
             SET state = ?2, available_at = ?3, last_error = ?4, updated_at = ?5
             WHERE id = ?1 AND state = 'dispatching'",
            params![id, state, available_at, error_code, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Marks comments left in flight by a stopped process as uncertain.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot update the delivery records.
    pub fn recover_inflight_jira_comments(&self) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE jira_comment_deliveries
                 SET state = 'uncertain', last_error = 'delivery_interrupted', updated_at = unixepoch()
                 WHERE state = 'dispatching'",
                [],
            )
            .map_err(Into::into)
    }

    /// Returns the most recent undelivered Jira comment state for a task.
    ///
    /// # Errors
    ///
    /// Returns an error when the delivery state cannot be read from the database.
    pub fn jira_comment_state_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT state FROM jira_comment_deliveries
                 WHERE task_id = ?1 AND state <> 'delivered'
                 ORDER BY updated_at DESC, id DESC LIMIT 1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Requeues conflicted or uncertain Jira comments for a task.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot update the delivery records.
    pub fn retry_jira_comments(&self, task_id: TaskId) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE jira_comment_deliveries
             SET state = 'queued', attempts = 0, available_at = unixepoch(),
                 attempted_at = NULL, last_error = NULL, updated_at = unixepoch()
             WHERE task_id = ?1 AND state IN ('conflict','uncertain')",
            [task_id.to_string()],
        )?;
        Ok(changed > 0)
    }
}

pub(super) fn queue_jira_transition(
    transaction: &rusqlite::Transaction<'_>,
    task_id: TaskId,
    target: TaskState,
) -> Result<(), TaskStoreError> {
    let link = transaction
        .query_row(
            "SELECT binding_id, jira_status_id FROM jira_issue_links WHERE task_id = ?1",
            [task_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((binding_id, current_status_id)) = link else {
        return Ok(());
    };
    let target_status_ids = {
        let mut statement = transaction.prepare(
            "SELECT jira_status_id FROM jira_status_mappings
             WHERE binding_id = ?1 AND task_state = ?2",
        )?;
        statement
            .query_map(params![binding_id, target.to_string()], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    if target_status_ids.is_empty() {
        return Err(TaskStoreError::InvalidJiraWorkflowMapping);
    }
    if target_status_ids
        .iter()
        .any(|status| status == &current_status_id)
    {
        return Ok(());
    }
    let active: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM jira_transition_deliveries
         WHERE task_id = ?1 AND state <> 'delivered'",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if active > 0 {
        return Err(TaskStoreError::JiraTransitionPending);
    }
    let pending: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM jira_transition_deliveries
         WHERE state IN ('queued','dispatching')",
        [],
        |row| row.get(0),
    )?;
    if pending >= MAX_PENDING_TRANSITIONS {
        return Err(TaskStoreError::JiraTransitionQueueFull);
    }
    transaction.execute(
        "INSERT INTO jira_transition_deliveries (id, task_id, target_task_state, state)
         VALUES (?1, ?2, ?3, 'queued')",
        params![
            uuid::Uuid::now_v7().to_string(),
            task_id.to_string(),
            target.to_string()
        ],
    )?;
    Ok(())
}

fn jira_issue_link_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JiraIssueLink> {
    Ok(JiraIssueLink {
        issue_id: row.get(0)?,
        issue_key: row.get(1)?,
        binding_id: parse_domain_id(&row.get::<_, String>(2)?)?,
        task_id: parse_domain_id(&row.get::<_, String>(3)?)?,
        jira_status_id: row.get(4)?,
        jira_status_name: row.get(5)?,
        jira_assignee_account_id: row.get(6)?,
        jira_assignee_name: row.get(7)?,
        remote_updated_at: row.get(8)?,
        last_synced_at: row.get(9)?,
    })
}

fn jira_binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JiraProjectBinding> {
    Ok(JiraProjectBinding {
        id: parse_domain_id::<JiraProjectBindingId>(&row.get::<_, String>(0)?)?,
        project_id: row.get(1)?,
        project_key: row.get(2)?,
        project_name: row.get(3)?,
        scope: JiraProjectScope::from_str(&row.get::<_, String>(4)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        hive_id: parse_domain_id(&row.get::<_, String>(5)?)?,
        apiary_id: row
            .get::<_, Option<String>>(6)?
            .map(|value| parse_domain_id(&value))
            .transpose()?,
        access_verified: row.get(7)?,
        workflow_mapped: row.get(8)?,
        auto_sync_assigned: row.get(9)?,
    })
}

fn validate_project(
    id: &str,
    key: &str,
    name: &str,
    scope: JiraProjectScope,
    apiary_id: Option<ApiaryId>,
) -> Result<(), TaskStoreError> {
    let valid = !id.is_empty()
        && id.len() <= MAX_PROJECT_ID_BYTES
        && !key.is_empty()
        && key.len() <= MAX_PROJECT_KEY_BYTES
        && !name.is_empty()
        && name.len() <= MAX_PROJECT_NAME_BYTES
        && !id.chars().any(char::is_control)
        && !key.chars().any(char::is_control)
        && !name.chars().any(char::is_control)
        && matches!(
            (scope, apiary_id),
            (JiraProjectScope::Hive, None) | (JiraProjectScope::Apiary, Some(_))
        );
    if valid {
        Ok(())
    } else {
        Err(TaskStoreError::InvalidJiraProject)
    }
}

fn validate_mappings(mappings: &[JiraStatusMapping]) -> Result<(), TaskStoreError> {
    let mut ids = HashSet::new();
    let valid = !mappings.is_empty()
        && mappings.len() <= MAX_JIRA_STATUSES
        && mappings.iter().all(|mapping| {
            let id = mapping.jira_status_id.trim();
            let name = mapping.jira_status_name.trim();
            !id.is_empty()
                && id.len() <= MAX_STATUS_ID_BYTES
                && !name.is_empty()
                && name.len() <= MAX_STATUS_NAME_BYTES
                && !id.chars().any(char::is_control)
                && !name.chars().any(char::is_control)
                && ids.insert(id.to_owned())
        });
    if valid {
        Ok(())
    } else {
        Err(TaskStoreError::InvalidJiraWorkflowMapping)
    }
}

fn valid_issue(issue: &JiraIssueSnapshot<'_>) -> bool {
    let bounded = |value: &str, max: usize| {
        let value = value.trim();
        !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
    };
    bounded(issue.issue_id, MAX_ISSUE_ID_BYTES)
        && bounded(issue.issue_key, MAX_ISSUE_KEY_BYTES)
        && bounded(issue.summary, super::MAX_TASK_TITLE_BYTES)
        && issue.description.len() <= super::MAX_TASK_DESCRIPTION_BYTES
        && bounded(issue.status_id, MAX_STATUS_ID_BYTES)
        && bounded(issue.status_name, MAX_STATUS_NAME_BYTES)
        && bounded(issue.remote_updated_at, MAX_REMOTE_TIMESTAMP_BYTES)
        && issue
            .assignee_account_id
            .is_none_or(|value| value.len() <= 256 && !value.chars().any(char::is_control))
        && issue
            .assignee_name
            .is_none_or(|value| value.len() <= 240 && !value.chars().any(char::is_control))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskDetailsUpdate, TaskState};

    #[test]
    fn project_binding_is_idempotent_by_remote_project_id() {
        let store = TaskStore::in_memory().unwrap();
        let first = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        let refreshed = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "SITE",
                project_name: "Web Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        assert_eq!(first.id, refreshed.id);
        assert_eq!(refreshed.project_key, "SITE");
        assert!(!refreshed.auto_sync_assigned);
        let enabled = store
            .set_jira_auto_sync_assigned(refreshed.id, true)
            .unwrap();
        assert!(enabled.auto_sync_assigned);
        assert_eq!(store.list_jira_project_bindings().unwrap().len(), 1);
    }

    #[test]
    fn workflow_mapping_is_complete_bounded_and_replaceable() {
        let store = TaskStore::in_memory().unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10002",
                project_key: "OPS",
                project_name: "Operations",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        assert!(!binding.workflow_mapped);
        let mappings = vec![
            JiraStatusMapping {
                jira_status_id: "1".into(),
                jira_status_name: "To Do".into(),
                task_state: TaskState::Ready,
            },
            JiraStatusMapping {
                jira_status_id: "3".into(),
                jira_status_name: "In Progress".into(),
                task_state: TaskState::Active,
            },
        ];
        let stored = store
            .replace_jira_status_mappings(binding.id, &mappings)
            .unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&mappings[0]));
        assert!(stored.contains(&mappings[1]));
        assert!(
            store
                .get_jira_project_binding(binding.id)
                .unwrap()
                .workflow_mapped
        );
        assert!(matches!(
            store.replace_jira_status_mappings(binding.id, &[]),
            Err(TaskStoreError::InvalidJiraWorkflowMapping)
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn issue_sync_is_idempotent_and_preserves_swarm_owned_work() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker(
                "Website",
                ProviderKind::ClaudeCode,
                "/projects/website",
                false,
                1,
            )
            .unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        store
            .replace_jira_status_mappings(
                binding.id,
                &[
                    JiraStatusMapping {
                        jira_status_id: "1".into(),
                        jira_status_name: "To Do".into(),
                        task_state: TaskState::Ready,
                    },
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                ],
            )
            .unwrap();
        let first = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "Confirm every launch condition.",
                    status_id: "1",
                    status_name: "To Do",
                    assignee_account_id: Some("account-1"),
                    assignee_name: Some("Bea"),
                    remote_updated_at: "2026-08-13T12:00:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        assert_eq!(first.workspace, "jira://project/10001");
        assert_eq!(first.state, TaskState::Ready);
        assert_eq!(first.description, "Confirm every launch condition.");
        assert_eq!(first.assigned_worker_id, None);
        store
            .update_task_details(
                first.id,
                &TaskDetailsUpdate {
                    description: Some("Local execution notes".into()),
                    ..TaskDetailsUpdate::default()
                },
            )
            .unwrap();
        store.assign_task_to_worker(first.id, worker.id).unwrap();
        assert_eq!(
            store.get_task(first.id).unwrap().workspace,
            "/projects/website"
        );

        let refreshed = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the public launch page",
                    description: "Confirm desktop and mobile.",
                    status_id: "3",
                    status_name: "In Progress",
                    assignee_account_id: Some("account-1"),
                    assignee_name: Some("Bea"),
                    remote_updated_at: "2026-08-13T13:00:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        assert_eq!(refreshed.id, first.id);
        assert_eq!(refreshed.title, "Polish the public launch page");
        assert_eq!(refreshed.state, TaskState::Active);
        assert_eq!(refreshed.description, "Local execution notes");
        assert_eq!(refreshed.assigned_worker_id, Some(worker.id));
        assert_eq!(store.list_jira_issue_links(binding.id).unwrap().len(), 1);
        store
            .update_jira_issue_link_status(first.id, "4", "In Review")
            .unwrap();
        let transitioned_link = store.list_jira_issue_links(binding.id).unwrap().remove(0);
        assert_eq!(transitioned_link.jira_status_id, "4");
        assert_eq!(transitioned_link.jira_status_name, "In Review");

        let cursor = store
            .list_control_room_events(0)
            .unwrap()
            .events
            .last()
            .unwrap()
            .sequence;
        store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the public launch page",
                    description: "Confirm desktop and mobile.",
                    status_id: "3",
                    status_name: "In Progress",
                    assignee_account_id: Some("account-1"),
                    assignee_name: Some("Bea"),
                    remote_updated_at: "2026-08-13T13:00:00.000+0000",
                }],
            )
            .unwrap();
        assert!(
            store
                .list_control_room_events(cursor)
                .unwrap()
                .events
                .is_empty(),
            "an unchanged Jira snapshot must not wake every connected client"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn local_jira_transition_is_durable_bounded_and_acknowledged() {
        let store = TaskStore::in_memory().unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        store
            .replace_jira_status_mappings(
                binding.id,
                &[
                    JiraStatusMapping {
                        jira_status_id: "1".into(),
                        jira_status_name: "To Do".into(),
                        task_state: TaskState::Ready,
                    },
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                ],
            )
            .unwrap();
        let task = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "",
                    status_id: "1",
                    status_name: "To Do",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T12:00:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);

        let active = store.transition_task(task.id, TaskState::Active).unwrap();
        assert_eq!(active.state, TaskState::Active);
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued")
        );
        let now = 2_000_000_000;
        let dispatch = store.claim_jira_transitions(now).unwrap().remove(0);
        assert_eq!(dispatch.task_id, task.id);
        assert_eq!(dispatch.issue_key, "WEB-42");
        assert_eq!(dispatch.target_task_state, TaskState::Active);
        assert_eq!(dispatch.target_status_ids, vec!["3"]);
        assert!(
            store
                .fail_jira_transition(
                    &dispatch.id,
                    now,
                    JiraTransitionFailure::Retryable,
                    "network_unavailable",
                )
                .unwrap()
        );
        let stale_remote = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "",
                    status_id: "1",
                    status_name: "To Do",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T12:01:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        assert_eq!(stale_remote.state, TaskState::Active);
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued")
        );
        assert!(store.claim_jira_transitions(now + 29).unwrap().is_empty());
        let retry = store.claim_jira_transitions(now + 30).unwrap().remove(0);
        assert_eq!(retry.id, dispatch.id);
        assert!(
            store
                .complete_jira_transition(&retry.id, "3", "In Progress", now + 31)
                .unwrap()
        );
        assert_eq!(store.jira_transition_state_for_task(task.id).unwrap(), None);
        let link = store.jira_issue_link_for_task(task.id).unwrap().unwrap();
        assert_eq!(link.jira_status_id, "3");
        assert_eq!(link.jira_status_name, "In Progress");

        let comment_id = store
            .queue_jira_comment(task.id, "Verified on desktop and mobile")
            .unwrap();
        assert_eq!(
            store
                .jira_comment_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("queued")
        );
        let comment = store.claim_jira_comments(now + 40).unwrap().remove(0);
        assert_eq!(comment.id, comment_id);
        assert_eq!(comment.issue_key, "WEB-42");
        assert_eq!(comment.body, "Verified on desktop and mobile");
        assert!(store.complete_jira_comment(&comment.id, now + 41).unwrap());
        assert_eq!(store.jira_comment_state_for_task(task.id).unwrap(), None);
    }

    #[test]
    fn crash_interrupted_jira_transition_becomes_explicitly_uncertain() {
        let store = TaskStore::in_memory().unwrap();
        let binding = store
            .upsert_jira_project_binding(&JiraProjectBindingInput {
                project_id: "10001",
                project_key: "WEB",
                project_name: "Website Services",
                scope: JiraProjectScope::Hive,
                apiary_id: None,
            })
            .unwrap();
        store
            .replace_jira_status_mappings(
                binding.id,
                &[
                    JiraStatusMapping {
                        jira_status_id: "1".into(),
                        jira_status_name: "To Do".into(),
                        task_state: TaskState::Ready,
                    },
                    JiraStatusMapping {
                        jira_status_id: "3".into(),
                        jira_status_name: "In Progress".into(),
                        task_state: TaskState::Active,
                    },
                ],
            )
            .unwrap();
        let task = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "",
                    status_id: "1",
                    status_name: "To Do",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T12:00:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        store.transition_task(task.id, TaskState::Active).unwrap();
        store.claim_jira_transitions(2_000_000_000).unwrap();

        assert_eq!(store.recover_inflight_jira_transitions().unwrap(), 1);
        assert_eq!(
            store
                .jira_transition_state_for_task(task.id)
                .unwrap()
                .as_deref(),
            Some("uncertain")
        );
        assert!(store.claim_jira_transitions(1_000).unwrap().is_empty());
        assert!(store.retry_jira_transition(task.id).unwrap());
        let retried = store
            .claim_jira_transitions(2_000_000_000)
            .unwrap()
            .remove(0);
        assert!(
            store
                .fail_jira_transition(
                    &retried.id,
                    2_000_000_001,
                    JiraTransitionFailure::Uncertain,
                    "response_unknown",
                )
                .unwrap()
        );
        let acknowledged = store
            .sync_jira_issues(
                binding.id,
                &[JiraIssueSnapshot {
                    issue_id: "20001",
                    issue_key: "WEB-42",
                    summary: "Polish the launch page",
                    description: "",
                    status_id: "3",
                    status_name: "In Progress",
                    assignee_account_id: None,
                    assignee_name: None,
                    remote_updated_at: "2026-08-13T12:02:00.000+0000",
                }],
            )
            .unwrap()
            .remove(0);
        assert_eq!(acknowledged.state, TaskState::Active);
        assert_eq!(store.jira_transition_state_for_task(task.id).unwrap(), None);
    }

    #[test]
    fn migrates_schema_v20_to_durable_jira_identity_and_mapping_tables() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        drop(store);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE jira_issue_links;
                 DROP TABLE jira_status_mappings;
                 DROP TABLE jira_project_bindings;
                 PRAGMA user_version = 20;",
            )
            .unwrap();
        drop(connection);

        let migrated = TaskStore::open(&path).unwrap();
        assert!(migrated.list_jira_project_bindings().unwrap().is_empty());
        let version = migrated
            .connection()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, crate::CURRENT_SCHEMA_VERSION);
    }
}
