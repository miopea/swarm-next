use std::str::FromStr;

use rusqlite::{OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, DeploymentAuthorization, DeploymentAuthorizationId, DeploymentGrant,
    DeploymentGrantId, PresenceMode, QueenActionClass, TaskId, WorkerId,
};

use super::{
    TaskStore, TaskStoreError, insert_control_room_event,
    orchestration::queen_autonomy_policy_from_connection,
    presence::operator_presence_from_connection,
};

const MAX_ENVIRONMENT_BYTES: usize = 80;
const MAX_GRANT_USES: u32 = 100;

impl TaskStore {
    pub fn deployment_grants(&self) -> Result<Vec<DeploymentGrant>, TaskStoreError> {
        let connection = self.connection()?;
        list_grants(&connection)
    }

    pub fn create_deployment_grant(
        &self,
        worker_id: WorkerId,
        environment: &str,
        max_uses: u32,
        expires_at: i64,
        now: i64,
    ) -> Result<DeploymentGrant, TaskStoreError> {
        let environment = normalize_environment(environment)?;
        if max_uses == 0 || max_uses > MAX_GRANT_USES || expires_at <= now {
            return Err(TaskStoreError::IntegrityFailure(
                "deployment grant limits are invalid".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let worker_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_profiles WHERE id = ?1)",
            [worker_id.to_string()],
            |row| row.get(0),
        )?;
        if !worker_exists {
            return Err(TaskStoreError::IntegrityFailure(
                "deployment grant worker was not found".into(),
            ));
        }
        let id = DeploymentGrantId::new();
        transaction.execute(
            "INSERT INTO deployment_grants (
                 id, worker_id, environment, max_uses, expires_at, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                worker_id.to_string(),
                environment,
                i64::from(max_uses),
                expires_at,
                now
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        drop(connection);
        self.deployment_grant(id)?.ok_or_else(|| {
            TaskStoreError::IntegrityFailure("created deployment grant disappeared".into())
        })
    }

    pub fn revoke_deployment_grant(
        &self,
        id: DeploymentGrantId,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE deployment_grants SET revoked_at = ?2
             WHERE id = ?1 AND revoked_at IS NULL",
            params![id.to_string(), now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Records Swarm authority before Queen asks Scout or a repository worker to deploy.
    /// Provider and operating-system permissions remain an independent boundary.
    pub fn authorize_night_watch_deployment(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        environment: &str,
        now: i64,
    ) -> Result<DeploymentAuthorization, TaskStoreError> {
        let environment = normalize_environment(environment)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let running: bool = transaction.query_row(
            "SELECT state = 'running' FROM queen_automation WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        let presence = operator_presence_from_connection(&transaction, now)?.mode;
        let policy = queen_autonomy_policy_from_connection(&transaction)?;
        if !running
            || presence != PresenceMode::NightWatch
            || !policy.permits(presence, QueenActionClass::ExternalSideEffect, true)
        {
            return Err(TaskStoreError::IntegrityFailure(
                "deployment rules are available only to an active Night Watch Queen review".into(),
            ));
        }
        let assigned: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks
             WHERE id = ?1 AND removed_at IS NULL AND state IN ('active','review')
               AND assigned_worker_id = ?2)",
            params![task_id.to_string(), worker_id.to_string()],
            |row| row.get(0),
        )?;
        if !assigned {
            return Err(TaskStoreError::IntegrityFailure(
                "deployment task is not active and assigned to that repository worker".into(),
            ));
        }
        if let Some(existing) = authorization_for(&transaction, task_id, worker_id, environment)? {
            transaction.commit()?;
            return Ok(existing);
        }
        let grant_id = transaction
            .query_row(
                "SELECT grant.id FROM deployment_grants grant
                 WHERE grant.worker_id = ?1 AND grant.environment = ?2
                   AND grant.revoked_at IS NULL AND grant.expires_at > ?3
                   AND (SELECT COUNT(*) FROM deployment_authorizations used
                        WHERE used.grant_id = grant.id) < grant.max_uses
                 ORDER BY grant.expires_at, grant.created_at, grant.id LIMIT 1",
                params![worker_id.to_string(), environment, now],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::IntegrityFailure(
                    "no active deployment rule matches this worker and environment".into(),
                )
            })?;
        let grant_id = DeploymentGrantId::from_str(&grant_id)
            .map_err(|_| TaskStoreError::IntegrityFailure("invalid deployment grant id".into()))?;
        let authorization = DeploymentAuthorization {
            id: DeploymentAuthorizationId::new(),
            grant_id,
            task_id,
            worker_id,
            environment: environment.to_owned(),
            authorized_at: now,
        };
        transaction.execute(
            "INSERT INTO deployment_authorizations (
                 id, grant_id, task_id, worker_id, environment, authorized_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                authorization.id.to_string(),
                authorization.grant_id.to_string(),
                task_id.to_string(),
                worker_id.to_string(),
                environment,
                now
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        transaction.commit()?;
        Ok(authorization)
    }

    fn deployment_grant(
        &self,
        id: DeploymentGrantId,
    ) -> Result<Option<DeploymentGrant>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!("{} WHERE grant.id = ?1", GRANT_SELECT))?;
        Ok(statement
            .query_row([id.to_string()], deployment_grant_from_row)
            .optional()?)
    }
}

const GRANT_SELECT: &str = "SELECT grant.id, grant.worker_id, worker.name, worker.workspace,
            grant.environment, grant.max_uses,
            (SELECT COUNT(*) FROM deployment_authorizations used WHERE used.grant_id = grant.id),
            grant.expires_at, grant.revoked_at, grant.created_at
     FROM deployment_grants grant JOIN worker_profiles worker ON worker.id = grant.worker_id";

fn normalize_environment(value: &str) -> Result<&str, TaskStoreError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_ENVIRONMENT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(TaskStoreError::IntegrityFailure(
            "deployment environment is invalid".into(),
        ));
    }
    Ok(value)
}

fn list_grants(connection: &rusqlite::Connection) -> Result<Vec<DeploymentGrant>, TaskStoreError> {
    let sql = format!(
        "{} ORDER BY grant.revoked_at IS NULL DESC, grant.expires_at DESC, grant.created_at DESC",
        GRANT_SELECT
    );
    let mut statement = connection.prepare(&sql)?;
    Ok(statement
        .query_map([], deployment_grant_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn deployment_grant_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeploymentGrant> {
    Ok(DeploymentGrant {
        id: DeploymentGrantId::from_str(&row.get::<_, String>(0)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        worker_id: WorkerId::from_str(&row.get::<_, String>(1)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        worker_name: row.get(2)?,
        repository: row.get(3)?,
        environment: row.get(4)?,
        max_uses: row.get(5)?,
        uses: row.get(6)?,
        expires_at: row.get(7)?,
        revoked_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn authorization_for(
    connection: &rusqlite::Connection,
    task_id: TaskId,
    worker_id: WorkerId,
    environment: &str,
) -> Result<Option<DeploymentAuthorization>, TaskStoreError> {
    Ok(connection
        .query_row(
            "SELECT id, grant_id, authorized_at FROM deployment_authorizations
             WHERE task_id = ?1 AND worker_id = ?2 AND environment = ?3",
            params![task_id.to_string(), worker_id.to_string(), environment],
            |row| {
                Ok(DeploymentAuthorization {
                    id: DeploymentAuthorizationId::from_str(&row.get::<_, String>(0)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    grant_id: DeploymentGrantId::from_str(&row.get::<_, String>(1)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_id,
                    worker_id,
                    environment: environment.to_owned(),
                    authorized_at: row.get(2)?,
                })
            },
        )
        .optional()?)
}

pub(super) fn migrate_deployment_grants(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS deployment_grants (
             id TEXT PRIMARY KEY,
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             environment TEXT NOT NULL,
             max_uses INTEGER NOT NULL CHECK (max_uses > 0 AND max_uses <= 100),
             expires_at INTEGER NOT NULL,
             revoked_at INTEGER,
             created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS deployment_grants_match
             ON deployment_grants(worker_id, environment, expires_at)
             WHERE revoked_at IS NULL;
         CREATE TABLE IF NOT EXISTS deployment_authorizations (
             id TEXT PRIMARY KEY,
             grant_id TEXT NOT NULL REFERENCES deployment_grants(id),
             task_id TEXT NOT NULL REFERENCES tasks(id),
             worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
             environment TEXT NOT NULL,
             authorized_at INTEGER NOT NULL,
             UNIQUE(task_id, worker_id, environment)
         );
         CREATE INDEX IF NOT EXISTS deployment_authorizations_by_grant
             ON deployment_authorizations(grant_id, authorized_at);
         PRAGMA user_version = 70;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{
        ProviderKind, QueenAutonomyLevel, QueenAutonomyPolicy, TaskPriority, TaskState,
        WorkerSessionId,
    };

    #[test]
    fn grant_is_exact_consumable_and_revocable() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        let worker = store
            .create_worker("App", ProviderKind::ClaudeCode, "/workspace/app", false, 1)
            .unwrap();
        let task = store
            .create_task_with_details("Deploy app", "", TaskPriority::Normal, "/workspace/app")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        let grant = store
            .create_deployment_grant(worker.id, "production", 1, 1_000, 100)
            .unwrap();
        store
            .set_queen_autonomy_policy(
                QueenAutonomyPolicy {
                    at_hive: QueenAutonomyLevel::Coordinate,
                    away: QueenAutonomyLevel::Coordinate,
                    night_watch: QueenAutonomyLevel::LocalExecution,
                },
                100,
            )
            .unwrap();
        store
            .set_manual_presence(Some(PresenceMode::NightWatch), 100)
            .unwrap();
        store.request_queen_automation_run(100).unwrap();
        let delivery = store.claim_queen_automation(101).unwrap().unwrap();
        store
            .complete_queen_automation_delivery(&delivery.run_id, 102)
            .unwrap();

        let first = store
            .authorize_night_watch_deployment(task.id, worker.id, "production", 103)
            .unwrap();
        let repeat = store
            .authorize_night_watch_deployment(task.id, worker.id, "production", 104)
            .unwrap();
        assert_eq!(first.id, repeat.id);
        assert_eq!(store.deployment_grants().unwrap()[0].uses, 1);
        assert!(
            store
                .authorize_night_watch_deployment(task.id, worker.id, "staging", 104)
                .is_err()
        );
        assert!(store.revoke_deployment_grant(grant.id, 105).unwrap());
        assert!(store.deployment_grants().unwrap()[0].revoked_at.is_some());
    }

    #[test]
    fn grant_cannot_be_consumed_outside_night_watch() {
        let store = TaskStore::in_memory().unwrap();
        let worker = store
            .create_worker("App", ProviderKind::ClaudeCode, "/workspace/app", false, 1)
            .unwrap();
        let task = store
            .create_task_with_details("Deploy app", "", TaskPriority::Normal, "/workspace/app")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.assign_task_to_worker(task.id, worker.id).unwrap();
        store.transition_task(task.id, TaskState::Active).unwrap();
        store
            .create_deployment_grant(worker.id, "production", 1, 1_000, 100)
            .unwrap();
        assert!(
            store
                .authorize_night_watch_deployment(task.id, worker.id, "production", 101)
                .is_err()
        );
    }
}
