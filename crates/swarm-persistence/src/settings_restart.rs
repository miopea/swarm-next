//! Which running workers still carry an older value of a setting they read
//! only when they start (ADR 0132), and the record of what each launch carried
//! that the answer is judged by. The rule is `swarm_domain::settings_restart`.

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    LaunchedValue, RestartBoundSetting, SystemAccess, WorkerId, WorkerSessionId,
    session_behind_setting,
};

use crate::{TaskStore, TaskStoreError, worker_model};

pub(crate) fn migrate_worker_launch_choices(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS worker_launch_choices (
             worker_id TEXT PRIMARY KEY REFERENCES worker_profiles(id) ON DELETE CASCADE,
             session_id TEXT NOT NULL,
             remote_control INTEGER CHECK (remote_control IN (0, 1)),
             queen_releases INTEGER CHECK (queen_releases IN (0, 1)),
             system_access TEXT,
             launched_at INTEGER NOT NULL
         );",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// The choices Swarm wrote beside a Claude worker's config before it started.
/// `None` is a choice it could not write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LaunchChoices {
    pub remote_control: Option<bool>,
    pub queen_releases: Option<bool>,
    pub system_access: Option<SystemAccess>,
}

/// A running worker that started on an older value of at least one setting it
/// reads only when it starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerBehindSettings {
    pub worker_id: WorkerId,
    pub name: String,
    /// In the order `RestartBoundSetting` declares them, never empty.
    pub settings: Vec<RestartBoundSetting>,
}

/// A Hive-wide on/off choice and when the operator last changed it.
fn hive_choice(
    connection: &Connection,
    table: &str,
) -> Result<(bool, Option<i64>), TaskStoreError> {
    Ok(connection
        .query_row(
            &format!("SELECT enabled, updated_at FROM {table} WHERE singleton = 1"),
            [],
            |row| Ok((row.get::<_, bool>(0)?, Some(row.get::<_, i64>(1)?))),
        )
        .optional()?
        .unwrap_or((false, None)))
}

fn launched<T>(recorded: bool, value: Option<T>) -> LaunchedValue<T> {
    match (recorded, value) {
        (false, _) => LaunchedValue::Unrecorded,
        (true, Some(value)) => LaunchedValue::Wrote(value),
        (true, None) => LaunchedValue::NotWritten,
    }
}

struct RunningClaudeWorker {
    worker_id: String,
    name: String,
    is_queen: bool,
    started_at: i64,
    system_access: SystemAccess,
    system_access_changed_at: Option<i64>,
    /// Whether the launch record belongs to this session, not an earlier one.
    recorded: bool,
    remote_control: Option<bool>,
    queen_releases: Option<bool>,
    launched_system_access: Option<SystemAccess>,
}

fn running_claude_workers(
    connection: &Connection,
) -> Result<Vec<RunningClaudeWorker>, TaskStoreError> {
    // Bounded by the live sessions, one per worker, that the terminal host
    // holds.
    let mut statement = connection.prepare(
        "SELECT worker.id, worker.name, worker.role, session.started_at,
                worker.system_access,
                (SELECT MAX(change.changed_at) FROM worker_system_access_changes AS change
                 WHERE change.worker_id = worker.id),
                launch.session_id = session.session_id,
                launch.remote_control, launch.queen_releases, launch.system_access
         FROM worker_sessions AS session
         JOIN worker_profiles AS worker ON worker.id = session.worker_id
         LEFT JOIN worker_launch_choices AS launch ON launch.worker_id = session.worker_id
         WHERE session.ended_at IS NULL
           AND worker.provider = 'claude_code'
           AND worker.archived_at IS NULL
         ORDER BY worker.name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(RunningClaudeWorker {
            worker_id: row.get(0)?,
            name: row.get(1)?,
            is_queen: row.get::<_, String>(2)? == "queen",
            started_at: row.get(3)?,
            system_access: SystemAccess::from_stored(&row.get::<_, String>(4)?),
            system_access_changed_at: row.get(5)?,
            recorded: row.get::<_, Option<bool>>(6)?.unwrap_or(false),
            remote_control: row.get(7)?,
            queen_releases: row.get(8)?,
            launched_system_access: row
                .get::<_, Option<String>>(9)?
                .map(|stored| SystemAccess::from_stored(&stored)),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn behind_from(connection: &Connection) -> Result<Vec<WorkerBehindSettings>, TaskStoreError> {
    let (remote_control, remote_control_changed) =
        hive_choice(connection, "worker_remote_control_settings")?;
    let (queen_releases, queen_releases_changed) =
        hive_choice(connection, "queen_release_settings")?;
    let short_of_model =
        worker_model::workers_short_of(connection, worker_model::effective_model(connection)?)?;
    let mut behind = Vec::new();
    for worker in running_claude_workers(connection)? {
        let worker_id = WorkerId::from_str(&worker.worker_id)
            .map_err(|_| TaskStoreError::IntegrityFailure("invalid worker identity".into()))?;
        let mut settings = Vec::new();
        if short_of_model.contains(&worker_id) {
            settings.push(RestartBoundSetting::WorkerModel);
        }
        if session_behind_setting(
            launched(worker.recorded, worker.remote_control),
            &remote_control,
            remote_control_changed,
            worker.started_at,
        ) {
            settings.push(RestartBoundSetting::RemoteControl);
        }
        // Queen takes it in her review prompt at her next review; she never
        // runs the release commands the auto-mode rule allows.
        if !worker.is_queen
            && session_behind_setting(
                launched(worker.recorded, worker.queen_releases),
                &queen_releases,
                queen_releases_changed,
                worker.started_at,
            )
        {
            settings.push(RestartBoundSetting::QueenReleases);
        }
        if session_behind_setting(
            launched(worker.recorded, worker.launched_system_access),
            &worker.system_access,
            worker.system_access_changed_at,
            worker.started_at,
        ) {
            settings.push(RestartBoundSetting::SystemAccess);
        }
        if !settings.is_empty() {
            behind.push(WorkerBehindSettings {
                worker_id,
                name: worker.name,
                settings,
            });
        }
    }
    Ok(behind)
}

impl TaskStore {
    /// Records what Swarm wrote beside a Claude worker's config for the
    /// session that just started from it.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_worker_launch_choices(
        &self,
        worker_id: WorkerId,
        session_id: WorkerSessionId,
        choices: LaunchChoices,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        self.connection()?.execute(
            "INSERT INTO worker_launch_choices
                 (worker_id, session_id, remote_control, queen_releases, system_access, launched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(worker_id) DO UPDATE SET
                 session_id = excluded.session_id,
                 remote_control = excluded.remote_control,
                 queen_releases = excluded.queen_releases,
                 system_access = excluded.system_access,
                 launched_at = excluded.launched_at",
            params![
                worker_id.to_string(),
                session_id.to_string(),
                choices.remote_control,
                choices.queen_releases,
                choices.system_access.map(SystemAccess::as_str),
                now
            ],
        )?;
        Ok(())
    }

    /// The running Claude workers still carrying an older value of a setting
    /// they read only when they start, by name.
    ///
    /// # Errors
    /// Returns persistence failures and stored identities that do not parse.
    pub fn workers_behind_settings(&self) -> Result<Vec<WorkerBehindSettings>, TaskStoreError> {
        let connection = self.connection()?;
        behind_from(&connection)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{ProviderKind, WorkerModel};

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

    fn launch(store: &TaskStore, (worker, session): (WorkerId, WorkerSessionId)) {
        store
            .record_worker_launch_choices(
                worker,
                session,
                LaunchChoices {
                    remote_control: Some(store.worker_remote_control().unwrap()),
                    queen_releases: Some(store.queen_releases().unwrap()),
                    system_access: Some(store.get_worker_profile(worker).unwrap().system_access),
                },
                0,
            )
            .unwrap();
    }

    fn behind(store: &TaskStore) -> Vec<(String, Vec<RestartBoundSetting>)> {
        store
            .workers_behind_settings()
            .unwrap()
            .into_iter()
            .map(|worker| (worker.name, worker.settings))
            .collect()
    }

    #[test]
    fn a_worker_is_behind_only_until_it_starts_on_the_current_settings() {
        let store = TaskStore::in_memory().unwrap();
        let petal = running(&store, "petal");
        launch(&store, petal);
        assert_eq!(behind(&store), vec![]);

        store.set_queen_releases(true, 10).unwrap();
        store.set_worker_remote_control(true, 10).unwrap();
        assert_eq!(
            behind(&store),
            vec![(
                "petal".to_owned(),
                vec![
                    RestartBoundSetting::RemoteControl,
                    RestartBoundSetting::QueenReleases
                ]
            )]
        );

        // Turned back off before anyone restarted: what petal started with is
        // current again, and nothing is asked of it.
        store.set_worker_remote_control(false, 11).unwrap();
        assert_eq!(
            behind(&store),
            vec![("petal".to_owned(), vec![RestartBoundSetting::QueenReleases])]
        );

        // A restart is a new session, launched on what is current now.
        store.release_worker_session(petal.1).unwrap();
        let restarted = WorkerSessionId::new();
        store.bind_worker_session(petal.0, restarted).unwrap();
        assert_eq!(
            behind(&store),
            vec![],
            "the earlier session's record says Queen releases was off, and says \
             nothing about this one, which started after the change"
        );
        launch(&store, (petal.0, restarted));
        assert_eq!(behind(&store), vec![]);
    }

    #[test]
    fn a_choice_swarm_could_not_write_is_not_assumed_to_have_arrived() {
        let store = TaskStore::in_memory().unwrap();
        let (worker, session) = running(&store, "petal");
        store
            .record_worker_launch_choices(
                worker,
                session,
                LaunchChoices {
                    remote_control: Some(false),
                    queen_releases: None,
                    system_access: Some(SystemAccess::None),
                },
                0,
            )
            .unwrap();
        assert_eq!(
            behind(&store),
            vec![("petal".to_owned(), vec![RestartBoundSetting::QueenReleases])]
        );
    }

    #[test]
    fn queen_and_stopped_workers_are_not_asked_to_restart_for_releases() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace").unwrap();
        let queen_session = WorkerSessionId::new();
        store.bind_worker_session(queen.id, queen_session).unwrap();
        launch(&store, (queen.id, queen_session));
        let resting = running(&store, "resting");
        launch(&store, resting);
        store.release_worker_session(resting.1).unwrap();

        store.set_queen_releases(true, 10).unwrap();
        assert_eq!(behind(&store), vec![]);

        // Remote Control does reach Queen at her next start.
        store.set_worker_remote_control(true, 10).unwrap();
        assert_eq!(
            behind(&store),
            vec![(queen.name, vec![RestartBoundSetting::RemoteControl])]
        );
    }

    #[test]
    fn a_session_from_before_the_record_is_judged_by_when_the_setting_changed() {
        let store = TaskStore::in_memory().unwrap();
        running(&store, "petal");
        assert_eq!(
            behind(&store),
            vec![],
            "nothing has changed since it started"
        );

        // Long after the session started: started_at is now, by the clock.
        store.set_queen_releases(true, i64::MAX).unwrap();
        assert_eq!(
            behind(&store),
            vec![("petal".to_owned(), vec![RestartBoundSetting::QueenReleases])]
        );
    }

    #[test]
    fn system_access_and_the_model_are_named_for_the_worker_they_reach() {
        let store = TaskStore::in_memory().unwrap();
        let petal = running(&store, "petal");
        let thistle = running(&store, "thistle");
        launch(&store, petal);
        launch(&store, thistle);
        store
            .record_worker_launch_model(petal.0, petal.1, None, 0)
            .unwrap();
        store
            .record_worker_launch_model(thistle.0, thistle.1, Some("sonnet"), 0)
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE worker_profiles SET system_access = 'inspect' WHERE id = ?1",
                [thistle.0.to_string()],
            )
            .unwrap();
        store.set_worker_model(WorkerModel::Sonnet, 10).unwrap();
        assert_eq!(
            behind(&store),
            vec![
                ("petal".to_owned(), vec![RestartBoundSetting::WorkerModel]),
                (
                    "thistle".to_owned(),
                    vec![RestartBoundSetting::SystemAccess]
                ),
            ]
        );
    }
}
