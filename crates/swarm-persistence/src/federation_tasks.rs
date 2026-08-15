use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ApiaryId, ApiaryTask, ApiaryTaskEvent, ApiaryTaskId, ApiaryTaskSource,
    FEDERATION_PROTOCOL_VERSION, FEDERATION_TASK_FEED_SCHEMA_VERSION, FederationTaskPage,
    FederationTaskSyncStatus, LocalApiaryContext, LocalApiaryRole, TaskPriority, TaskState,
};

use crate::{
    MAX_TASK_DESCRIPTION_BYTES, MAX_TASK_TITLE_BYTES, TaskStore, TaskStoreError,
    federation::{authenticate_member_credential, decode_node_credential},
    parse_domain_id,
};

const MAX_FEDERATION_TASK_PAGE: usize = 100;
const MAX_APIARY_TASKS: usize = 10_000;

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
        let task = ApiaryTask {
            id: ApiaryTaskId::new(),
            apiary_id: apiary.id,
            source: ApiaryTaskSource::Swarm,
            title: title.to_owned(),
            description: description.to_owned(),
            priority,
            state: TaskState::Ready,
            home_node_id: None,
            home_hive_id: None,
            revision: 1,
            created_at: now,
            updated_at: now,
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let count = transaction.query_row(
            "SELECT COUNT(*) FROM apiary_tasks WHERE apiary_id = ?1",
            [apiary.id.to_string()],
            |row| row.get::<_, usize>(0),
        )?;
        if count >= MAX_APIARY_TASKS {
            return Err(TaskStoreError::InvalidFederationTask);
        }
        transaction.execute(
            "INSERT INTO apiary_tasks
                (id, apiary_id, source, title, description, priority, state,
                 home_node_id, home_hive_id, revision, created_at, updated_at)
             VALUES (?1, ?2, 'swarm', ?3, ?4, ?5, ?6, NULL, NULL, 1, ?7, ?7)",
            params![
                task.id.to_string(),
                task.apiary_id.to_string(),
                task.title,
                task.description,
                task.priority.to_string(),
                task.state.to_string(),
                now,
            ],
        )?;
        let snapshot_json =
            serde_json::to_string(&task).map_err(|_| TaskStoreError::InvalidFederationTask)?;
        transaction.execute(
            "INSERT INTO apiary_task_events
                (apiary_id, sequence, task_id, task_revision, snapshot_json, occurred_at)
             VALUES (
                ?1,
                (SELECT COALESCE(MAX(sequence), 0) + 1
                 FROM apiary_task_events WHERE apiary_id = ?1),
                ?2, 1, ?3, ?4
             )",
            params![
                task.apiary_id.to_string(),
                task.id.to_string(),
                snapshot_json,
                now
            ],
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
