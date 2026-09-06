use rusqlite::{OptionalExtension, Transaction, params};
use swarm_domain::{ControlRoomEventKind, Task, TaskActivityActor, TaskBlockReassessment};

use crate::{TaskStore, TaskStoreError, insert_control_room_event};

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_block_reassessments (
        task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        block_entry_sequence INTEGER NOT NULL REFERENCES task_activity(sequence),
        expected_activity_sequence INTEGER NOT NULL,
        recorded_sequence INTEGER NOT NULL REFERENCES task_activity(sequence),
        reason TEXT NOT NULL,
        evidence TEXT NOT NULL,
        not_before INTEGER
    );",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::TASK_BLOCK_REASSESSMENT_SCHEMA_VERSION,
    )
}

impl TaskStore {
    /// Correct a current block in place, preserving assignment and transport.
    ///
    /// # Errors
    /// Refuses non-Queen/operator callers, changed state/history and invalid input.
    pub fn reassess_task_block(
        &self,
        input: &TaskBlockReassessment,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<Task, TaskStoreError> {
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        crate::task_prerequisites::authorize(&tx, actor)?;
        let id = input.task_id.to_string();
        let state: String = tx
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL
             AND hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)",
                [&id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(TaskStoreError::NotFound)?;
        if state != "blocked" {
            return Err(TaskStoreError::IntegrityFailure(
                "task is no longer Blocked; reread before reassessing".into(),
            ));
        }
        let latest: i64 = tx.query_row(
            "SELECT coalesce(max(sequence), 0) FROM task_activity WHERE task_id = ?1",
            [&id],
            |row| row.get(0),
        )?;
        let same_retry: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_block_reassessments WHERE task_id = ?1
             AND expected_activity_sequence = ?2 AND recorded_sequence = ?3
             AND reason = ?4 AND evidence = ?5 AND not_before IS ?6)",
            params![
                id,
                input.expected_activity_sequence,
                latest,
                input.reason,
                input.evidence,
                input.not_before
            ],
            |row| row.get(0),
        )?;
        if !same_retry {
            input
                .validate(now)
                .map_err(|message| TaskStoreError::IntegrityFailure(message.into()))?;
            if latest != input.expected_activity_sequence {
                return Err(TaskStoreError::IntegrityFailure(
                    "task history changed; reread before reassessing".into(),
                ));
            }
            let entry: i64 = tx.query_row("SELECT sequence FROM task_activity WHERE task_id = ?1 AND kind = 'state_changed' AND to_state = 'blocked' ORDER BY sequence DESC LIMIT 1", [&id], |row| row.get(0))?;
            tx.execute(
                "INSERT INTO task_activity (task_id, kind, to_state, note, actor_kind, actor_id, occurred_at)
                 VALUES (?1, 'corrected', 'blocked', ?2, ?3, ?4, ?5)",
                params![id, format!("Block reassessed: {}\nEvidence: {}\nNot before: {:?}", input.reason, input.evidence, input.not_before), actor.kind.to_string(), actor.id, now],
            )?;
            let recorded = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO task_block_reassessments (task_id, block_entry_sequence, expected_activity_sequence, recorded_sequence, reason, evidence, not_before)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(task_id) DO UPDATE SET block_entry_sequence=excluded.block_entry_sequence,
                   expected_activity_sequence=excluded.expected_activity_sequence, recorded_sequence=excluded.recorded_sequence,
                   reason=excluded.reason, evidence=excluded.evidence, not_before=excluded.not_before",
                params![id, entry, input.expected_activity_sequence, recorded, input.reason, input.evidence, input.not_before],
            )?;
            tx.execute(
                "UPDATE tasks SET blocked_until = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, input.not_before, now],
            )?;
            insert_control_room_event(&tx, ControlRoomEventKind::TasksChanged)?;
        }
        tx.commit()?;
        drop(connection);
        self.get_task(input.task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{NextMoveOwner, TaskActivityKind, TaskState};

    fn block(store: &TaskStore) -> TaskBlockReassessment {
        let task = store
            .create_task("Fictional external window", "/workspace/demo")
            .unwrap();
        for state in [TaskState::Ready, TaskState::Active] {
            store.transition_task(task.id, state).unwrap();
        }
        store
            .transition_task_with_note(task.id, TaskState::Blocked, "Original long blocker")
            .unwrap();
        TaskBlockReassessment {
            task_id: task.id,
            expected_activity_sequence: store.list_task_activity(task.id, 1).unwrap().events[0]
                .sequence,
            reason: "Waiting for the verified external window".into(),
            evidence: "Fictional schedule was read at source".into(),
            not_before: Some(4_000_000_000),
        }
    }

    #[test]
    fn reassessment_preserves_state_and_original_history_and_replays_once() {
        let store = TaskStore::in_memory().unwrap();
        let input = block(&store);
        let actor = TaskActivityActor::operator();
        let before = store.list_task_activity(input.task_id, 100).unwrap().events;
        let result = store.reassess_task_block(&input, &actor, 100).unwrap();
        assert_eq!(result.state, TaskState::Blocked);
        assert_eq!(result.blocked_note.as_deref(), Some(input.reason.as_str()));
        assert_eq!(result.blocked_until, input.not_before);
        assert_eq!(result.next_move_owner, NextMoveOwner::Blocked);
        // A lost receipt is still recoverable after the original date passes.
        store
            .reassess_task_block(&input, &actor, 4_000_000_001)
            .unwrap();
        let after = store.list_task_activity(input.task_id, 100).unwrap().events;
        assert_eq!(after.len(), before.len() + 1);
        assert!(
            after
                .iter()
                .any(|event| event.note == "Original long blocker")
        );
        assert!(
            after
                .iter()
                .any(|event| event.kind == TaskActivityKind::Corrected)
        );
        let mut changed = input.clone();
        changed.reason = "Different".into();
        assert!(store.reassess_task_block(&changed, &actor, 101).is_err());
    }

    #[test]
    fn a_new_block_does_not_reuse_an_old_reassessment() {
        let store = TaskStore::in_memory().unwrap();
        let input = block(&store);
        store
            .reassess_task_block(&input, &TaskActivityActor::operator(), 100)
            .unwrap();
        store
            .transition_task(input.task_id, TaskState::Ready)
            .unwrap();
        store
            .transition_task(input.task_id, TaskState::Active)
            .unwrap();
        let task = store
            .transition_task_with_note(
                input.task_id,
                TaskState::Blocked,
                "New independent obstacle",
            )
            .unwrap();
        assert_eq!(
            task.blocked_note.as_deref(),
            Some("New independent obstacle")
        );
        assert_eq!(task.blocked_until, None);
        assert_eq!(task.next_move_owner, NextMoveOwner::Queen);
        assert!(
            store
                .reassess_task_block(&input, &TaskActivityActor::operator(), 101)
                .is_err()
        );
    }

    #[test]
    fn ordinary_worker_and_stale_history_cannot_reassess() {
        let store = TaskStore::in_memory().unwrap();
        let input = block(&store);
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/demo",
                false,
                1,
            )
            .unwrap();
        assert!(
            store
                .reassess_task_block(&input, &TaskActivityActor::worker(worker.id), 100)
                .is_err()
        );
        store
            .append_task_correction(
                input.task_id,
                "New facts arrived",
                &TaskActivityActor::operator(),
            )
            .unwrap();
        assert!(
            store
                .reassess_task_block(&input, &TaskActivityActor::operator(), 101)
                .is_err()
        );
        let task = store.get_task(input.task_id).unwrap();
        assert_eq!(task.blocked_note.as_deref(), Some("Original long blocker"));
        assert_eq!(task.blocked_until, None);
    }

    #[test]
    fn competing_reassessments_cannot_overwrite_each_other() {
        let store = TaskStore::in_memory().unwrap();
        let input = block(&store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["Verified reason A", "Verified reason B"]
            .into_iter()
            .map(|reason| {
                let store = store.clone();
                let barrier = barrier.clone();
                let mut input = input.clone();
                input.reason = reason.into();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.reassess_task_block(&input, &TaskActivityActor::operator(), 100)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    }

    #[test]
    fn invalid_hold_is_atomic_and_clearing_hold_does_not_resume() {
        let store = TaskStore::in_memory().unwrap();
        let mut input = block(&store);
        let before = store
            .list_task_activity(input.task_id, 100)
            .unwrap()
            .events
            .len();
        input.not_before = Some(100);
        assert!(
            store
                .reassess_task_block(&input, &TaskActivityActor::operator(), 100)
                .is_err()
        );
        assert_eq!(
            store
                .list_task_activity(input.task_id, 100)
                .unwrap()
                .events
                .len(),
            before
        );
        input.not_before = Some(4_000_000_000);
        store
            .reassess_task_block(&input, &TaskActivityActor::operator(), 100)
            .unwrap();
        input.expected_activity_sequence =
            store.list_task_activity(input.task_id, 1).unwrap().events[0].sequence;
        input.not_before = None;
        let task = store
            .reassess_task_block(&input, &TaskActivityActor::operator(), 101)
            .unwrap();
        assert_eq!(task.state, TaskState::Blocked);
        assert_eq!(task.next_move_owner, NextMoveOwner::Queen);
        assert_eq!(task.blocked_until, None);
    }

    #[test]
    fn migration_from_previous_schema_preserves_existing_block() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hive.db");
        let store = TaskStore::open(&path).unwrap();
        let input = block(&store);
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TABLE task_block_reassessments; PRAGMA user_version=140;")
            .unwrap();
        drop(store);
        let reopened = TaskStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .get_task(input.task_id)
                .unwrap()
                .blocked_note
                .as_deref(),
            Some("Original long blocker")
        );
        reopened
            .reassess_task_block(&input, &TaskActivityActor::operator(), 100)
            .unwrap();
        drop(reopened);
        assert_eq!(
            TaskStore::open(&path)
                .unwrap()
                .get_task(input.task_id)
                .unwrap()
                .blocked_note
                .as_deref(),
            Some(input.reason.as_str())
        );
    }
}
