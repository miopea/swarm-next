use crate::{OPS_TICKETS_SCHEMA_VERSION, TaskStore, TaskStoreError, insert_control_room_event};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use swarm_domain::{AuthorizedOpsTicket, ControlRoomEventKind, OpsIntegrationScope, Task, TaskId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpsTicketReceipt {
    pub task_id: TaskId,
    pub replayed: bool,
}

pub(crate) fn migrate_ops_tickets(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS ops_console_tickets (
            integration_id TEXT NOT NULL,
            app_id TEXT NOT NULL,
            request_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            command_digest BLOB NOT NULL CHECK(length(command_digest) = 32),
            workspace TEXT NOT NULL,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (integration_id, app_id, request_id)
         );
         CREATE INDEX IF NOT EXISTS ops_console_tickets_task ON ops_console_tickets(task_id);",
    )?;
    transaction.pragma_update(None, "user_version", OPS_TICKETS_SCHEMA_VERSION)
}

impl TaskStore {
    /// Atomically accepts an authorized external request as one inert draft.
    /// Identical retries survive lost responses and restarts; changed retries fail.
    ///
    /// # Errors
    /// Returns a conflict for a changed command under the same source key, or a
    /// persistence error. A failed transaction leaves neither a task nor a key.
    pub fn submit_ops_ticket(
        &self,
        command: &AuthorizedOpsTicket,
    ) -> Result<OpsTicketReceipt, TaskStoreError> {
        let input = command.input();
        // The normalized command is immutable after domain authorization. JSON
        // struct field order is deterministic, unlike a caller's object ordering.
        let serialized = serde_json::to_vec(command)
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let digest = Sha256::digest(&serialized);
        let hive_id = self.local_hive_identity()?.hive.id;
        let mut connection = self.connection()?;
        // IMMEDIATE serializes separate store connections before checking the key.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, Vec<u8>)> = transaction
            .query_row(
                "SELECT task_id, command_digest FROM ops_console_tickets
             WHERE integration_id = ?1 AND app_id = ?2 AND request_id = ?3",
                params![command.integration_id(), input.app_id, input.request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((task_id, stored_digest)) = existing {
            if stored_digest.as_slice() != digest.as_slice() {
                return Err(TaskStoreError::OpsTicketConflict);
            }
            let task_id = task_id
                .parse()
                .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
            return Ok(OpsTicketReceipt {
                task_id,
                replayed: true,
            });
        }
        let task_id = TaskId::new();
        transaction.execute(
            "INSERT INTO tasks (id, hive_id, title, description, priority, workspace, state, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'draft',
                     COALESCE((SELECT MAX(position) + 1 FROM tasks WHERE hive_id = ?2), 0))",
            params![task_id.to_string(), hive_id.to_string(), input.title,
                input.description, input.priority.to_string(), command.workspace()],
        )?;
        transaction.execute(
            "INSERT INTO ops_console_tickets
             (integration_id, app_id, request_id, conversation_id, command_digest, workspace, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![command.integration_id(), input.app_id, input.request_id,
                input.conversation_id, digest.as_slice(), command.workspace(), task_id.to_string()],
        )?;
        // System attribution retains a dedicated integration identity, without
        // inventing a worker or rewriting the existing activity CHECK constraint.
        transaction.execute(
            "INSERT INTO task_activity (task_id, kind, to_state, actor_kind, actor_id)
             VALUES (?1, 'created', 'draft', 'system', ?2)",
            params![
                task_id.to_string(),
                format!("ops-console:{}", command.integration_id())
            ],
        )?;
        insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        transaction.commit()?;
        Ok(OpsTicketReceipt {
            task_id,
            replayed: false,
        })
    }

    /// Reads only a task originating from this integration and app. Removed tasks
    /// retain their external key so a retry cannot recreate the work.
    ///
    /// # Errors
    /// Returns `NotFound` for out-of-scope, unknown, removed or remapped tickets.
    pub fn ops_ticket_task(
        &self,
        scope: &OpsIntegrationScope,
        app_id: &str,
        request_id: &str,
    ) -> Result<Task, TaskStoreError> {
        let workspace = scope
            .workspace_for(app_id)
            .map_err(|_| TaskStoreError::NotFound)?;
        let task_id: String = self
            .connection()?
            .query_row(
                "SELECT task_id FROM ops_console_tickets
             WHERE integration_id = ?1 AND app_id = ?2 AND request_id = ?3 AND workspace = ?4",
                params![scope.integration_id, app_id, request_id, workspace],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        let task_id = task_id
            .parse()
            .map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))?;
        let task = self.get_task(task_id)?;
        // A later operator move must not grant access to another workspace.
        if task.workspace != workspace {
            return Err(TaskStoreError::NotFound);
        }
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{OpsAppBinding, OpsTicketInput, TaskActivityActor, TaskPriority, TaskState};
    fn scope() -> OpsIntegrationScope {
        OpsIntegrationScope {
            integration_id: "console".into(),
            bindings: vec![OpsAppBinding {
                app_id: "app-one".into(),
                workspace: "/work/one".into(),
            }],
        }
    }
    fn input() -> OpsTicketInput {
        OpsTicketInput {
            app_id: "app-one".into(),
            request_id: "request-one".into(),
            conversation_id: "feedback:1".into(),
            title: "Calendar export".into(),
            description: "Reviewed scope".into(),
            priority: TaskPriority::Normal,
        }
    }
    fn count(store: &TaskStore, table: &str) -> i64 {
        store
            .connection()
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn ops_ticket_retry_is_one_draft_with_its_own_provenance() {
        let store = TaskStore::in_memory().unwrap();
        let command = scope().authorize(input()).unwrap();
        let first = store.submit_ops_ticket(&command).unwrap();
        let replay = store.submit_ops_ticket(&command).unwrap();
        assert!(!first.replayed);
        assert!(replay.replayed);
        assert_eq!(first.task_id, replay.task_id);
        let task = store
            .ops_ticket_task(&scope(), "app-one", "request-one")
            .unwrap();
        assert_eq!(task.state, TaskState::Draft);
        assert_eq!(count(&store, "tasks"), 1);
        assert_eq!(count(&store, "ops_console_tickets"), 1);
        let history = store.list_task_activity(first.task_id, 10).unwrap();
        assert_eq!(history.events.len(), 1);
        assert_eq!(
            history.events[0].actor_id.as_deref(),
            Some("ops-console:console")
        );
        let mut changed = input();
        changed.description = "Different scope".into();
        assert!(matches!(
            store.submit_ops_ticket(&scope().authorize(changed).unwrap()),
            Err(TaskStoreError::OpsTicketConflict)
        ));
    }
    #[test]
    fn ops_ticket_survives_restart_and_removed_tasks_do_not_recreate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.db");
        let command = scope().authorize(input()).unwrap();
        let first = {
            let store = TaskStore::open(&path).unwrap();
            let receipt = store.submit_ops_ticket(&command).unwrap();
            store
                .remove_task_as(receipt.task_id, &TaskActivityActor::operator(), "")
                .unwrap();
            receipt
        };
        let store = TaskStore::open(&path).unwrap();
        let replay = store.submit_ops_ticket(&command).unwrap();
        assert_eq!(replay.task_id, first.task_id);
        assert!(replay.replayed);
        assert_eq!(count(&store, "tasks"), 1);
    }
    #[test]
    fn ops_ticket_parallel_connections_return_one_task() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("swarm.db");
        let first = TaskStore::open(&path).unwrap();
        let second = TaskStore::open(&path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .submit_ops_ticket(&scope().authorize(input()).unwrap())
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results[0].task_id, results[1].task_id);
        assert_eq!(results.iter().filter(|r| !r.replayed).count(), 1);
    }
    #[test]
    fn ops_ticket_failure_rolls_back_task_key_and_activity() {
        let store = TaskStore::in_memory().unwrap();
        let events_before = count(&store, "control_room_events");
        store.connection().unwrap().execute_batch("CREATE TRIGGER reject_ops_activity BEFORE INSERT ON task_activity BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
        let command = scope().authorize(input()).unwrap();
        assert!(store.submit_ops_ticket(&command).is_err());
        assert_eq!(count(&store, "tasks"), 0);
        assert_eq!(count(&store, "ops_console_tickets"), 0);
        assert_eq!(count(&store, "task_activity"), 0);
        assert_eq!(count(&store, "control_room_events"), events_before);
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_ops_activity")
            .unwrap();
        assert!(!store.submit_ops_ticket(&command).unwrap().replayed);
    }
    #[test]
    fn ops_ticket_reads_refuse_other_integrations_apps_and_mappings() {
        let store = TaskStore::in_memory().unwrap();
        store
            .submit_ops_ticket(&scope().authorize(input()).unwrap())
            .unwrap();
        let mut other = scope();
        other.integration_id = "another-console".into();
        assert!(matches!(
            store.ops_ticket_task(&other, "app-one", "request-one"),
            Err(TaskStoreError::NotFound)
        ));
        assert!(matches!(
            store.ops_ticket_task(&scope(), "app-two", "request-one"),
            Err(TaskStoreError::NotFound)
        ));
        let mut remapped = scope();
        remapped.bindings[0].workspace = "/work/two".into();
        assert!(matches!(
            store.ops_ticket_task(&remapped, "app-one", "request-one"),
            Err(TaskStoreError::NotFound)
        ));
    }
}
