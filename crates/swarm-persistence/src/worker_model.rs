//! The worker model's storage (ADR 0116): this Hive's choice, the model Swarm
//! wrote beside each repository worker's config, and the model each worker's
//! current session started with. The rules are `swarm_domain::worker_model`.

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    ControlRoomEventKind, RequiredSetting, WorkerId, WorkerModel, WorkerSessionId,
    session_short_of_model,
};

use crate::{
    TaskStore, TaskStoreError, apiary_policy::received_policy_settings_from,
    insert_control_room_event,
};

pub(crate) fn migrate_worker_model(
    transaction: &rusqlite::Transaction<'_>,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_model_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             model TEXT NOT NULL CHECK (model IN ('default', 'opus', 'sonnet', 'haiku')),
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS worker_model_launches (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             written TEXT,
             session_id TEXT,
             launched_with TEXT,
             launched_at INTEGER
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::WORKER_MODEL_SCHEMA_MARKER)
}

/// This Hive's operator's own choice, before any Apiary requirement.
fn own_model(connection: &Connection) -> Result<WorkerModel, TaskStoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT model FROM worker_model_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    stored.map_or(Ok(WorkerModel::Default), |model| {
        WorkerModel::from_str(&model).map_err(TaskStoreError::IntegrityFailure)
    })
}

/// The model this Hive's Apiary requires, if it requires one (ADR 0113).
fn required_model(connection: &Connection) -> Result<Option<WorkerModel>, TaskStoreError> {
    Ok(received_policy_settings_from(connection)?
        .into_iter()
        .find(|setting| setting.key == RequiredSetting::WorkerModel.key())
        .and_then(|setting| WorkerModel::from_str(&setting.value).ok()))
}

/// The model in effect: the Apiary's, where it requires one.
pub(crate) fn effective_model(connection: &Connection) -> Result<WorkerModel, TaskStoreError> {
    Ok(required_model(connection)?.unwrap_or(own_model(connection)?))
}

/// The Claude repository workers running a session that started on something
/// other than the model in effect. Queen is never counted: she keeps her own
/// default (decision 01a13bc9).
pub(crate) fn workers_short_of(
    connection: &Connection,
    model: WorkerModel,
) -> Result<Vec<WorkerId>, TaskStoreError> {
    if model.alias().is_none() {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT session.session_id, launch.session_id, launch.launched_with, session.worker_id
         FROM worker_sessions AS session
         JOIN worker_profiles AS worker ON worker.id = session.worker_id
         LEFT JOIN worker_model_launches AS launch ON launch.worker_id = session.worker_id
         WHERE session.ended_at IS NULL
           AND worker.role = 'worker'
           AND worker.provider = 'claude_code'
           AND worker.archived_at IS NULL",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut short = Vec::new();
    for row in rows {
        let (session, launched_session, launched_with, worker) = row?;
        // A launch recorded for an earlier session says nothing about this one.
        let launched_with = launched_with.filter(|_| launched_session.as_deref() == Some(&session));
        if session_short_of_model(model, launched_with.as_deref()) {
            short.push(
                WorkerId::from_str(&worker).map_err(|_| {
                    TaskStoreError::IntegrityFailure("invalid worker identity".into())
                })?,
            );
        }
    }
    Ok(short)
}

fn setting_from(connection: &Connection) -> Result<WorkerModelSetting, TaskStoreError> {
    let model = effective_model(connection)?;
    Ok(WorkerModelSetting {
        model,
        set_by_apiary: required_model(connection)?,
        workers_short: u32::try_from(workers_short_of(connection, model)?.len())
            .unwrap_or(u32::MAX),
    })
}

/// The worker model as this Hive runs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerModelSetting {
    /// The model in effect.
    pub model: WorkerModel,
    /// The model this Hive's Apiary requires, if it requires one. It is the
    /// model in effect and only its Keeper can change it.
    pub set_by_apiary: Option<WorkerModel>,
    /// Running repository workers that started on another model, and keep it
    /// until they next start.
    pub workers_short: u32,
}

impl TaskStore {
    /// The worker model in effect on this Hive.
    ///
    /// # Errors
    /// Returns persistence or stored-setting integrity failures.
    pub fn worker_model(&self) -> Result<WorkerModelSetting, TaskStoreError> {
        let connection = self.connection()?;
        setting_from(&connection)
    }

    /// This Hive's operator choosing its workers' model.
    ///
    /// # Errors
    /// Refuses a model other than the one this Hive's Apiary requires (ADR
    /// 0113), and returns persistence failures.
    pub fn set_worker_model(
        &self,
        model: WorkerModel,
        now: i64,
    ) -> Result<WorkerModelSetting, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let required = required_model(&transaction)?;
        if required.is_some_and(|required| required != model) {
            return Err(TaskStoreError::SetByApiary(
                "your Apiary sets this Hive's worker model; only its Keeper can change it"
                    .to_owned(),
            ));
        }
        if required.is_none() && own_model(&transaction)? != model {
            transaction.execute(
                "INSERT INTO worker_model_settings (singleton, model, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     model = excluded.model, updated_at = excluded.updated_at",
                params![model.as_str(), now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        let setting = setting_from(&transaction)?;
        transaction.commit()?;
        Ok(setting)
    }

    /// The model Swarm last wrote beside this worker's config, if it still
    /// stands there.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn worker_model_written(
        &self,
        worker_id: WorkerId,
    ) -> Result<Option<String>, TaskStoreError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT written FROM worker_model_launches WHERE worker_id = ?1",
                [worker_id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// Records what Swarm now has in this worker's settings file: the model it
    /// wrote, or `None` once it has taken its own value back out.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_worker_model_written(
        &self,
        worker_id: WorkerId,
        written: Option<&str>,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "INSERT INTO worker_model_launches (worker_id, written) VALUES (?1, ?2)
             ON CONFLICT(worker_id) DO UPDATE SET written = excluded.written",
            params![worker_id.to_string(), written],
        )?;
        Ok(())
    }

    /// Records the model a worker's new session started with, as its settings
    /// file named it at launch, or `None` when it named none.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_worker_launch_model(
        &self,
        worker_id: WorkerId,
        session_id: WorkerSessionId,
        launched_with: Option<&str>,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "INSERT INTO worker_model_launches (worker_id, session_id, launched_with, launched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(worker_id) DO UPDATE SET
                 session_id = excluded.session_id,
                 launched_with = excluded.launched_with,
                 launched_at = excluded.launched_at",
            params![
                worker_id.to_string(),
                session_id.to_string(),
                launched_with,
                now
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ApiaryPolicySetting, ProviderKind};

    use super::*;

    fn running(store: &TaskStore, name: &str) -> (WorkerId, WorkerSessionId) {
        let worker = store
            .create_worker(
                name,
                ProviderKind::ClaudeCode,
                &format!("/workspace/{name}"),
                false,
                0,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        (worker.id, session)
    }

    /// Stands in for a verified policy body. The signed path that writes it
    /// is the API test's, which runs a real Keeper and member.
    fn require(store: &TaskStore, settings: &[ApiaryPolicySetting]) {
        store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO local_policy_manifest
                    (singleton, policy_revision, settings_digest, settings_json, received_at)
                 VALUES (1, 1, 'test', ?1, 1)
                 ON CONFLICT(singleton) DO UPDATE SET settings_json = excluded.settings_json",
                [serde_json::to_string(settings).unwrap()],
            )
            .unwrap();
    }

    /// Default by default: a Hive that never chose names no model, and asks
    /// nothing of the workers it already runs.
    #[test]
    fn a_hive_starts_on_claudes_default_and_chooses_once() {
        let store = TaskStore::in_memory().unwrap();
        running(&store, "petal");
        assert_eq!(
            store.worker_model().unwrap(),
            WorkerModelSetting {
                model: WorkerModel::Default,
                set_by_apiary: None,
                workers_short: 0
            }
        );

        let chosen = store.set_worker_model(WorkerModel::Sonnet, 10).unwrap();
        assert_eq!(chosen.model, WorkerModel::Sonnet);
        assert_eq!(
            chosen.workers_short, 1,
            "a worker running since before the choice keeps its model until it next starts"
        );
        assert_eq!(store.worker_model().unwrap().model, WorkerModel::Sonnet);
    }

    /// ⚠️ IN EFFECT MEANS STARTED ON IT. A worker counts as on the model only
    /// once its CURRENT session started with it; a launch recorded for an
    /// earlier session, or on another model, still leaves it short.
    #[test]
    fn a_worker_is_on_the_model_once_its_current_session_started_with_it() {
        let store = TaskStore::in_memory().unwrap();
        store.set_worker_model(WorkerModel::Sonnet, 10).unwrap();
        let (petal, first) = running(&store, "petal");
        let (stem, stem_session) = running(&store, "stem");

        store
            .record_worker_launch_model(petal, first, Some("sonnet"), 20)
            .unwrap();
        store
            .record_worker_launch_model(stem, stem_session, Some("opus"), 20)
            .unwrap();
        assert_eq!(
            store.worker_model().unwrap().workers_short,
            1,
            "stem is on opus"
        );

        // Petal restarts outside this path: its record is for the old session.
        store.release_worker_session(first).unwrap();
        store
            .bind_worker_session(petal, WorkerSessionId::new())
            .unwrap();
        assert_eq!(store.worker_model().unwrap().workers_short, 2);

        // A worker that is not running is not short of anything.
        store.release_worker_session(stem_session).unwrap();
        assert_eq!(store.worker_model().unwrap().workers_short, 1);

        // Back on Claude's default, nobody is short.
        store.set_worker_model(WorkerModel::Default, 30).unwrap();
        assert_eq!(store.worker_model().unwrap().workers_short, 0);
    }

    /// Queen keeps her own default, so she is never counted as short of the
    /// Hive's model (decision 01a13bc9).
    #[test]
    fn queen_is_never_counted() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();
        store.set_worker_model(WorkerModel::Haiku, 10).unwrap();
        assert_eq!(store.worker_model().unwrap().workers_short, 0);
    }

    #[test]
    fn what_swarm_wrote_is_remembered_until_it_takes_it_back() {
        let store = TaskStore::in_memory().unwrap();
        let (petal, session) = running(&store, "petal");
        assert_eq!(store.worker_model_written(petal).unwrap(), None);
        store
            .record_worker_model_written(petal, Some("sonnet"))
            .unwrap();
        store
            .record_worker_launch_model(petal, session, Some("sonnet"), 20)
            .unwrap();
        assert_eq!(
            store.worker_model_written(petal).unwrap().as_deref(),
            Some("sonnet"),
            "recording the launch must not forget what was written"
        );
        store.record_worker_model_written(petal, None).unwrap();
        assert_eq!(store.worker_model_written(petal).unwrap(), None);
    }

    /// ⚠️ ADR 0113: a model the Apiary sets is the one in effect, and the
    /// member's own choice survives underneath it.
    #[test]
    fn a_required_model_wins_and_the_members_own_choice_waits_underneath() {
        let store = TaskStore::in_memory().unwrap();
        store.set_worker_model(WorkerModel::Opus, 10).unwrap();
        require(
            &store,
            &[ApiaryPolicySetting {
                key: "worker.model".to_owned(),
                value: "sonnet".to_owned(),
            }],
        );

        let setting = store.worker_model().unwrap();
        assert_eq!(setting.model, WorkerModel::Sonnet);
        assert_eq!(setting.set_by_apiary, Some(WorkerModel::Sonnet));
        assert!(matches!(
            store.set_worker_model(WorkerModel::Haiku, 20),
            Err(TaskStoreError::SetByApiary(_))
        ));
        assert_eq!(
            store
                .set_worker_model(WorkerModel::Sonnet, 20)
                .unwrap()
                .model,
            WorkerModel::Sonnet,
            "choosing the required model is not a conflict"
        );

        require(&store, &[]);
        assert_eq!(store.worker_model().unwrap().model, WorkerModel::Opus);
    }

    /// ⚠️ THE HOST'S OWN RECOVERY STARTS A SESSION THE API NEVER LAUNCHED. It
    /// relaunches from the same settings file, so the successor carries its
    /// parent's model rather than reading as a worker off it.
    #[test]
    fn a_continuation_successor_keeps_the_model_its_parent_started_with() {
        use swarm_domain::{ConversationRecovery, ConversationRecoveryState};
        let store = TaskStore::in_memory().unwrap();
        store.set_worker_model(WorkerModel::Sonnet, 10).unwrap();
        let (petal, previous) = running(&store, "petal");
        store
            .record_worker_launch_model(petal, previous, Some("sonnet"), 20)
            .unwrap();
        let ConversationRecoveryState::Attempt { attempt } =
            ConversationRecovery::new(None, true).state()
        else {
            panic!("a first attempt");
        };
        let mut recovery = ConversationRecovery::from_attempt(attempt).unwrap();
        recovery.observe(
            attempt,
            swarm_domain::ConversationRecoveryEvidence::ContextUnavailable,
        );
        let ConversationRecoveryState::Attempt { attempt: fresh } = recovery.state() else {
            panic!("a fresh attempt");
        };
        let successor = WorkerSessionId::new();
        assert!(
            store
                .reconcile_continuation_successor(previous, attempt, successor, fresh)
                .unwrap()
        );
        assert_eq!(store.worker_model().unwrap().workers_short, 0);
    }
}
