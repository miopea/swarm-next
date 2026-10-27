//! What an outside tool said about the work it filed (ADR 0126).
//!
//! A connected tool files drafts for Queen to route. It may not know the
//! repository, so the task holds the connection placeholder until Queen assigns
//! it, and assigning sets the worker's repository. Where the work came from and
//! who the tool suggested are kept here, beside the task, rather than in its
//! text.

use std::collections::HashMap;
use std::str::FromStr as _;

use rusqlite::params;
use swarm_domain::{
    IntakeSource, RecommendedWorker, Task, TaskActivityActor, TaskId, TaskIntake, TaskPriority,
    TaskState, WorkerId,
};

use crate::{CONNECTION_WORKSPACE, TaskStore, TaskStoreError, insert_draft, task_from_row};

/// The most filed tasks one listing reads, newest first.
const FILED_TASKS_LIMIT: i64 = 500;

/// Whether `?1` filed the task `t`. The creator comes from the `created`
/// activity row, as `worker_owns_unrouted_draft` reads it, so it is true for
/// work filed before intake existed.
const FILED_BY: &str = "EXISTS (SELECT 1 FROM task_activity created
              WHERE created.task_id = t.id AND created.kind = 'created'
                AND created.actor_kind = 'worker' AND created.actor_id = ?1)";

pub(crate) fn migrate_task_intake(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_intake (
             task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
             repository_named INTEGER NOT NULL CHECK (repository_named IN (0, 1)),
             recommended_worker_id TEXT REFERENCES worker_profiles(id) ON DELETE SET NULL,
             source_link TEXT
                 CHECK (source_link IS NULL OR length(source_link) BETWEEN 1 AND 2048),
             source_jira_key TEXT
                 CHECK (source_jira_key IS NULL OR length(source_jira_key) BETWEEN 1 AND 64),
             requested_by TEXT
                 CHECK (requested_by IS NULL OR length(requested_by) BETWEEN 1 AND 200),
             created_at INTEGER NOT NULL DEFAULT (unixepoch())
         ) WITHOUT ROWID;",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// One filing from an outside tool.
pub struct IntakeFiling<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub priority: TaskPriority,
    /// `None` when the tool named no repository.
    pub workspace: Option<&'a str>,
    pub recommended_worker: Option<WorkerId>,
    pub source: &'a IntakeSource,
}

/// An open task as an outside tool's search reads it: no description.
#[derive(Debug)]
pub struct OpenTaskTitle {
    pub id: TaskId,
    pub title: String,
    pub state: TaskState,
    pub workspace: String,
    pub jira_key: Option<String>,
    pub filed_by_reader: bool,
}

impl TaskStore {
    /// Files a draft and what the tool said about it in one transaction, so no
    /// intake task exists without its record.
    ///
    /// # Errors
    /// Returns an error for invalid content or unavailable persistence.
    pub fn file_intake_task_as(
        &self,
        filing: &IntakeFiling<'_>,
        actor: &TaskActivityActor,
    ) -> Result<Task, TaskStoreError> {
        let named = filing
            .workspace
            .map(str::trim)
            .filter(|workspace| !workspace.is_empty());
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = insert_draft(
            &transaction,
            hive_id,
            filing.title,
            filing.description,
            filing.priority,
            named.unwrap_or(CONNECTION_WORKSPACE),
            actor,
        )?;
        transaction.execute(
            "INSERT INTO task_intake (task_id, repository_named, recommended_worker_id,
                                      source_link, source_jira_key, requested_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                named.is_some(),
                filing.recommended_worker.map(|worker| worker.to_string()),
                filing.source.link,
                filing.source.jira_key,
                filing.source.requested_by,
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_task(id)
    }

    /// What was said at intake about each of these tasks that has a record.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn intake_for_tasks(
        &self,
        ids: &[TaskId],
    ) -> Result<HashMap<TaskId, TaskIntake>, TaskStoreError> {
        let mut found = HashMap::new();
        if ids.is_empty() {
            return Ok(found);
        }
        let connection = self.connection()?;
        let placeholders = vec!["?"; ids.len()].join(",");
        let mut statement = connection.prepare(&format!(
            "SELECT i.task_id, i.repository_named, w.id, w.name,
                    i.source_link, i.source_jira_key, i.requested_by
             FROM task_intake i
             LEFT JOIN worker_profiles w
               ON w.id = i.recommended_worker_id AND w.archived_at IS NULL
             WHERE i.task_id IN ({placeholders})"
        ))?;
        let bound = ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), |row| {
            let worker: Option<String> = row.get(2)?;
            let worker_name: Option<String> = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                TaskIntake {
                    repository_named: row.get(1)?,
                    recommended_worker: worker
                        .and_then(|id| WorkerId::from_str(&id).ok())
                        .zip(worker_name)
                        .map(|(id, name)| RecommendedWorker { id, name }),
                    source: IntakeSource {
                        link: row.get(4)?,
                        jira_key: row.get(5)?,
                        requested_by: row.get(6)?,
                    },
                },
            ))
        })?;
        for row in rows {
            let (task_id, intake) = row?;
            if let Ok(task_id) = TaskId::from_str(&task_id) {
                found.insert(task_id, intake);
            }
        }
        Ok(found)
    }

    /// The tasks this worker filed, newest first, closed ones included.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn list_tasks_filed_by(&self, worker_id: WorkerId) -> Result<Vec<Task>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{projection}
            WHERE t.removed_at IS NULL AND {FILED_BY}
            ORDER BY t.updated_at DESC, t.id
            LIMIT ?2",
            projection = Self::TASK_PROJECTION,
        ))?;
        statement
            .query_map(
                params![worker_id.to_string(), FILED_TASKS_LIMIT],
                task_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()
            .map_err(TaskStoreError::from)
    }

    /// Every open task by title, newest first, with the Jira key it was filed
    /// under and whether `reader` filed it.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn open_task_titles(&self, reader: WorkerId) -> Result<Vec<OpenTaskTitle>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT t.id, t.title, t.state, t.workspace, i.source_jira_key, {FILED_BY}
             FROM tasks t
             LEFT JOIN task_intake i ON i.task_id = t.id
             WHERE t.removed_at IS NULL AND t.state NOT IN ('completed', 'abandoned')
             ORDER BY t.updated_at DESC, t.id"
        ))?;
        let rows = statement.query_map([reader.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, bool>(5)?,
            ))
        })?;
        let mut titles = Vec::new();
        for row in rows {
            let (id, title, state, workspace, jira_key, filed_by_reader) = row?;
            titles.push(OpenTaskTitle {
                id: TaskId::from_str(&id).map_err(|_| {
                    TaskStoreError::IntegrityFailure("unreadable task id on an open task".into())
                })?,
                title,
                state: TaskState::from_str(&state).map_err(|_| {
                    TaskStoreError::IntegrityFailure("unreadable state on an open task".into())
                })?,
                workspace,
                jira_key,
                filed_by_reader,
            });
        }
        Ok(titles)
    }
}

#[cfg(test)]
mod tests {
    use swarm_domain::{
        IntakeSource, ProviderKind, TaskActivityActor, TaskPriority, TaskState, WorkerId,
    };

    use super::IntakeFiling;
    use crate::{CONNECTION_WORKSPACE, TaskStore};

    fn filing<'a>(
        title: &'a str,
        workspace: Option<&'a str>,
        recommended_worker: Option<WorkerId>,
        source: &'a IntakeSource,
    ) -> IntakeFiling<'a> {
        IntakeFiling {
            title,
            description: "From an email.",
            priority: TaskPriority::Normal,
            workspace,
            recommended_worker,
            source,
        }
    }

    /// Without a repository the task holds the placeholder, and assigning it
    /// is what gives it one: the worker's.
    #[test]
    fn a_filing_without_a_repository_takes_the_assigned_workers() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker("Web", ProviderKind::ClaudeCode, "/workspace/web", false, 1)
            .unwrap();
        let tool = store.connection_principal("client-a", "Claude").unwrap();
        let source =
            IntakeSource::normalized(Some("https://mail.example/1"), Some("abc-12"), Some("Ann"))
                .unwrap();

        let task = store
            .file_intake_task_as(
                &filing("Renewal email is late", None, Some(worker.id), &source),
                &TaskActivityActor::worker(tool.id),
            )
            .unwrap();
        assert_eq!(task.state, TaskState::Draft);
        assert_eq!(task.workspace, CONNECTION_WORKSPACE);
        let intake = store
            .intake_for_tasks(&[task.id])
            .unwrap()
            .remove(&task.id)
            .unwrap();
        assert!(!intake.repository_named);
        assert_eq!(intake.recommended_worker.unwrap().name, "Web");
        assert_eq!(intake.source, source);

        store.transition_task(task.id, TaskState::Ready).unwrap();
        let assigned = store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        assert_eq!(assigned.workspace, "/workspace/web");
    }

    #[test]
    fn a_named_repository_is_kept_and_a_removed_recommendation_is_dropped() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker("Web", ProviderKind::ClaudeCode, "/workspace/web", false, 1)
            .unwrap();
        let tool = store.connection_principal("client-a", "Claude").unwrap();
        let task = store
            .file_intake_task_as(
                &filing(
                    "Fix it",
                    Some(" /workspace/api "),
                    Some(worker.id),
                    &IntakeSource::default(),
                ),
                &TaskActivityActor::worker(tool.id),
            )
            .unwrap();
        assert_eq!(task.workspace, "/workspace/api");

        store.archive_worker_profile(worker.id).unwrap();
        let intake = store
            .intake_for_tasks(&[task.id])
            .unwrap()
            .remove(&task.id)
            .unwrap();
        assert!(intake.repository_named);
        assert_eq!(intake.recommended_worker, None);
    }

    /// A filer's list is what IT filed: not another tool's, not a worker's and
    /// not the operator's.
    #[test]
    fn filed_tasks_are_the_filers_own() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker("Web", ProviderKind::ClaudeCode, "/workspace/web", false, 1)
            .unwrap();
        let tool = store.connection_principal("client-a", "Claude").unwrap();
        let other = store.connection_principal("client-b", "Other").unwrap();
        let none = IntakeSource::default();
        let mine = store
            .file_intake_task_as(
                &filing("Mine", None, None, &none),
                &TaskActivityActor::worker(tool.id),
            )
            .unwrap();
        store
            .file_intake_task_as(
                &filing("Theirs", None, None, &none),
                &TaskActivityActor::worker(other.id),
            )
            .unwrap();
        store
            .create_task_with_details_as(
                "A worker's",
                "",
                TaskPriority::Normal,
                "/workspace/web",
                &TaskActivityActor::worker(worker.id),
            )
            .unwrap();
        store
            .create_task("The operator's", "/workspace/web")
            .unwrap();

        let filed = store.list_tasks_filed_by(tool.id).unwrap();
        assert_eq!(
            filed.iter().map(|task| task.id).collect::<Vec<_>>(),
            [mine.id]
        );
    }

    #[test]
    fn open_titles_leave_out_closed_work_and_say_which_the_reader_filed() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let tool = store.connection_principal("client-a", "Claude").unwrap();
        let keyed = IntakeSource::normalized(None, Some("ABC-12"), None).unwrap();
        let mine = store
            .file_intake_task_as(
                &filing("Mine", None, None, &keyed),
                &TaskActivityActor::worker(tool.id),
            )
            .unwrap();
        let open = store.create_task("Open", "/workspace/web").unwrap();
        let closed = store.create_task("Closed", "/workspace/web").unwrap();
        store
            .transition_task(closed.id, TaskState::Abandoned)
            .unwrap();

        let titles = store.open_task_titles(tool.id).unwrap();
        let ids = titles.iter().map(|title| title.id).collect::<Vec<_>>();
        assert!(ids.contains(&mine.id) && ids.contains(&open.id));
        assert!(!ids.contains(&closed.id));
        let found = titles.iter().find(|title| title.id == mine.id).unwrap();
        assert!(found.filed_by_reader);
        assert_eq!(found.jira_key.as_deref(), Some("ABC-12"));
        assert!(
            !titles
                .iter()
                .find(|title| title.id == open.id)
                .unwrap()
                .filed_by_reader
        );
    }
}
