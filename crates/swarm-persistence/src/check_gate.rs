//! The check gate's storage (ADR 0115): this Hive's level, and CI as last read
//! for each task. The rules are `swarm_domain::gate_verdict`; this keeps the
//! facts they judge and refuses a close inside the transaction that makes it.

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use swarm_domain::{
    CheckGateLevel, CiObservation, CiOutcome, ControlRoomEventKind, GateVerdict, RequiredSetting,
    TaskId,
};

use crate::{
    TaskStore, TaskStoreError, apiary_policy::received_policy_settings_from,
    insert_control_room_event,
};

/// How soon CI that was still running is read again.
const PENDING_REREAD_SECONDS: i64 = 60;
/// How soon CI that failed or could not be read is read again: a rerun can
/// pass, and access can come back.
const SETTLED_REREAD_SECONDS: i64 = 600;

pub(crate) fn migrate_check_gate(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS check_gate_settings (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             level TEXT NOT NULL CHECK (level IN ('off', 'warn', 'require')),
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS task_ci_observations (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             commits TEXT NOT NULL,
             observation_json TEXT NOT NULL,
             observed_at INTEGER NOT NULL
         );",
    )?;
    transaction.pragma_update(None, "user_version", crate::CHECK_GATE_SCHEMA_MARKER)
}

/// This Hive's operator's own level, before any Apiary requirement.
fn own_level(connection: &Connection) -> Result<CheckGateLevel, TaskStoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT level FROM check_gate_settings WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    stored.map_or(Ok(CheckGateLevel::Off), |level| {
        CheckGateLevel::from_str(&level).map_err(TaskStoreError::IntegrityFailure)
    })
}

/// The level this Hive's Apiary requires, if it requires one (ADR 0113).
fn required_level(connection: &Connection) -> Result<Option<CheckGateLevel>, TaskStoreError> {
    Ok(received_policy_settings_from(connection)?
        .into_iter()
        .find(|setting| setting.key == RequiredSetting::CheckGate.key())
        .and_then(|setting| CheckGateLevel::from_str(&setting.value).ok()))
}

/// The level in effect: the Apiary's, where it requires one.
fn effective_level(connection: &Connection) -> Result<CheckGateLevel, TaskStoreError> {
    Ok(required_level(connection)?.unwrap_or(own_level(connection)?))
}

fn recorded_commits(
    connection: &Connection,
    task_id: TaskId,
) -> Result<Vec<String>, TaskStoreError> {
    let mut statement =
        connection.prepare("SELECT sha FROM task_commits WHERE task_id = ?1 ORDER BY sha")?;
    let commits = statement
        .query_map([task_id.to_string()], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(commits)
}

fn observation(
    connection: &Connection,
    task_id: TaskId,
) -> Result<Option<CiObservation>, TaskStoreError> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT observation_json FROM task_ci_observations WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    stored
        .map(|json| {
            serde_json::from_str(&json)
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))
        })
        .transpose()
}

/// Whether this task may close now, judged inside the caller's transaction.
pub(crate) fn verdict(
    connection: &Connection,
    task_id: TaskId,
) -> Result<GateVerdict, TaskStoreError> {
    let level = effective_level(connection)?;
    if level != CheckGateLevel::Require {
        return Ok(GateVerdict::Allow);
    }
    Ok(swarm_domain::gate_verdict(
        level,
        &recorded_commits(connection, task_id)?,
        observation(connection, task_id)?.as_ref(),
    ))
}

/// Records, beside a close the gate let through, that CI was not checked.
pub(crate) fn note_unchecked_close(
    connection: &Connection,
    task_id: TaskId,
    why: &str,
) -> Result<(), TaskStoreError> {
    connection.execute(
        "INSERT INTO task_activity (task_id, kind, note, actor_kind)
         VALUES (?1, 'noted', ?2, 'system')",
        params![
            task_id.to_string(),
            format!("Closed without a CI check: {why}.")
        ],
    )?;
    Ok(())
}

/// The check gate as this Hive runs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckGateSetting {
    /// The level in effect.
    pub level: CheckGateLevel,
    /// The level this Hive's Apiary requires, if it requires one. It is the
    /// level in effect and only its Keeper can change it.
    pub set_by_apiary: Option<CheckGateLevel>,
}

/// A task whose CI is due to be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CiCandidate {
    pub task_id: TaskId,
    pub workspace: String,
    pub commits: Vec<String>,
}

impl TaskStore {
    /// The check gate in effect on this Hive.
    ///
    /// # Errors
    /// Returns persistence or stored-setting integrity failures.
    pub fn check_gate(&self) -> Result<CheckGateSetting, TaskStoreError> {
        let connection = self.connection()?;
        Ok(CheckGateSetting {
            level: effective_level(&connection)?,
            set_by_apiary: required_level(&connection)?,
        })
    }

    /// This Hive's operator choosing its level.
    ///
    /// # Errors
    /// Refuses a level other than the one this Hive's Apiary requires (ADR
    /// 0113), and returns persistence failures.
    pub fn set_check_gate_level(
        &self,
        level: CheckGateLevel,
        now: i64,
    ) -> Result<CheckGateSetting, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let required = required_level(&transaction)?;
        if required.is_some_and(|required| required != level) {
            return Err(TaskStoreError::SetByApiary(
                "your Apiary sets this Hive's check gate; only its Keeper can change it".to_owned(),
            ));
        }
        if required.is_none() && own_level(&transaction)? != level {
            transaction.execute(
                "INSERT INTO check_gate_settings (singleton, level, updated_at)
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(singleton) DO UPDATE SET
                     level = excluded.level, updated_at = excluded.updated_at",
                params![level.as_str(), now],
            )?;
            insert_control_room_event(&transaction, ControlRoomEventKind::RuntimeChanged)?;
        }
        let setting = CheckGateSetting {
            level: effective_level(&transaction)?,
            set_by_apiary: required,
        };
        transaction.commit()?;
        Ok(setting)
    }

    /// Why this Hive cannot read CI at all, while its gate is on: the newest
    /// reading, when it failed for a reason that is the Hive's and not one
    /// repository's (ADR 0115). Cleared by the next reading that gets through.
    ///
    /// # Errors
    /// Returns persistence or stored-observation integrity failures.
    pub fn hive_wide_ci_problem(&self) -> Result<Option<String>, TaskStoreError> {
        let connection = self.connection()?;
        if effective_level(&connection)? == CheckGateLevel::Off {
            return Ok(None);
        }
        let newest: Option<String> = connection
            .query_row(
                "SELECT observation_json FROM task_ci_observations
                 ORDER BY observed_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = newest else {
            return Ok(None);
        };
        let observed: CiObservation = serde_json::from_str(&json)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        Ok(swarm_domain::hive_wide_ci_problem(&observed).map(str::to_owned))
    }

    /// CI as last read for this task, if it has been read.
    ///
    /// # Errors
    /// Returns persistence or stored-observation integrity failures.
    pub fn ci_observation(&self, task_id: TaskId) -> Result<Option<CiObservation>, TaskStoreError> {
        observation(&*self.connection()?, task_id)
    }

    /// Stores what CI says about a task, replacing the last reading.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn record_ci_observation(
        &self,
        task_id: TaskId,
        observed: &CiObservation,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let json = serde_json::to_string(observed)
            .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
        let changed = observation(&transaction, task_id)?.is_none_or(|previous| {
            previous.commits != observed.commits || previous.outcome != observed.outcome
        });
        transaction.execute(
            "INSERT INTO task_ci_observations (task_id, commits, observation_json, observed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(task_id) DO UPDATE SET
                 commits = excluded.commits, observation_json = excluded.observation_json,
                 observed_at = excluded.observed_at",
            params![
                task_id.to_string(),
                observed.commits,
                json,
                observed.observed_at
            ],
        )?;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Up to `limit` tasks whose CI is due to be read: work in review or
    /// awaiting release that recorded commits in a repository this Hive read,
    /// while the gate is on.
    ///
    /// ⚠️ BOUNDED, AND A PASS IS FINAL. A task whose commits passed is never
    /// read again until it records more; running CI is read again after a
    /// minute; failed or unreadable CI after ten, because a rerun can pass and
    /// access can come back. Never-read tasks go first.
    ///
    /// # Errors
    /// Returns persistence failures.
    pub fn ci_observation_candidates(
        &self,
        now: i64,
        limit: usize,
    ) -> Result<Vec<CiCandidate>, TaskStoreError> {
        let connection = self.connection()?;
        if effective_level(&connection)? == CheckGateLevel::Off {
            return Ok(Vec::new());
        }
        let mut statement = connection.prepare(
            "SELECT t.id, report.workspace, observation.observation_json, observation.observed_at
             FROM tasks t
             JOIN task_commit_reports report ON report.task_id = t.id
             LEFT JOIN task_ci_observations observation ON observation.task_id = t.id
             WHERE t.state IN ('review', 'awaiting_release')
               AND report.repository_state = 'read'
               AND EXISTS (SELECT 1 FROM task_commits c WHERE c.task_id = t.id)
             ORDER BY observation.observed_at IS NOT NULL, observation.observed_at, t.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut due = Vec::new();
        for (task_id, workspace, json, observed_at) in rows {
            if due.len() == limit {
                break;
            }
            let task_id = crate::parse_domain_id::<TaskId>(&task_id)?;
            let commits = recorded_commits(&connection, task_id)?;
            let previous = json
                .map(|json| serde_json::from_str::<CiObservation>(&json))
                .transpose()
                .map_err(|error| TaskStoreError::IntegrityFailure(error.to_string()))?;
            let is_due = match (previous, observed_at) {
                (Some(previous), Some(observed_at))
                    if previous.commits == swarm_domain::commits_digest(&commits) =>
                {
                    match previous.outcome {
                        CiOutcome::Passed => false,
                        CiOutcome::Pending { .. } => now - observed_at >= PENDING_REREAD_SECONDS,
                        CiOutcome::Failed { .. } | CiOutcome::Unreadable { .. } => {
                            now - observed_at >= SETTLED_REREAD_SECONDS
                        }
                    }
                }
                _ => true,
            };
            if is_due {
                due.push(CiCandidate {
                    task_id,
                    workspace,
                    commits,
                });
            }
        }
        Ok(due)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{CommitRepositoryState, CommitVerdict, TaskCommit, TaskState};

    use super::*;

    const SHA: &str = "1111aaaa2222bbbb";

    fn reviewed_with_commit(store: &TaskStore) -> TaskId {
        let task = store.create_task("Gate me", "/workspace/petal").unwrap();
        for state in [TaskState::Ready, TaskState::Active, TaskState::Review] {
            store.transition_task(task.id, state).unwrap();
        }
        store
            .record_task_commits(
                task.id,
                "/workspace/petal",
                CommitRepositoryState::Read,
                &[TaskCommit {
                    sha: SHA.to_owned(),
                    verdict: CommitVerdict::Present,
                    subject: "fix: the thing".to_owned(),
                    changed_paths: vec!["src/lib.rs".to_owned()],
                    found_in: None,
                }],
                1_000,
            )
            .unwrap();
        task.id
    }

    fn observe(store: &TaskStore, task: TaskId, outcome: CiOutcome) {
        store
            .record_ci_observation(
                task,
                &CiObservation {
                    commits: swarm_domain::commits_digest(&[SHA.to_owned()]),
                    judged: vec![SHA.to_owned()],
                    outcome,
                    observed_at: 2_000,
                },
            )
            .unwrap();
    }

    fn notes(store: &TaskStore, task: TaskId) -> Vec<String> {
        store
            .list_task_activity(task, 50)
            .unwrap()
            .events
            .into_iter()
            .map(|event| event.note)
            .collect()
    }

    /// ⚠️ THE OPERATOR'S RULINGS (decision 01a1378c) AT THE CHOKE POINT EVERY
    /// MANUAL AND COORDINATOR CLOSE PASSES: require holds while CI fails or
    /// runs, and lets unreadable CI through with the reason on the record.
    #[test]
    fn require_holds_a_close_until_ci_passes_and_says_when_it_was_not_checked() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_with_commit(&store);

        // Off by default: nothing changes for a Hive that never chose.
        assert_eq!(store.check_gate().unwrap().level, CheckGateLevel::Off);
        let warned = reviewed_with_commit(&store);
        store
            .set_check_gate_level(CheckGateLevel::Warn, 1_500)
            .unwrap();
        store.transition_task(warned, TaskState::Completed).unwrap();

        store
            .set_check_gate_level(CheckGateLevel::Require, 1_500)
            .unwrap();
        assert!(matches!(
            store.transition_task(task, TaskState::Completed),
            Err(TaskStoreError::ChecksNotPassed(why)) if why.contains("has not been read")
        ));
        observe(
            &store,
            task,
            CiOutcome::Failed {
                failing: vec!["rust".to_owned()],
            },
        );
        assert!(matches!(
            store.transition_task(task, TaskState::Completed),
            Err(TaskStoreError::ChecksNotPassed(why)) if why == "CI failed on 1111aaaa: rust"
        ));
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);

        observe(
            &store,
            task,
            CiOutcome::Unreadable {
                why: "no CI is set up for this repository".to_owned(),
            },
        );
        store.transition_task(task, TaskState::Completed).unwrap();
        assert!(notes(&store, task).contains(
            &"Closed without a CI check: no CI is set up for this repository.".to_owned()
        ));

        // Work with no recorded commits has nothing for CI to have run on.
        let paperwork = store.create_task("Paperwork", "/workspace/petal").unwrap();
        for state in [
            TaskState::Ready,
            TaskState::Active,
            TaskState::Review,
            TaskState::Completed,
        ] {
            store.transition_task(paperwork.id, state).unwrap();
        }
    }

    /// The release settle writes the state itself, so it asks the gate itself:
    /// held work stays parked for the next deployment or sweep.
    #[test]
    fn released_work_stays_awaiting_release_while_its_ci_fails() {
        let store = TaskStore::in_memory().unwrap();
        store
            .set_check_gate_level(CheckGateLevel::Require, 1_500)
            .unwrap();
        let task = reviewed_with_commit(&store);
        store
            .transition_task(task, TaskState::AwaitingRelease)
            .unwrap();
        observe(
            &store,
            task,
            CiOutcome::Pending {
                waiting: vec!["rust".to_owned()],
            },
        );
        store
            .record_task_deployment(task, "release", "v1.0.1 at 1111aaaa", 3_000)
            .unwrap();
        assert_eq!(
            store.get_task(task).unwrap().state,
            TaskState::AwaitingRelease
        );

        observe(&store, task, CiOutcome::Passed);
        store
            .record_task_deployment(task, "release", "v1.0.2 at 1111aaaa", 4_000)
            .unwrap();
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Completed);
    }

    /// Only work that recorded commits, in review or awaiting release, while
    /// the gate is on, and a pass is never read again.
    #[test]
    fn ci_is_read_only_for_gated_work_and_a_pass_is_final() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_with_commit(&store);
        assert!(
            store
                .ci_observation_candidates(5_000, 10)
                .unwrap()
                .is_empty()
        );

        store
            .set_check_gate_level(CheckGateLevel::Warn, 1_500)
            .unwrap();
        let due = store.ci_observation_candidates(5_000, 10).unwrap();
        assert_eq!(
            due,
            vec![CiCandidate {
                task_id: task,
                workspace: "/workspace/petal".to_owned(),
                commits: vec![SHA.to_owned()],
            }]
        );

        observe(&store, task, CiOutcome::Pending { waiting: vec![] });
        assert!(
            store
                .ci_observation_candidates(2_030, 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.ci_observation_candidates(2_060, 10).unwrap().len(), 1);

        observe(&store, task, CiOutcome::Passed);
        assert!(
            store
                .ci_observation_candidates(999_999, 10)
                .unwrap()
                .is_empty(),
            "a pass is final until the task records more commits"
        );
        assert_eq!(
            store.ci_observation(task).unwrap().map(|seen| seen.outcome),
            Some(CiOutcome::Passed)
        );
    }

    /// The member's half of the Keeper's view: the newest reading, when it says
    /// this machine cannot read CI at all, and nothing once a reading gets
    /// through or the gate is off.
    #[test]
    fn the_newest_reading_says_whether_this_hive_can_read_ci_at_all() {
        let store = TaskStore::in_memory().unwrap();
        let task = reviewed_with_commit(&store);
        store
            .set_check_gate_level(CheckGateLevel::Require, 1_500)
            .unwrap();
        assert_eq!(store.hive_wide_ci_problem().unwrap(), None);

        observe(
            &store,
            task,
            CiOutcome::Unreadable {
                why: swarm_domain::CI_GH_SIGNED_OUT.to_owned(),
            },
        );
        assert_eq!(
            store.hive_wide_ci_problem().unwrap().as_deref(),
            Some("gh is not logged in on this machine")
        );
        observe(
            &store,
            task,
            CiOutcome::Unreadable {
                why: "no CI ran on this work".to_owned(),
            },
        );
        assert_eq!(
            store.hive_wide_ci_problem().unwrap(),
            None,
            "one repository without CI is that repository's, not the Hive's"
        );
        observe(
            &store,
            task,
            CiOutcome::Unreadable {
                why: swarm_domain::CI_GH_MISSING.to_owned(),
            },
        );
        store
            .set_check_gate_level(CheckGateLevel::Off, 1_600)
            .unwrap();
        assert_eq!(
            store.hive_wide_ci_problem().unwrap(),
            None,
            "no gate, nothing to report"
        );
    }
}
