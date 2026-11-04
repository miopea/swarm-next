use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use swarm_domain::{
    ControlRoomEventKind, MAX_HIVE_PREREQUISITES, MAX_TASK_PREREQUISITES, TaskActivityActor,
    TaskActivityActorKind, TaskId, TaskPrerequisite, TaskPrerequisiteError, TaskState, WorkerId,
    validate_prerequisite_reason, validate_task_prerequisite,
};

use crate::{TaskStore, TaskStoreError, insert_control_room_event, parse_domain_id};

pub(super) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_prerequisites (
            task_id TEXT NOT NULL REFERENCES tasks(id),
            prerequisite_id TEXT NOT NULL REFERENCES tasks(id),
            reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 2048),
            actor_kind TEXT NOT NULL CHECK(actor_kind IN ('operator','worker')),
            actor_id TEXT,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(task_id, prerequisite_id),
            CHECK(task_id != prerequisite_id)
         );
         CREATE INDEX IF NOT EXISTS task_prerequisites_by_target
         ON task_prerequisites(prerequisite_id, task_id);",
    )?;
    tx.pragma_update(
        None,
        "user_version",
        crate::TASK_PREREQUISITES_SCHEMA_VERSION,
    )
}

pub(super) fn authorize(
    tx: &Transaction<'_>,
    actor: &TaskActivityActor,
) -> Result<(), TaskStoreError> {
    authorize_for(tx, actor, None)
}

/// ⚠️ ADD AND REMOVE ARE NOT THE SAME PERMISSION, and collapsing them would be
/// a real hole. `naming_blocker_on` passes the task an assigned worker is
/// blocking, which lets that worker RECORD what its own work waits on — the
/// affordance the Blocked guard demanded and did not supply. It is never passed
/// on the remove path: a worker that could delete its own prerequisite could
/// walk itself out of the gate it was parked behind, which is worse than the
/// stall this fixes. Naming a blocker is safe; unnaming one is not.
pub(super) fn authorize_for(
    tx: &Transaction<'_>,
    actor: &TaskActivityActor,
    naming_blocker_on: Option<TaskId>,
) -> Result<(), TaskStoreError> {
    if actor.kind == TaskActivityActorKind::Operator {
        return Ok(());
    }
    if actor.kind == TaskActivityActorKind::Worker {
        let queen: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_profiles
             WHERE id = ?1 AND role = 'queen' AND archived_at IS NULL)",
            [actor.id.as_deref()],
            |row| row.get(0),
        )?;
        if queen {
            return Ok(());
        }
        if let Some(task) = naming_blocker_on {
            let owns: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks
                 WHERE id = ?1 AND assigned_worker_id = ?2)",
                params![task.to_string(), actor.id.as_deref()],
                |row| row.get(0),
            )?;
            if owns {
                return Ok(());
            }
        }
    }
    Err(TaskPrerequisiteError::Unauthorized.into())
}

fn local_task(tx: &Connection, id: TaskId) -> Result<TaskState, TaskStoreError> {
    let state: String = tx
        .query_row(
            "SELECT state FROM tasks WHERE id = ?1 AND removed_at IS NULL
         AND hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(TaskStoreError::NotFound)?;
    TaskState::from_str(&state).map_err(|_| TaskStoreError::Sql(rusqlite::Error::InvalidQuery))
}

fn audit(
    tx: &Transaction<'_>,
    task: TaskId,
    prerequisite: TaskId,
    reason: &str,
    action: &str,
    actor: &TaskActivityActor,
    now: i64,
) -> Result<(), TaskStoreError> {
    tx.execute(
        "INSERT INTO task_activity (task_id, kind, note, actor_kind, actor_id)
         VALUES (?1, 'noted', ?2, ?3, ?4)",
        params![
            task.to_string(),
            format!("Prerequisite {action}: {prerequisite}. {reason}"),
            actor.kind.to_string(),
            actor.id
        ],
    )?;
    tx.execute(
        "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
        params![task.to_string(), now],
    )?;
    insert_control_room_event(tx, ControlRoomEventKind::TasksChanged)?;
    Ok(())
}

impl TaskStore {
    /// Blocked work whose explicit prerequisites finished and whose next review is due.
    /// Returns at most 64 tasks and an explicit overflow indication. This is
    /// discovery for Queen, never authorization to resume or bypass other blocks.
    ///
    /// # Errors
    /// Returns an error when current dependency evidence cannot be read safely.
    pub fn tasks_ready_after_prerequisites(
        &self,
        now: i64,
    ) -> Result<(Vec<swarm_domain::Task>, bool), TaskStoreError> {
        self.blocked_review_candidates(now, true)
    }

    /// Bounded discovery of blocks without an outstanding structured hold.
    /// Includes unlinked blocks so missing dependency metadata cannot hide work
    /// from Queen. Absence of a hold is NOT proof that work is safe to resume.
    ///
    /// # Errors
    /// Returns an error if the current task/decision/dependency evidence fails.
    pub fn blocked_tasks_for_reassessment(
        &self,
        now: i64,
    ) -> Result<(Vec<swarm_domain::Task>, bool), TaskStoreError> {
        self.blocked_review_candidates(now, false)
    }

    fn blocked_review_candidates(
        &self,
        now: i64,
        require_prerequisite: bool,
    ) -> Result<(Vec<swarm_domain::Task>, bool), TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "{} WHERE t.state = 'blocked' AND t.removed_at IS NULL
             AND t.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)
             AND (t.blocked_until IS NULL OR t.blocked_until <= ?1)
             AND (?2 = 0 OR EXISTS(SELECT 1 FROM task_prerequisites p WHERE p.task_id = t.id))
             AND NOT EXISTS(SELECT 1 FROM task_prerequisites p
                 LEFT JOIN tasks upstream ON upstream.id = p.prerequisite_id
                 WHERE p.task_id = t.id AND (upstream.id IS NULL
                     OR upstream.removed_at IS NOT NULL OR upstream.state != 'completed'))
             AND NOT EXISTS(SELECT 1 FROM decision_requests d WHERE d.id IN
                 (SELECT decision_id FROM task_decision_membership WHERE task_id=t.id) AND d.state = 'pending')
             ORDER BY t.position, t.id LIMIT 65",
            Self::TASK_PROJECTION,
        );
        let mut statement = connection.prepare(&sql)?;
        let mut tasks = statement
            .query_map(params![now, require_prerequisite], crate::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let truncated = tasks.len() > 64;
        tasks.truncate(64);
        Ok((tasks, truncated))
    }

    /// Rechecks the immutable briefing claim before any terminal contact.
    ///
    /// # Errors
    /// Fails closed on unavailable or malformed persistence.
    pub fn task_briefing_can_submit(
        &self,
        delivery: &crate::TaskDispatch,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let current: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_dispatches d
             JOIN task_assignments a ON a.id = d.assignment_id AND a.released_at IS NULL
             JOIN tasks t ON t.id = d.task_id AND t.removed_at IS NULL
             JOIN worker_sessions s ON s.session_id = a.worker_session_id AND s.ended_at IS NULL
             WHERE d.assignment_id = ?1 AND d.generation = ?2 AND d.state = 'dispatching'
               AND t.id = ?3 AND t.state IN ('ready','active')
               AND t.assigned_worker_id = ?4 AND d.worker_id = ?4 AND s.worker_id = ?4
               AND s.session_id = ?5)",
            params![
                delivery.assignment_id,
                delivery.generation,
                delivery.task_id.to_string(),
                delivery.worker_id.to_string(),
                delivery.session_id.to_string()
            ],
            |row| row.get(0),
        )?;
        Ok(current
            && read_prerequisites(&connection, delivery.task_id)?
                .iter()
                .all(TaskPrerequisite::satisfied))
    }

    /// Recheck an automatic wake after lifecycle ownership is acquired.
    ///
    /// # Errors
    /// Fails closed on unavailable or malformed persistence.
    pub fn coordinator_wake_can_start(
        &self,
        wake: &crate::CoordinatorWorkerWake,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let current: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM coordinator_actions a JOIN tasks t ON t.id = a.task_id
             WHERE a.id = ?1 AND a.kind = 'wake_assigned_worker' AND a.state = 'running'
               AND a.task_id = ?2 AND a.worker_id = ?3 AND t.assigned_worker_id = ?3
               AND t.state = 'ready' AND t.removed_at IS NULL)",
            params![
                wake.action_id,
                wake.task_id.to_string(),
                wake.worker_id.to_string()
            ],
            |row| row.get(0),
        )?;
        Ok(current
            && read_prerequisites(&connection, wake.task_id)?
                .iter()
                .all(TaskPrerequisite::satisfied))
    }

    /// A pre-start hold attempted no process and must spend no recovery attempt.
    ///
    /// # Errors
    /// Reports unavailable persistence without clearing the owned action.
    pub fn defer_coordinator_worker_wake(
        &self,
        action_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.execute(
            "UPDATE coordinator_actions SET state = 'queued', attempts = 0, attempted_at = NULL, updated_at = ?2
             WHERE id = ?1 AND kind = 'wake_assigned_worker' AND state = 'running'",
            params![action_id, now],
        )? == 1)
    }

    /// Add one explicit prerequisite without rewriting lifecycle or assignment.
    ///
    /// # Errors
    /// Refuses unauthorized, missing/foreign tasks, invalid edges and capacity.
    pub fn add_task_prerequisite(
        &self,
        task: TaskId,
        prerequisite: TaskId,
        reason: &str,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        validate_prerequisite_reason(reason)?;
        let reason = reason.trim();
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        authorize_for(&tx, actor, Some(task))?;
        let state = local_task(&tx, task)?;
        local_task(&tx, prerequisite)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT reason FROM task_prerequisites WHERE task_id = ?1 AND prerequisite_id = ?2",
                params![task.to_string(), prerequisite.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            return if existing == reason {
                Ok(())
            } else {
                Err(TaskPrerequisiteError::Conflict.into())
            };
        }
        let edges = {
            let mut query = tx.prepare(
                "SELECT p.task_id, p.prerequisite_id FROM task_prerequisites p
                 JOIN tasks source ON source.id = p.task_id
                 WHERE source.hive_id = (SELECT hive_id FROM local_hive_identity WHERE singleton = 1)
                 LIMIT ?1",
            )?;
            query
                .query_map(
                    [i64::try_from(MAX_HIVE_PREREQUISITES + 1)
                        .map_err(|_| TaskPrerequisiteError::Capacity)?],
                    |row| {
                        Ok((
                            parse_domain_id::<TaskId>(&row.get::<_, String>(0)?)?,
                            parse_domain_id::<TaskId>(&row.get::<_, String>(1)?)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        validate_task_prerequisite(task, state, prerequisite, reason, &edges)?;
        tx.execute(
            "INSERT INTO task_prerequisites (task_id, prerequisite_id, reason, actor_kind, actor_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![task.to_string(), prerequisite.to_string(), reason, actor.kind.to_string(), actor.id, now],
        )?;
        audit(&tx, task, prerequisite, reason, "added", actor, now)?;
        crate::task_waits::type_untyped_block(&tx, task, actor)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove an obsolete prerequisite, preserving the explanation in history.
    ///
    /// # Errors
    /// Refuses unauthorized or invalid changes and unavailable persistence.
    pub fn remove_task_prerequisite(
        &self,
        task: TaskId,
        prerequisite: TaskId,
        reason: &str,
        actor: &TaskActivityActor,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        validate_prerequisite_reason(reason)?;
        let mut connection = self.connection()?;
        let tx = connection.transaction()?;
        authorize(&tx, actor)?;
        local_task(&tx, task)?;
        let changed = tx.execute(
            "DELETE FROM task_prerequisites WHERE task_id = ?1 AND prerequisite_id = ?2",
            params![task.to_string(), prerequisite.to_string()],
        )?;
        if changed > 0 {
            audit(
                &tx,
                task,
                prerequisite,
                reason.trim(),
                "removed",
                actor,
                now,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Whether this worker holds a task that WAITS ON the given one.
    ///
    /// ⚠️ THE CASE THIS EXISTS FOR. A blocked worker can see THAT it is blocked
    /// and by which task id — the prerequisite appears on its own task with a
    /// title and a state — and could not read the task it was waiting on. The
    /// board names the reason and then refuses to show it, which is the spirit
    /// of the prerequisite primitive being broken. Hit first-hand on
    /// 01a091a1-4b43, whose prerequisite had to be read out of the database
    /// directly because the tool refused.
    ///
    /// Deliberately narrow: only a prerequisite RECORDED on a task this worker
    /// is assigned, and only a read. It grants nothing about tasks the board has
    /// not already named to this worker as its own blocker.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn worker_waits_on_task(
        &self,
        worker: WorkerId,
        prerequisite: TaskId,
    ) -> Result<bool, TaskStoreError> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_prerequisites p
                 JOIN tasks t ON t.id = p.task_id
                 WHERE p.prerequisite_id = ?1
                   AND t.assigned_worker_id = ?2
                   AND t.removed_at IS NULL)",
            params![prerequisite.to_string(), worker.to_string()],
            |row| row.get(0),
        )?)
    }

    /// Read current prerequisite facts, including removed upstream work.
    ///
    /// # Errors
    /// Refuses missing local work, oversized stored data and unavailable persistence.
    pub fn task_prerequisites(
        &self,
        task: TaskId,
    ) -> Result<Vec<TaskPrerequisite>, TaskStoreError> {
        let connection = self.connection()?;
        local_task(&connection, task)?;
        read_prerequisites(&connection, task)
    }
}

pub(super) fn read_prerequisites(
    connection: &Connection,
    task: TaskId,
) -> Result<Vec<TaskPrerequisite>, TaskStoreError> {
    let mut query = connection.prepare(
        "SELECT p.prerequisite_id, t.title, t.state, t.assigned_worker_id,
                    t.removed_at IS NOT NULL, p.reason, p.created_at
             FROM task_prerequisites p LEFT JOIN tasks t ON t.id = p.prerequisite_id
             WHERE p.task_id = ?1 ORDER BY p.created_at, p.prerequisite_id LIMIT ?2",
    )?;
    let prerequisites = query
        .query_map(
            params![
                task.to_string(),
                i64::try_from(MAX_TASK_PREREQUISITES + 1)
                    .map_err(|_| TaskPrerequisiteError::Capacity)?
            ],
            |row| {
                Ok(TaskPrerequisite {
                    task_id: task,
                    prerequisite_id: parse_domain_id(&row.get::<_, String>(0)?)?,
                    title: row.get(1)?,
                    state: TaskState::from_str(&row.get::<_, String>(2)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    assigned_worker_id: row
                        .get::<_, Option<String>>(3)?
                        .map(|id| parse_domain_id(&id))
                        .transpose()?,
                    removed: row.get(4)?,
                    reason: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if prerequisites.len() > MAX_TASK_PREREQUISITES {
        return Err(TaskPrerequisiteError::Capacity.into());
    }
    Ok(prerequisites)
}

pub(super) fn ensure_satisfied(
    connection: &Connection,
    task: TaskId,
) -> Result<(), TaskStoreError> {
    if read_prerequisites(connection, task)?
        .iter()
        .any(|item| !item.satisfied())
    {
        return Err(TaskPrerequisiteError::Unresolved.into());
    }
    Ok(())
}

/// What a draft's wait allows it to become (ADR 0152).
///
/// Blocked needs something that can still finish: a prerequisite that is
/// neither finished, abandoned nor removed, or an operator decision still
/// pending. A block on nothing would park the draft out of Queen's sight.
/// Ready is refused while such a prerequisite is outstanding, and the refusal
/// names Blocked: that refusal used to be the only answer Queen got.
pub(super) fn ensure_draft_move_matches_its_wait(
    connection: &Connection,
    task: TaskId,
    target: TaskState,
) -> Result<(), TaskStoreError> {
    let outstanding = read_prerequisites(connection, task)?
        .iter()
        .any(TaskPrerequisite::outstanding);
    match target {
        TaskState::Blocked if !outstanding => {
            let pending_decision: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_decision_membership m
                     JOIN decision_requests d ON d.id = m.decision_id
                     WHERE m.task_id = ?1 AND d.state = 'pending')",
                [task.to_string()],
                |row| row.get(0),
            )?;
            if pending_decision {
                Ok(())
            } else {
                Err(TaskPrerequisiteError::DraftBlocksOnlyBehindUnfinishedWork.into())
            }
        }
        TaskState::Ready if outstanding => {
            Err(TaskPrerequisiteError::DraftWaitsOnUnfinishedWork.into())
        }
        _ => Ok(()),
    }
}

pub(super) fn from_projection(
    row: &rusqlite::Row<'_>,
    column: usize,
) -> rusqlite::Result<Vec<TaskPrerequisite>> {
    let prerequisites: Vec<TaskPrerequisite> = serde_json::from_str(&row.get::<_, String>(column)?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    if prerequisites.len() > MAX_TASK_PREREQUISITES {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(prerequisites)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(store: &TaskStore, title: &str) -> TaskId {
        let task = store.create_task(title, "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store.transition_task(task.id, TaskState::Blocked).unwrap();
        task.id
    }

    /// A draft routed to a worker, as Queen leaves one she has assigned.
    fn routed_draft(store: &TaskStore) -> (TaskId, swarm_domain::WorkerProfile) {
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let draft = store
            .create_task("Fixture follow-up", "/workspace")
            .unwrap()
            .id;
        store.assign_task_to_worker(draft, worker.id).unwrap();
        (draft, worker)
    }

    /// Takes blocked work through to Completed.
    fn finish(store: &TaskStore, id: TaskId) {
        for state in [TaskState::Active, TaskState::Review, TaskState::Completed] {
            store.transition_task(id, state).unwrap();
        }
    }

    fn completed_contract(store: &TaskStore) -> TaskId {
        let id = blocked(store, "Contract");
        store.transition_task(id, TaskState::Active).unwrap();
        store.transition_task(id, TaskState::Review).unwrap();
        store.transition_task(id, TaskState::Completed).unwrap();
        id
    }

    #[test]
    fn unlinked_blocks_are_reassessment_candidates_not_permission_to_resume() {
        let store = TaskStore::in_memory().unwrap();
        let task = blocked(&store, "Missing structured blocker");
        let cursor = store.list_control_room_events(0).unwrap().next_cursor;
        assert!(
            store
                .tasks_ready_after_prerequisites(20)
                .unwrap()
                .0
                .is_empty()
        );
        let (candidates, truncated) = store.blocked_tasks_for_reassessment(20).unwrap();
        assert!(!truncated);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, task);
        assert_eq!(
            candidates[0].next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        assert_eq!(store.get_task(task).unwrap().state, TaskState::Blocked);
        assert!(
            store
                .list_control_room_events(cursor)
                .unwrap()
                .events
                .is_empty()
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET blocked_until = 30 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();
        assert!(
            store
                .blocked_tasks_for_reassessment(29)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(store.blocked_tasks_for_reassessment(30).unwrap().0.len(), 1);
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET blocked_until = unixepoch() + 3600 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Blocked
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET blocked_until = unixepoch() - 1 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();
        assert_eq!(
            store.get_task(task).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET removed_at = 31 WHERE id = ?1",
                [task.to_string()],
            )
            .unwrap();
        assert!(
            store
                .blocked_tasks_for_reassessment(31)
                .unwrap()
                .0
                .is_empty()
        );
    }

    #[test]
    fn review_prerequisites_preserve_review_and_gate_completion_until_satisfied() {
        let store = TaskStore::in_memory().unwrap();
        let task = blocked(&store, "Verify with a shared member session");
        store.transition_task(task, TaskState::Active).unwrap();
        store.transition_task(task, TaskState::Review).unwrap();
        let upstream = blocked(&store, "Provide the shared test session");
        store
            .add_task_prerequisite(
                task,
                upstream,
                "The real browser session is needed for verification",
                &TaskActivityActor::operator(),
                100,
            )
            .unwrap();
        let waiting = store.get_task(task).unwrap();
        assert_eq!(waiting.state, TaskState::Review);
        assert_eq!(
            waiting.next_move_owner,
            swarm_domain::NextMoveOwner::Blocked
        );
        for target in [TaskState::Completed, TaskState::AwaitingRelease] {
            assert!(
                store.transition_task(task, target).is_err(),
                "unsatisfied verification cannot be closed or shipped"
            );
            assert_eq!(store.get_task(task).unwrap().state, TaskState::Review);
        }
        store.transition_task(upstream, TaskState::Active).unwrap();
        store.transition_task(upstream, TaskState::Review).unwrap();
        store
            .transition_task(upstream, TaskState::Completed)
            .unwrap();
        let ready_for_review = store.get_task(task).unwrap();
        assert_eq!(ready_for_review.state, TaskState::Review);
        assert_eq!(
            ready_for_review.next_move_owner,
            swarm_domain::NextMoveOwner::Queen
        );
        store.transition_task(task, TaskState::Completed).unwrap();
    }

    #[test]
    fn ready_prerequisite_discovery_respects_due_dates_and_current_upstream_state() {
        let store = TaskStore::in_memory().unwrap();
        let consumer = blocked(&store, "Consumer");
        let upstream = completed_contract(&store);
        store
            .add_task_prerequisite(
                consumer,
                upstream,
                "Contract first",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        let (tasks, truncated) = store.tasks_ready_after_prerequisites(20).unwrap();
        assert!(!truncated);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, consumer);
        assert_eq!(tasks[0].state, TaskState::Blocked);
        assert_eq!(tasks[0].next_move_owner, swarm_domain::NextMoveOwner::Queen);
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET blocked_until = 30 WHERE id = ?1",
                [consumer.to_string()],
            )
            .unwrap();
        assert!(
            store
                .tasks_ready_after_prerequisites(29)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(
            store.tasks_ready_after_prerequisites(30).unwrap().0.len(),
            1
        );
        for (state, removed) in [
            ("active", None),
            ("abandoned", None),
            ("completed", Some(31)),
        ] {
            store
                .connection()
                .unwrap()
                .execute(
                    "UPDATE tasks SET state = ?2, removed_at = ?3 WHERE id = ?1",
                    params![upstream.to_string(), state, removed],
                )
                .unwrap();
            assert!(
                store
                    .tasks_ready_after_prerequisites(40)
                    .unwrap()
                    .0
                    .is_empty()
            );
            assert!(
                store
                    .blocked_tasks_for_reassessment(40)
                    .unwrap()
                    .0
                    .is_empty()
            );
        }
    }

    #[test]
    fn ready_prerequisite_discovery_filters_before_its_explicit_page_bound() {
        let store = TaskStore::in_memory().unwrap();
        for _ in 0..70 {
            blocked(&store, "Unrelated blocked work");
        }
        let upstream = completed_contract(&store);
        for _ in 0..65 {
            let consumer = blocked(&store, "Ready for Queen");
            store
                .add_task_prerequisite(
                    consumer,
                    upstream,
                    "Contract first",
                    &TaskActivityActor::operator(),
                    10,
                )
                .unwrap();
        }
        let (tasks, truncated) = store.tasks_ready_after_prerequisites(20).unwrap();
        assert!(truncated);
        assert_eq!(tasks.len(), 64);
        assert!(tasks.iter().all(|task| task.title == "Ready for Queen"));
        let (reassessment, truncated) = store.blocked_tasks_for_reassessment(20).unwrap();
        assert!(truncated);
        assert_eq!(reassessment.len(), 64);
    }

    #[test]
    fn ready_prerequisite_discovery_does_not_override_a_pending_operator_decision() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let consumer = blocked(&store, "Consumer");
        let upstream = completed_contract(&store);
        store
            .add_task_prerequisite(
                consumer,
                upstream,
                "Contract first",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        let decision = store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(consumer),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Choose the scope",
                summary: "The contract is ready but scope needs judgment.",
                reason: "Two valid approaches",
                risk: "",
                evidence: "",
                suggested_action: "Proceed",
                allowed_actions: &["Proceed".to_owned()],
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
                requested_script: None,
            })
            .unwrap();
        assert!(
            store
                .tasks_ready_after_prerequisites(20)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(store.get_task(consumer).unwrap().state, TaskState::Blocked);
        assert!(
            store
                .blocked_tasks_for_reassessment(20)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(
            store.get_task(consumer).unwrap().next_move_owner,
            swarm_domain::NextMoveOwner::Operator
        );
        store
            .resolve_decision_request(decision.id, "Proceed", "", "test")
            .unwrap();
        let task = store.get_task(consumer).unwrap();
        assert_eq!(task.state, TaskState::Blocked);
        assert_eq!(task.next_move_owner, swarm_domain::NextMoveOwner::Queen);
        assert_eq!(store.blocked_tasks_for_reassessment(20).unwrap().0.len(), 1);
        assert_eq!(
            store.tasks_ready_after_prerequisites(20).unwrap().0.len(),
            1
        );
    }

    #[test]
    fn edges_are_atomic_idempotent_and_do_not_rewrite_work() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        let actor = TaskActivityActor::operator();
        store
            .add_task_prerequisite(a, b, "Needs contract", &actor, 10)
            .unwrap();
        store
            .add_task_prerequisite(a, b, "Needs contract", &actor, 11)
            .unwrap();
        assert_eq!(store.task_prerequisites(a).unwrap().len(), 1);
        assert_eq!(store.get_task(a).unwrap().state, TaskState::Blocked);
        assert!(matches!(
            store.add_task_prerequisite(b, a, "Cycle", &actor, 12),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Cycle
            ))
        ));
        assert!(store.task_prerequisites(b).unwrap().is_empty());
        assert!(matches!(
            store.add_task_prerequisite(a, b, "Changed", &actor, 13),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Conflict
            ))
        ));
        let count: i64 = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_activity WHERE task_id = ?1 AND kind = 'noted'",
                [a.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(matches!(
            store.transition_task(a, TaskState::Ready),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Unresolved
            ))
        ));
        store
            .remove_task_prerequisite(a, b, "No longer needed", &actor, 14)
            .unwrap();
        store.transition_task(a, TaskState::Ready).unwrap();
    }

    #[test]
    fn completed_removed_and_reopened_prerequisites_remain_distinct() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        store
            .add_task_prerequisite(a, b, "Needs contract", &TaskActivityActor::operator(), 10)
            .unwrap();
        store.transition_task(b, TaskState::Active).unwrap();
        store.transition_task(b, TaskState::Review).unwrap();
        store.transition_task(b, TaskState::Completed).unwrap();
        assert!(store.task_prerequisites(a).unwrap()[0].satisfied());
        assert_eq!(store.get_task(a).unwrap().state, TaskState::Blocked);
        store.transition_task(a, TaskState::Ready).unwrap();
        // A remote status change is not permission to ignore the current prerequisite.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET state = 'active' WHERE id = ?1",
                [b.to_string()],
            )
            .unwrap();
        assert!(!store.task_prerequisites(a).unwrap()[0].satisfied());
        assert!(matches!(
            store.transition_task(a, TaskState::Active),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Unresolved
            ))
        ));
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET state = 'completed', removed_at = 20 WHERE id = ?1",
                [b.to_string()],
            )
            .unwrap();
        let prerequisite = store.task_prerequisites(a).unwrap().remove(0);
        assert!(prerequisite.removed);
        assert!(!prerequisite.satisfied());
    }

    #[test]
    fn prerequisite_failure_rolls_back_relation_and_audit_together() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        store.connection().unwrap().execute_batch("CREATE TRIGGER fail_prerequisite_audit BEFORE INSERT ON task_activity WHEN NEW.kind = 'noted' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        assert!(
            store
                .add_task_prerequisite(a, b, "Contract", &TaskActivityActor::operator(), 10)
                .is_err()
        );
        assert!(store.task_prerequisites(a).unwrap().is_empty());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_prerequisite_audit")
            .unwrap();
        store
            .add_task_prerequisite(a, b, "Contract", &TaskActivityActor::operator(), 11)
            .unwrap();
        assert_eq!(store.task_prerequisites(a).unwrap().len(), 1);
    }

    #[test]
    fn ordinary_workers_and_missing_tasks_cannot_gain_edges() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        assert!(matches!(
            store.add_task_prerequisite(
                a,
                b,
                "Contract",
                &TaskActivityActor::worker(worker.id),
                10
            ),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Unauthorized
            ))
        ));
        assert!(matches!(
            store.add_task_prerequisite(
                a,
                TaskId::new(),
                "Missing",
                &TaskActivityActor::operator(),
                10
            ),
            Err(TaskStoreError::NotFound)
        ));
        assert!(store.task_prerequisites(a).unwrap().is_empty());
    }

    #[test]
    fn a_prerequisite_added_after_claim_prevents_briefing_and_spends_no_attempt() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        let session = swarm_domain::WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store.transition_task(a, TaskState::Ready).unwrap();
        store.assign_task(a, session).unwrap();
        let delivery = store
            .claim_task_dispatches(10, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(store.task_briefing_can_submit(&delivery).unwrap());
        store.transition_task(a, TaskState::Blocked).unwrap();
        store
            .add_task_prerequisite(a, b, "Contract first", &TaskActivityActor::operator(), 11)
            .unwrap();
        assert!(!store.task_briefing_can_submit(&delivery).unwrap());
        store
            .defer_task_dispatch(&delivery.assignment_id, delivery.generation, 12)
            .unwrap();
        // Simulate externally canonical Jira state: local briefing still waits.
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET state = 'ready' WHERE id = ?1",
                [a.to_string()],
            )
            .unwrap();
        assert!(
            store
                .claim_task_dispatches(13, &std::collections::HashSet::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.held_task_dispatches(13).unwrap()[0].reason,
            crate::DispatchHold::PrerequisiteUnresolved
        );
        store
            .remove_task_prerequisite(
                a,
                b,
                "Contract no longer needed",
                &TaskActivityActor::operator(),
                14,
            )
            .unwrap();
        let retried = store
            .claim_task_dispatches(15, &std::collections::HashSet::new())
            .unwrap()
            .remove(0);
        assert!(store.task_briefing_can_submit(&retried).unwrap());
    }

    #[test]
    fn a_changed_prerequisite_defers_an_owned_wake_without_losing_it() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        let queen = store.ensure_queen("/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                swarm_domain::ProviderKind::ClaudeCode,
                "/workspace",
                false,
                1,
            )
            .unwrap();
        store.transition_task(a, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(a, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        let wake = store.claim_coordinator_worker_wakes(10).unwrap().remove(0);
        assert!(store.coordinator_wake_can_start(&wake).unwrap());
        store.transition_task(a, TaskState::Blocked).unwrap();
        store
            .add_task_prerequisite(a, b, "Contract first", &TaskActivityActor::operator(), 11)
            .unwrap();
        assert!(!store.coordinator_wake_can_start(&wake).unwrap());
        store
            .defer_coordinator_worker_wake(&wake.action_id, 12)
            .unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET state = 'ready' WHERE id = ?1",
                [a.to_string()],
            )
            .unwrap();
        assert!(store.claim_coordinator_worker_wakes(13).unwrap().is_empty());
        store
            .remove_task_prerequisite(
                a,
                b,
                "Contract no longer needed",
                &TaskActivityActor::operator(),
                14,
            )
            .unwrap();
        assert_eq!(
            store.claim_coordinator_worker_wakes(15).unwrap()[0].action_id,
            wake.action_id
        );
    }

    #[test]
    fn foreign_hive_tasks_cannot_be_linked_in_either_direction() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Foreign contract");
        let foreign = swarm_domain::HiveId::new().to_string();
        let operator = swarm_domain::OperatorId::new().to_string();
        {
            let connection = store.connection().unwrap();
            connection
                .execute(
                    "INSERT INTO operators (id, display_name) VALUES (?1, 'Other operator')",
                    [&operator],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO hives (id, name, operator_id) VALUES (?1, 'Other Hive', ?2)",
                    params![foreign, operator],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE tasks SET hive_id = ?1 WHERE id = ?2",
                    params![foreign, b.to_string()],
                )
                .unwrap();
        }
        for (source, target) in [(a, b), (b, a)] {
            assert!(matches!(
                store.add_task_prerequisite(
                    source,
                    target,
                    "Contract",
                    &TaskActivityActor::operator(),
                    10
                ),
                Err(TaskStoreError::NotFound)
            ));
        }
        assert!(store.task_prerequisites(a).unwrap().is_empty());
        assert!(matches!(
            store.task_prerequisites(b),
            Err(TaskStoreError::NotFound)
        ));
    }

    #[test]
    fn concurrent_opposite_edges_cannot_commit_a_cycle() {
        let store = TaskStore::in_memory().unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [(a, b), (b, a)]
            .into_iter()
            .map(|(source, target)| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.add_task_prerequisite(
                        source,
                        target,
                        "Contract first",
                        &TaskActivityActor::operator(),
                        10,
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results.iter().any(|result| matches!(
            result,
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Cycle
            ))
        )));
        assert_eq!(
            store.task_prerequisites(a).unwrap().len() + store.task_prerequisites(b).unwrap().len(),
            1
        );
    }

    #[test]
    fn per_task_capacity_refuses_the_next_edge_without_an_audit_write() {
        let store = TaskStore::in_memory().unwrap();
        let task = blocked(&store, "Consumer");
        for _ in 0..MAX_TASK_PREREQUISITES {
            let target = store.create_task("Contract", "/workspace").unwrap();
            store
                .add_task_prerequisite(
                    task,
                    target.id,
                    "Contract",
                    &TaskActivityActor::operator(),
                    10,
                )
                .unwrap();
        }
        let target = store.create_task("One too many", "/workspace").unwrap();
        assert!(matches!(
            store.add_task_prerequisite(
                task,
                target.id,
                "Contract",
                &TaskActivityActor::operator(),
                11
            ),
            Err(TaskStoreError::TaskPrerequisite(
                TaskPrerequisiteError::Capacity
            ))
        ));
        assert_eq!(
            store.task_prerequisites(task).unwrap().len(),
            MAX_TASK_PREREQUISITES
        );
        assert_eq!(store.get_task(task).unwrap().updated_at, 10);
    }

    #[test]
    fn prerequisites_survive_reopen_and_project_the_next_move_without_resuming() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prerequisites.sqlite3");
        let store = TaskStore::open(&path).unwrap();
        let a = blocked(&store, "Consumer");
        let b = blocked(&store, "Contract");
        store
            .add_task_prerequisite(a, b, "Contract first", &TaskActivityActor::operator(), 10)
            .unwrap();
        drop(store);
        let store = TaskStore::open(&path).unwrap();
        let task = store.get_task(a).unwrap();
        assert_eq!(task.prerequisites.len(), 1);
        assert_eq!(task.next_move_owner, swarm_domain::NextMoveOwner::Blocked);
        store.transition_task(b, TaskState::Active).unwrap();
        store.transition_task(b, TaskState::Review).unwrap();
        store.transition_task(b, TaskState::Completed).unwrap();
        let task = store.get_task(a).unwrap();
        assert_eq!(task.next_move_owner, swarm_domain::NextMoveOwner::Queen);
        assert_eq!(task.state, TaskState::Blocked);
        assert!(task.prerequisites[0].satisfied());
        assert_eq!(
            store
                .list_board_tasks()
                .unwrap()
                .iter()
                .find(|task| task.id == a)
                .unwrap()
                .prerequisites,
            task.prerequisites
        );
    }

    /// A draft waits on unfinished work as Blocked with its link intact. When
    /// that work finishes, nothing moves it: unlike other Blocked work, which
    /// comes back to Ready by itself, it stays Blocked and its next move is
    /// Queen's, with a worker assigned and after the resume pass (ADR 0152).
    #[test]
    fn a_draft_behind_unfinished_work_blocks_keeps_its_link_and_returns_to_queen() {
        let store = TaskStore::in_memory().unwrap();
        let (draft, worker) = routed_draft(&store);
        // The prerequisite is itself blocked: it can still finish.
        let upstream = blocked(&store, "Fixture contract");
        store
            .add_task_prerequisite(
                draft,
                upstream,
                "Needs the contract first",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        // The control: ordinary work blocked behind the same task.
        let ordinary = store
            .create_task("Fixture ordinary consumer", "/workspace")
            .unwrap()
            .id;
        store.transition_task(ordinary, TaskState::Ready).unwrap();
        store.transition_task(ordinary, TaskState::Active).unwrap();
        store
            .add_task_prerequisite(
                ordinary,
                upstream,
                "Needs the contract too",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        store
            .transition_task_with_note(ordinary, TaskState::Blocked, "Waits on the contract.")
            .unwrap();

        let refused = store.transition_task(draft, TaskState::Ready).unwrap_err();
        assert!(
            matches!(
                refused,
                TaskStoreError::TaskPrerequisite(TaskPrerequisiteError::DraftWaitsOnUnfinishedWork)
            ),
            "{refused:?}"
        );
        assert!(
            refused.to_string().contains("Move it to blocked"),
            "{refused}"
        );
        assert_eq!(store.get_task(draft).unwrap().state, TaskState::Draft);

        store
            .transition_task_with_note(draft, TaskState::Blocked, "Waits on the contract.")
            .unwrap();
        let waiting = store.get_task(draft).unwrap();
        assert_eq!(waiting.state, TaskState::Blocked);
        assert_eq!(
            waiting.next_move_owner,
            swarm_domain::NextMoveOwner::Blocked
        );
        let wait = waiting.wait.expect("the block names what it waits on");
        assert_eq!(wait.kind, swarm_domain::TaskWaitKind::Task);
        assert!(wait.entered_from_draft);
        assert!(
            !store
                .get_task(ordinary)
                .unwrap()
                .wait
                .unwrap()
                .entered_from_draft
        );
        let links = store.task_prerequisites(draft).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].prerequisite_id, upstream);
        assert_eq!(links[0].reason, "Needs the contract first");

        finish(&store, upstream);
        assert_eq!(
            store.resume_cleared_waits(i64::MAX / 2).unwrap().resumed,
            [ordinary],
            "ordinary work comes back by itself; the former draft does not"
        );
        assert_eq!(store.get_task(ordinary).unwrap().state, TaskState::Ready);
        let released = store.get_task(draft).unwrap();
        assert_eq!(released.state, TaskState::Blocked, "never moved by itself");
        assert_eq!(released.assigned_worker_id, Some(worker.id));
        assert_eq!(released.next_move_owner, swarm_domain::NextMoveOwner::Queen);
        assert_eq!(
            store
                .tasks_ready_after_prerequisites(20)
                .unwrap()
                .0
                .iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec![draft]
        );
        store.transition_task(draft, TaskState::Ready).unwrap();
    }

    /// Blocked has to mean waiting on something that can still finish. A draft
    /// with nothing to wait on, or only work that is already over, is refused
    /// and told what to name (ADR 0152).
    #[test]
    fn a_draft_blocks_only_behind_work_that_can_still_finish() {
        let store = TaskStore::in_memory().unwrap();
        let refused_as_unlinked = |task: TaskId| {
            let refused = store.transition_task(task, TaskState::Blocked).unwrap_err();
            assert!(
                matches!(
                    refused,
                    TaskStoreError::TaskPrerequisite(
                        TaskPrerequisiteError::DraftBlocksOnlyBehindUnfinishedWork
                    )
                ),
                "{refused:?}"
            );
            let text = refused.to_string();
            assert!(text.contains("prerequisite_id"), "{text}");
            assert!(text.contains("decision_id"), "{text}");
            assert_eq!(store.get_task(task).unwrap().state, TaskState::Draft);
        };

        let draft = store
            .create_task("Fixture follow-up", "/workspace")
            .unwrap()
            .id;
        refused_as_unlinked(draft);

        let finished = completed_contract(&store);
        store
            .add_task_prerequisite(
                draft,
                finished,
                "Finished",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        refused_as_unlinked(draft);
        // Nothing it waits on can still finish, so Ready stays open.
        store.transition_task(draft, TaskState::Ready).unwrap();

        let other = store
            .create_task("Fixture other follow-up", "/workspace")
            .unwrap()
            .id;
        let dropped = blocked(&store, "Fixture dropped");
        store
            .add_task_prerequisite(
                other,
                dropped,
                "Dropped",
                &TaskActivityActor::operator(),
                10,
            )
            .unwrap();
        store
            .transition_task(dropped, TaskState::Abandoned)
            .unwrap();
        refused_as_unlinked(other);
    }

    /// A draft waiting on a pending operator decision is waiting on something
    /// that can still finish, so it may be Blocked behind that. The answer
    /// hands it to Queen rather than bringing it back by itself (ADR 0152).
    #[test]
    fn a_draft_waiting_on_a_pending_decision_may_block() {
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let (draft, _worker) = routed_draft(&store);
        let decision = store
            .create_decision_request(&crate::NewDecisionRequest {
                requesting_worker_id: queen.id,
                task_id: Some(draft),
                kind: swarm_domain::DecisionRequestKind::Input,
                urgency: swarm_domain::DecisionUrgency::Normal,
                title: "Choose the scope",
                summary: "Scope needs judgment.",
                reason: "Two valid approaches",
                risk: "",
                evidence: "",
                suggested_action: "Proceed",
                allowed_actions: &["Proceed".to_owned()],
                operator_actions: &[],
                questions: &[],
                deadline: None,
                requested_command: None,
                requested_script: None,
            })
            .unwrap();
        store.transition_task(draft, TaskState::Blocked).unwrap();
        let waiting = store.get_task(draft).unwrap();
        assert_eq!(waiting.state, TaskState::Blocked);
        assert_eq!(
            waiting.next_move_owner,
            swarm_domain::NextMoveOwner::Operator
        );

        // The answer hands it to Queen; it does not come back by itself.
        store
            .resolve_decision_request(decision.id, "Proceed", "", "test")
            .unwrap();
        assert!(
            store
                .resume_cleared_waits(i64::MAX / 2)
                .unwrap()
                .resumed
                .is_empty()
        );
        let answered = store.get_task(draft).unwrap();
        assert_eq!(answered.state, TaskState::Blocked);
        assert_eq!(answered.next_move_owner, swarm_domain::NextMoveOwner::Queen);
    }
}
