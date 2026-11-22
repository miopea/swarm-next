//! Read links (ADR 0182): another task a task's worker may read.
//!
//! Only Queen or the operator adds or removes one, and it grants a read and
//! nothing else: no write, no transition, no message, no place on the board.

use std::collections::HashMap;
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use swarm_domain::{
    ControlRoomEventKind, MAX_HIVE_READ_LINKS, MAX_TASK_READ_LINKS, ReadLinkOperation,
    TaskActivityActor, TaskId, TaskPrerequisiteError, TaskReadLink, TaskReadLinkChange,
    TaskReadLinkError, TaskState, WorkerId, validate_task_read_link,
};

use crate::{TaskStore, TaskStoreError, insert_control_room_event, parse_domain_id};

pub(crate) fn migrate_task_read_links(
    transaction: &Transaction<'_>,
    version: i64,
) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_read_links (
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            readable_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 2048),
            actor_kind TEXT NOT NULL CHECK(actor_kind IN ('operator','worker')),
            actor_id TEXT,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(task_id, readable_task_id),
            CHECK(task_id != readable_task_id)
         );
         CREATE INDEX IF NOT EXISTS task_read_links_by_readable
         ON task_read_links(readable_task_id, task_id);",
    )?;
    transaction.pragma_update(None, "user_version", version)
}

/// Queen or the operator, and nobody else. The prerequisite rule's own form
/// without its one exception: a worker may name what its own task waits on,
/// but it may not open another task to itself.
fn authorize(tx: &Transaction<'_>, actor: &TaskActivityActor) -> Result<(), TaskStoreError> {
    crate::task_prerequisites::authorize(tx, actor).map_err(|error| match error {
        TaskStoreError::TaskPrerequisite(TaskPrerequisiteError::Unauthorized) => {
            TaskReadLinkError::Unauthorized.into()
        }
        other => other,
    })
}

fn local_task(connection: &Connection, id: TaskId) -> Result<(), TaskStoreError> {
    connection
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1 AND removed_at IS NULL
             AND hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)",
            [id.to_string()],
            |_| Ok(()),
        )
        .optional()?
        .ok_or(TaskStoreError::NotFound)
}

fn audit(
    tx: &Transaction<'_>,
    change: &TaskReadLinkChange,
    action: &str,
    actor: &TaskActivityActor,
    now: i64,
) -> Result<(), TaskStoreError> {
    tx.execute(
        "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
         VALUES (?1, 'noted', ?2, ?3, ?4)",
        params![
            change.task_id.to_string(),
            format!(
                "Read link {action}: {}. {}",
                change.readable_task_id,
                change.reason.trim()
            ),
            actor.kind.to_string(),
            actor.id
        ],
    )?;
    tx.execute(
        "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
        params![change.task_id.to_string(), now],
    )?;
    insert_control_room_event(tx, ControlRoomEventKind::TasksChanged)?;
    Ok(())
}

fn read_links(
    connection: &Connection,
    tasks: &[TaskId],
) -> Result<HashMap<TaskId, Vec<TaskReadLink>>, TaskStoreError> {
    let mut found: HashMap<TaskId, Vec<TaskReadLink>> = HashMap::new();
    if tasks.is_empty() {
        return Ok(found);
    }
    let placeholders = vec!["?"; tasks.len()].join(",");
    let mut statement = connection.prepare(&format!(
        "SELECT link.task_id, link.readable_task_id, readable.title, readable.state,
                readable.workspace, link.reason, link.created_at
         FROM task_read_links link JOIN tasks readable ON readable.id = link.readable_task_id
         WHERE link.task_id IN ({placeholders}) AND readable.removed_at IS NULL
         ORDER BY link.task_id, link.created_at, link.readable_task_id"
    ))?;
    let bound = tasks.iter().map(ToString::to_string).collect::<Vec<_>>();
    let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), |row| {
        Ok((
            parse_domain_id::<TaskId>(&row.get::<_, String>(0)?)?,
            TaskReadLink {
                readable_task_id: parse_domain_id(&row.get::<_, String>(1)?)?,
                title: row.get(2)?,
                state: TaskState::from_str(&row.get::<_, String>(3)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                workspace: row.get(4)?,
                reason: row.get(5)?,
                created_at: row.get(6)?,
            },
        ))
    })?;
    for row in rows {
        let (task, link) = row?;
        let links = found.entry(task).or_default();
        if links.len() >= MAX_TASK_READ_LINKS {
            return Err(TaskReadLinkError::Capacity.into());
        }
        links.push(link);
    }
    Ok(found)
}

impl TaskStore {
    /// Add or remove one read link, and return the task's links after it.
    ///
    /// Adding the same link with the same reason again changes nothing.
    /// Removing a link that is not there changes nothing either.
    ///
    /// # Errors
    /// Refuses anyone but Queen or the operator, a missing or foreign task, an
    /// invalid reason, a task linked to itself, a changed reason, and a link
    /// past either bound.
    pub fn change_task_read_link(
        &self,
        change: &TaskReadLinkChange,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<Vec<TaskReadLink>, TaskStoreError> {
        validate_task_read_link(change)?;
        let reason = change.reason.trim();
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        authorize(&tx, actor)?;
        local_task(&tx, change.task_id)?;
        let (task, readable) = (
            change.task_id.to_string(),
            change.readable_task_id.to_string(),
        );
        match change.operation {
            ReadLinkOperation::Add => {
                local_task(&tx, change.readable_task_id)?;
                let existing: Option<String> = tx
                    .query_row(
                        "SELECT reason FROM task_read_links
                         WHERE task_id = ?1 AND readable_task_id = ?2",
                        params![task, readable],
                        |row| row.get(0),
                    )
                    .optional()?;
                match existing {
                    Some(existing) if existing == reason => {}
                    Some(_) => return Err(TaskReadLinkError::Conflict.into()),
                    None => {
                        let (on_task, in_hive): (usize, usize) = tx.query_row(
                            "SELECT (SELECT count(*) FROM task_read_links WHERE task_id = ?1),
                                    (SELECT count(*) FROM task_read_links)",
                            [&task],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )?;
                        if on_task >= MAX_TASK_READ_LINKS || in_hive >= MAX_HIVE_READ_LINKS {
                            return Err(TaskReadLinkError::Capacity.into());
                        }
                        tx.execute(
                            "INSERT INTO task_read_links
                             (task_id, readable_task_id, reason, actor_kind, actor_id, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![
                                task,
                                readable,
                                reason,
                                actor.kind.to_string(),
                                actor.id,
                                now
                            ],
                        )?;
                        audit(&tx, change, "added", actor, now)?;
                    }
                }
            }
            ReadLinkOperation::Remove => {
                let removed = tx.execute(
                    "DELETE FROM task_read_links WHERE task_id = ?1 AND readable_task_id = ?2",
                    params![task, readable],
                )?;
                if removed > 0 {
                    audit(&tx, change, "removed", actor, now)?;
                }
            }
        }
        let links = read_links(&tx, &[change.task_id])?
            .remove(&change.task_id)
            .unwrap_or_default();
        tx.commit()?;
        Ok(links)
    }

    /// Whether this worker holds, or finished, a task with a read link to
    /// `task`. A read link is the only way it opens another task this way, and
    /// only Queen or the operator sets one.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn worker_reads_linked_task(
        &self,
        worker: WorkerId,
        task: TaskId,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_read_links link
                 JOIN tasks holder ON holder.id = link.task_id
                 JOIN tasks readable ON readable.id = link.readable_task_id
                 WHERE link.readable_task_id = ?1
                   AND holder.assigned_worker_id = ?2
                   AND holder.removed_at IS NULL
                   AND readable.removed_at IS NULL)",
            params![task.to_string(), worker.to_string()],
            |row| row.get(0),
        )?)
    }

    /// The read links on each of these tasks, for listing them with the task.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or a stored row does
    /// not read.
    pub fn read_links_for_tasks(
        &self,
        tasks: &[TaskId],
    ) -> Result<HashMap<TaskId, Vec<TaskReadLink>>, TaskStoreError> {
        let connection = self.connection()?;
        read_links(&connection, tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::ProviderKind;

    fn add(task: TaskId, readable: TaskId, reason: &str) -> TaskReadLinkChange {
        TaskReadLinkChange {
            task_id: task,
            readable_task_id: readable,
            operation: ReadLinkOperation::Add,
            reason: reason.into(),
        }
    }

    fn remove(task: TaskId, readable: TaskId) -> TaskReadLinkChange {
        TaskReadLinkChange {
            operation: ReadLinkOperation::Remove,
            ..add(task, readable, "No longer needed.")
        }
    }

    struct Fixture {
        store: TaskStore,
        queen: TaskActivityActor,
        worker: WorkerId,
        mine: TaskId,
        theirs: TaskId,
    }

    fn fixture() -> Fixture {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/fixture/queen").unwrap();
        let worker = store
            .create_worker("Platform", ProviderKind::ClaudeCode, "/platform", false, 1)
            .unwrap();
        let mine = store.create_task("Build the rules", "/platform").unwrap();
        let theirs = store.create_task("Holds the rules", "/services").unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET assigned_worker_id = ?2 WHERE id = ?1",
                params![mine.id.to_string(), worker.id.to_string()],
            )
            .unwrap();
        Fixture {
            store,
            queen: TaskActivityActor::worker(queen.id),
            worker: worker.id,
            mine: mine.id,
            theirs: theirs.id,
        }
    }

    #[test]
    fn queen_links_a_task_and_its_worker_may_read_it() {
        let hive = fixture();
        assert!(
            !hive
                .store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
        let links = hive
            .store
            .change_task_read_link(
                &add(hive.mine, hive.theirs, "Holds the rules this follows."),
                &hive.queen,
                100,
            )
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].readable_task_id, hive.theirs);
        assert_eq!(links[0].title, "Holds the rules");
        assert_eq!(links[0].workspace, "/services");
        assert!(
            hive.store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
    }

    #[test]
    fn the_operator_may_link_a_task_too() {
        let hive = fixture();
        hive.store
            .change_task_read_link(
                &add(hive.mine, hive.theirs, "Why."),
                &TaskActivityActor::operator(),
                100,
            )
            .unwrap();
        assert!(
            hive.store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
    }

    #[test]
    fn a_worker_may_not_link_a_task_even_on_its_own_task() {
        let hive = fixture();
        let refused = hive.store.change_task_read_link(
            &add(hive.mine, hive.theirs, "I want to read it."),
            &TaskActivityActor::worker(hive.worker),
            100,
        );
        assert!(matches!(
            refused,
            Err(TaskStoreError::TaskReadLink(
                TaskReadLinkError::Unauthorized
            ))
        ));
        assert!(
            !hive
                .store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
    }

    #[test]
    fn a_link_opens_only_the_linked_task_and_only_to_its_holder() {
        let hive = fixture();
        let other = hive.store.create_task("Unrelated", "/elsewhere").unwrap();
        let stranger = hive
            .store
            .create_worker("Other", ProviderKind::ClaudeCode, "/other", false, 2)
            .unwrap();
        hive.store
            .change_task_read_link(&add(hive.mine, hive.theirs, "Why."), &hive.queen, 100)
            .unwrap();
        assert!(
            !hive
                .store
                .worker_reads_linked_task(hive.worker, other.id)
                .unwrap()
        );
        assert!(
            !hive
                .store
                .worker_reads_linked_task(stranger.id, hive.theirs)
                .unwrap()
        );
    }

    #[test]
    fn removing_the_link_closes_the_read_and_both_go_on_the_history() {
        let hive = fixture();
        hive.store
            .change_task_read_link(&add(hive.mine, hive.theirs, "Why."), &hive.queen, 100)
            .unwrap();
        let links = hive
            .store
            .change_task_read_link(&remove(hive.mine, hive.theirs), &hive.queen, 200)
            .unwrap();
        assert!(links.is_empty());
        assert!(
            !hive
                .store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
        let notes: Vec<String> = hive
            .store
            .connection()
            .unwrap()
            .prepare("SELECT note FROM task_activity WHERE task_id = ?1 AND note LIKE 'Read link %' ORDER BY sequence")
            .unwrap()
            .query_map([hive.mine.to_string()], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with(&format!("Read link added: {}. Why.", hive.theirs)));
        assert!(notes[1].starts_with("Read link removed:"));
    }

    #[test]
    fn the_same_link_again_changes_nothing_and_a_new_reason_is_refused() {
        let hive = fixture();
        let link = add(hive.mine, hive.theirs, "Why.");
        hive.store
            .change_task_read_link(&link, &hive.queen, 100)
            .unwrap();
        hive.store
            .change_task_read_link(&link, &hive.queen, 200)
            .unwrap();
        let added: i64 = hive
            .store
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM task_activity WHERE note LIKE 'Read link added:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(added, 1);
        assert!(matches!(
            hive.store.change_task_read_link(
                &add(hive.mine, hive.theirs, "Another reason."),
                &hive.queen,
                300
            ),
            Err(TaskStoreError::TaskReadLink(TaskReadLinkError::Conflict))
        ));
    }

    #[test]
    fn a_task_holds_at_most_its_bound_of_links() {
        let hive = fixture();
        for index in 0..MAX_TASK_READ_LINKS {
            let readable = hive
                .store
                .create_task(&format!("Readable {index}"), "/services")
                .unwrap();
            hive.store
                .change_task_read_link(&add(hive.mine, readable.id, "Why."), &hive.queen, 100)
                .unwrap();
        }
        let one_more = hive.store.create_task("One more", "/services").unwrap();
        assert!(matches!(
            hive.store.change_task_read_link(
                &add(hive.mine, one_more.id, "Why."),
                &hive.queen,
                100
            ),
            Err(TaskStoreError::TaskReadLink(TaskReadLinkError::Capacity))
        ));
    }

    #[test]
    fn a_removed_task_is_neither_linked_nor_read() {
        let hive = fixture();
        hive.store
            .change_task_read_link(&add(hive.mine, hive.theirs, "Why."), &hive.queen, 100)
            .unwrap();
        hive.store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET removed_at = 150 WHERE id = ?1",
                [hive.theirs.to_string()],
            )
            .unwrap();
        assert!(
            !hive
                .store
                .worker_reads_linked_task(hive.worker, hive.theirs)
                .unwrap()
        );
        assert!(
            hive.store
                .read_links_for_tasks(&[hive.mine])
                .unwrap()
                .is_empty()
        );
        let gone = hive.store.create_task("Gone", "/services").unwrap();
        hive.store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET removed_at = 150 WHERE id = ?1",
                [gone.id.to_string()],
            )
            .unwrap();
        assert!(matches!(
            hive.store
                .change_task_read_link(&add(hive.mine, gone.id, "Why."), &hive.queen, 200),
            Err(TaskStoreError::NotFound)
        ));
    }

    /// The real cases: the rules sit on a finished task, and the worker keeps
    /// the read after its own task is finished too.
    #[test]
    fn a_closed_task_is_read_and_a_finished_holder_keeps_the_read() {
        let hive = fixture();
        hive.store
            .change_task_read_link(&add(hive.mine, hive.theirs, "Why."), &hive.queen, 100)
            .unwrap();
        for task in [hive.theirs, hive.mine] {
            hive.store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE tasks SET state = 'completed' WHERE id = ?1",
                    [task.to_string()],
                )
                .unwrap();
            assert!(
                hive.store
                    .worker_reads_linked_task(hive.worker, hive.theirs)
                    .unwrap()
            );
        }
    }

    #[test]
    fn links_are_listed_per_task() {
        let hive = fixture();
        let second = hive.store.create_task("Second", "/services").unwrap();
        hive.store
            .change_task_read_link(&add(hive.mine, hive.theirs, "First."), &hive.queen, 100)
            .unwrap();
        hive.store
            .change_task_read_link(&add(hive.mine, second.id, "Second."), &hive.queen, 200)
            .unwrap();
        let listed = hive
            .store
            .read_links_for_tasks(&[hive.mine, hive.theirs])
            .unwrap();
        let mine = &listed[&hive.mine];
        assert_eq!(
            mine.iter()
                .map(|link| link.readable_task_id)
                .collect::<Vec<_>>(),
            vec![hive.theirs, second.id]
        );
        assert!(!listed.contains_key(&hive.theirs));
    }
}
