//! A worker's independent checks of its task's commits (ADR 0171).
//!
//! Append-only and pruned per task. A check is stored with the digest of the
//! commits it was built from, and the comparison that decides whether a
//! result still speaks for the work happens in the same transaction that
//! stores it, so a commit recorded in between can never be pinned to a check
//! that did not see it.

use rusqlite::{OptionalExtension as _, params};
use swarm_domain::{
    IndependentCheck, IndependentCheckVerdict, TaskId, commits_digest, independent_check_basis,
};

use crate::{TaskStore, TaskStoreError};

/// How many checks one task keeps. The latest decides what is shown; the few
/// before it say whether it failed first. It bounds the table, it is not a
/// quota.
const CHECKS_KEPT_PER_TASK: i64 = 10;

pub(crate) fn migrate_independent_checks(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_independent_checks (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
             commits TEXT NOT NULL CHECK (length(commits) BETWEEN 1 AND 20000),
             head TEXT NOT NULL CHECK (length(head) BETWEEN 4 AND 64),
             verdict TEXT NOT NULL CHECK (verdict IN ('pass','partial','fail')),
             ran TEXT NOT NULL CHECK (length(ran) BETWEEN 1 AND 1000),
             findings TEXT NOT NULL DEFAULT '' CHECK (length(findings) <= 4000),
             recorded_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS task_independent_checks_by_task
             ON task_independent_checks(task_id, id);",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// What became of a check's result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndependentCheckRecording {
    Recorded(IndependentCheck),
    /// The task has no recorded commits, so there was nothing to check.
    NoCommits,
    /// The task's commits are not the ones the checker was given. `current`
    /// is the basis the input would carry now.
    CommitsMoved {
        current: String,
    },
}

/// One result to store, before the store has compared it with the task.
pub struct IndependentCheckResult<'a> {
    /// The basis the checker's input carried.
    pub basis: &'a str,
    /// The newest commit the input covered.
    pub head: &'a str,
    pub verdict: IndependentCheckVerdict,
    pub ran: &'a str,
    pub findings: &'a str,
    pub now: i64,
}

impl TaskStore {
    /// Stores a check's result, if the task's commits are still the ones it
    /// was given.
    ///
    /// # Errors
    /// Returns an error when the task does not exist or persistence is
    /// unavailable.
    pub fn record_independent_check(
        &self,
        task_id: TaskId,
        result: &IndependentCheckResult<'_>,
    ) -> Result<IndependentCheckRecording, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        let shas: Vec<String> = {
            let mut statement =
                transaction.prepare("SELECT sha FROM task_commits WHERE task_id = ?1")?;
            statement
                .query_map([task_id.to_string()], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        if shas.is_empty() {
            return Ok(IndependentCheckRecording::NoCommits);
        }
        let current = independent_check_basis(&shas);
        if current != result.basis.trim() {
            return Ok(IndependentCheckRecording::CommitsMoved { current });
        }
        let check = IndependentCheck {
            commits: commits_digest(&shas),
            head: result.head.to_owned(),
            verdict: result.verdict,
            ran: result.ran.trim().to_owned(),
            findings: result.findings.trim().to_owned(),
            recorded_at: result.now,
        };
        transaction.execute(
            "INSERT INTO task_independent_checks
                 (task_id, commits, head, verdict, ran, findings, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                task_id.to_string(),
                check.commits,
                check.head,
                check.verdict.to_string(),
                check.ran,
                check.findings,
                check.recorded_at
            ],
        )?;
        transaction.execute(
            "DELETE FROM task_independent_checks
              WHERE task_id = ?1
                AND id NOT IN (SELECT id FROM task_independent_checks
                                WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2)",
            params![task_id.to_string(), CHECKS_KEPT_PER_TASK],
        )?;
        transaction.commit()?;
        Ok(IndependentCheckRecording::Recorded(check))
    }

    /// The task's most recent check, or `None` when nobody recorded one.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn latest_independent_check(
        &self,
        task_id: TaskId,
    ) -> Result<Option<IndependentCheck>, TaskStoreError> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT commits, head, verdict, ran, findings, recorded_at
                   FROM task_independent_checks
                  WHERE task_id = ?1
                  ORDER BY id DESC LIMIT 1",
                [task_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((commits, head, verdict, ran, findings, recorded_at)) = row else {
            return Ok(None);
        };
        let verdict = verdict.parse().map_err(|_| {
            TaskStoreError::IntegrityFailure(format!("unknown independent check verdict {verdict}"))
        })?;
        Ok(Some(IndependentCheck {
            commits,
            head,
            verdict,
            ran,
            findings,
            recorded_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        CommitRepositoryState, CommitVerdict, IndependentCheckState, IndependentCheckVerdict,
        TaskCommit, independent_check_basis, independent_check_state,
    };

    use super::{IndependentCheckRecording, IndependentCheckResult};
    use crate::TaskStore;

    fn commit(sha: &str) -> TaskCommit {
        TaskCommit {
            sha: sha.to_owned(),
            verdict: CommitVerdict::Present,
            subject: format!("feat: {sha}"),
            changed_paths: vec!["src/lib.rs".to_owned()],
            found_in: None,
        }
    }

    fn record_commits(store: &TaskStore, task: swarm_domain::TaskId, shas: &[&str]) {
        let commits: Vec<TaskCommit> = shas.iter().map(|sha| commit(sha)).collect();
        store
            .record_task_commits(task, "/work", CommitRepositoryState::Read, &commits, 1_000)
            .unwrap();
    }

    fn result(basis: &str, verdict: IndependentCheckVerdict) -> IndependentCheckResult<'_> {
        IndependentCheckResult {
            basis,
            head: "bbbbbbbb",
            verdict,
            ran: " sh scripts/verify.sh all ",
            findings: "",
            now: 2_000,
        }
    }

    fn basis(shas: &[&str]) -> String {
        independent_check_basis(&shas.iter().map(|sha| (*sha).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn a_result_for_the_current_commits_is_stored_and_reads_back_as_the_latest() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Build it", "/work").unwrap();
        record_commits(&store, task.id, &["aaaaaaaa", "bbbbbbbb"]);

        let recorded = store
            .record_independent_check(
                task.id,
                &result(
                    &basis(&["bbbbbbbb", "aaaaaaaa"]),
                    IndependentCheckVerdict::Pass,
                ),
            )
            .unwrap();

        let IndependentCheckRecording::Recorded(check) = recorded else {
            panic!("expected a stored check, got {recorded:?}");
        };
        assert_eq!(check.ran, "sh scripts/verify.sh all", "trimmed when stored");
        assert_eq!(
            store.latest_independent_check(task.id).unwrap(),
            Some(check)
        );
    }

    #[test]
    fn a_task_with_no_commits_has_nothing_to_check() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Paperwork", "/work").unwrap();
        record_commits(&store, task.id, &[]);

        let recorded = store
            .record_independent_check(task.id, &result(&basis(&[]), IndependentCheckVerdict::Pass))
            .unwrap();

        assert_eq!(recorded, IndependentCheckRecording::NoCommits);
        assert_eq!(store.latest_independent_check(task.id).unwrap(), None);
    }

    #[test]
    fn a_commit_recorded_after_the_input_was_built_refuses_the_result() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Build it", "/work").unwrap();
        record_commits(&store, task.id, &["aaaaaaaa"]);
        let given = basis(&["aaaaaaaa"]);
        record_commits(&store, task.id, &["cccccccc"]);

        let recorded = store
            .record_independent_check(task.id, &result(&given, IndependentCheckVerdict::Pass))
            .unwrap();

        assert_eq!(
            recorded,
            IndependentCheckRecording::CommitsMoved {
                current: basis(&["aaaaaaaa", "cccccccc"])
            }
        );
        assert_eq!(store.latest_independent_check(task.id).unwrap(), None);
    }

    #[test]
    fn a_check_recorded_before_more_commits_reads_stale_afterwards() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Build it", "/work").unwrap();
        record_commits(&store, task.id, &["aaaaaaaa"]);
        store
            .record_independent_check(
                task.id,
                &result(&basis(&["aaaaaaaa"]), IndependentCheckVerdict::Fail),
            )
            .unwrap();
        record_commits(&store, task.id, &["cccccccc"]);

        let report = store.task_commit_report(task.id).unwrap();
        let latest = store.latest_independent_check(task.id).unwrap();
        let state = independent_check_state(report.as_ref(), latest.as_ref());

        assert!(
            matches!(
                &state,
                Some(IndependentCheckState::Stale { since, verdict: IndependentCheckVerdict::Fail, .. })
                    if since == &vec!["cccccccc".to_owned()]
            ),
            "{state:?}"
        );
    }

    #[test]
    fn only_the_latest_ten_checks_of_a_task_are_kept() {
        let store = TaskStore::in_memory().unwrap();
        let task = store.create_task("Build it", "/work").unwrap();
        let other = store.create_task("Another", "/work").unwrap();
        record_commits(&store, task.id, &["aaaaaaaa"]);
        record_commits(&store, other.id, &["aaaaaaaa"]);
        let given = basis(&["aaaaaaaa"]);
        store
            .record_independent_check(other.id, &result(&given, IndependentCheckVerdict::Pass))
            .unwrap();
        for _ in 0..12 {
            store
                .record_independent_check(
                    task.id,
                    &result(&given, IndependentCheckVerdict::Partial),
                )
                .unwrap();
        }

        let count = |id: swarm_domain::TaskId| -> i64 {
            store
                .connection()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM task_independent_checks WHERE task_id = ?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count(task.id), 10);
        assert_eq!(count(other.id), 1, "pruning one task never touches another");
    }

    #[test]
    fn a_recorded_check_survives_reopening_the_database() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("swarm.sqlite");
        let task_id = {
            let store = TaskStore::open(&path).unwrap();
            let task = store.create_task("Build it", "/work").unwrap();
            record_commits(&store, task.id, &["aaaaaaaa"]);
            store
                .record_independent_check(
                    task.id,
                    &result(&basis(&["aaaaaaaa"]), IndependentCheckVerdict::Pass),
                )
                .unwrap();
            task.id
        };

        let reopened = TaskStore::open(&path).unwrap();

        let latest = reopened.latest_independent_check(task_id).unwrap().unwrap();
        assert_eq!(latest.verdict, IndependentCheckVerdict::Pass);
        assert_eq!(latest.head, "bbbbbbbb");
    }

    #[test]
    fn a_result_for_a_task_that_does_not_exist_is_an_error() {
        let store = TaskStore::in_memory().unwrap();
        let missing = swarm_domain::TaskId::new();
        assert!(
            store
                .record_independent_check(missing, &result("x", IndependentCheckVerdict::Pass))
                .is_err()
        );
    }
}
