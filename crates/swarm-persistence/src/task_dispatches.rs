use std::{collections::HashSet, str::FromStr};

use rusqlite::params;
use swarm_domain::{TaskDispatchState, TaskId, TaskPriority, WorkerId, WorkerSessionId};

use super::{TaskStore, TaskStoreError, insert_control_room_event};
use swarm_domain::ControlRoomEventKind;

const MAX_DISPATCH_CLAIMS: i64 = 16;
/// The same ceiling the assign and transition paths enforce, so repairing a
/// queue cannot push it past a bound those paths respect.
const MAX_PENDING_DISPATCHES: i64 = 256;
/// How long a delivered briefing gets before the work it briefed stops holding
/// up the rest of that worker's queue.
///
/// A worker acting on a briefing leaves Ready within seconds, so this is not a
/// race against normal work: it is the line between "still reading it" and
/// abandoned. The stall that prompted it had been Ready for fifteen hours.
const ABANDONED_BRIEF_SECONDS: i64 = 30 * 60;
const MAX_DISPATCH_ATTEMPTS: i64 = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDispatch {
    pub assignment_id: String,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub session_id: WorkerSessionId,
    pub title: String,
    pub description: String,
    pub priority: TaskPriority,
    pub workspace: String,
    /// The operator's line about how this work should be approached. It governs
    /// the brief, so it travels with it.
    pub operator_instruction: String,
    /// Who wrote in, when this work came from an email.
    ///
    /// A person waiting on a thread is part of the work, not metadata about it.
    /// A worker that is not told cannot know to answer them.
    pub email_requester: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskDispatchFailure {
    Retryable,
    Uncertain,
}

/// Why a briefing that is ready to send has not been sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchHold {
    /// Somebody is using that terminal. Delivering would type into their work.
    OperatorInTheTerminal,
    /// The worker is already on something. Two briefs at once is two tasks.
    WorkerAlreadyWorking,
    /// Behind earlier Ready work for the same worker that has not been briefed
    /// yet. `blocked_by` names it.
    WaitingItsTurn,
}

/// One briefing waiting, and what it is waiting on.
#[derive(Clone, Debug, serde::Serialize)]
pub struct HeldTaskDispatch {
    pub task_id: String,
    pub title: String,
    pub worker_id: String,
    pub worker_name: String,
    pub queued_at: i64,
    pub reason: DispatchHold,
    /// The earlier task this one is queued behind, when that is the hold.
    /// Without it "waiting its turn" is unfalsifiable — sixteen briefings
    /// reported it at once on 2026-08-24 and it named nothing to look at.
    pub blocked_by: Option<String>,
}

impl TaskStore {
    /// Forgets unconfirmed briefings for work that has since moved on.
    ///
    /// "Swarm could not confirm this briefing landed" is a question about work
    /// that is still waiting to be done. Once the task has left ready or active
    /// — completed, blocked, sent to review, removed, or reassigned — the
    /// question is answered or moot, and the mark is telling the operator to go
    /// and check a terminal about something already finished.
    ///
    /// Nothing cleared these. The oldest observed was eight days old, against a
    /// task that had been completed, on a worker with no open work at all.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn forget_moot_unconfirmed_briefings(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let forgotten = transaction.execute(
            "DELETE FROM task_dispatches
             WHERE state = 'uncertain'
               AND NOT EXISTS (
                   SELECT 1 FROM tasks t
                   JOIN task_assignments a
                       ON a.id = task_dispatches.assignment_id AND a.released_at IS NULL
                   WHERE t.id = task_dispatches.task_id
                     AND t.removed_at IS NULL
                     AND t.state IN ('ready', 'active')
               )",
            [],
        )?;
        if forgotten > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::WorkersChanged)?;
        }
        transaction.commit()?;
        Ok(forgotten)
    }

    /// Lists workers holding a briefing Swarm wrote but could not confirm.
    ///
    /// This is Swarm's own delivery evidence, not a reading of terminal
    /// content, so it stays inside the rule that worker attention never comes
    /// from guessing at what is on a screen.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn workers_with_unconfirmed_delivery(&self) -> Result<HashSet<WorkerId>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT DISTINCT worker_id FROM task_dispatches WHERE state = 'uncertain'")?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|result| -> Result<WorkerId, TaskStoreError> {
                let id = result?;
                WorkerId::from_str(&id).map_err(|_| {
                    TaskStoreError::IntegrityFailure("invalid dispatch worker identity".into())
                })
            })
            .collect()
    }

    /// Briefs Ready work that has a live assignment and has never been briefed.
    ///
    /// A briefing is created when work is assigned while Ready, and when work
    /// already assigned becomes Ready. Some work reaches Ready through neither
    /// — measured 2026-08-24, three tasks on one worker with a live assignment,
    /// a live session and no dispatch row at all. Undeliverable, and because
    /// queue order is decided on task position, they blocked every later task
    /// for that worker as well.
    ///
    /// Deliberately only work with no dispatch at all. A briefing that was
    /// delivered and ignored must not be sent again on every tick: that is a
    /// loop, and "Ready work whose delivered brief did not start" is already a
    /// coordination-attention case for Queen to judge.
    ///
    /// Run before every claim so it repairs the queue it is about to read,
    /// rather than needing a sweep somebody remembers to schedule.
    fn rebrief_ready_work_without_a_briefing(
        transaction: &rusqlite::Transaction<'_>,
    ) -> Result<usize, TaskStoreError> {
        let queued: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM task_dispatches WHERE state IN ('queued','dispatching')",
            [],
            |row| row.get(0),
        )?;
        if queued >= MAX_PENDING_DISPATCHES {
            return Ok(0);
        }
        Ok(transaction.execute(
            "INSERT INTO task_dispatches (assignment_id, task_id, worker_id, state)
             SELECT assignment.id, assignment.task_id, session.worker_id, 'queued'
             FROM task_assignments assignment
             JOIN tasks t ON t.id = assignment.task_id
             JOIN worker_sessions session
               ON session.session_id = assignment.worker_session_id
              AND session.ended_at IS NULL
             WHERE assignment.released_at IS NULL
               AND t.state = 'ready' AND t.removed_at IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM task_dispatches dispatch
                   WHERE dispatch.assignment_id = assignment.id
               )",
            [],
        )?)
    }

    /// Atomically claims a bounded batch of current assignments whose worker is quiet.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    /// Why each queued briefing is not being delivered.
    ///
    /// A briefing is held back while the operator is in that worker's terminal,
    /// and while the worker already has other Active work. Both are right —
    /// nobody wants a brief typed into a terminal a person is using, or two
    /// tasks started at once. Neither left any trace.
    ///
    /// Measured 2026-08-24: thirteen briefings six hours old, `attempts` still
    /// zero, nothing in the refusal ledger and nothing in the log, because a
    /// dispatch that is never claimed is never attempted and so never refused.
    /// From the board it was indistinguishable from a broken dispatcher, and
    /// the operator reasonably read it as Queen failing to route work.
    ///
    /// # Errors
    /// Returns an error when the dispatch queue cannot be read.
    pub fn held_task_dispatches(&self, now: i64) -> Result<Vec<HeldTaskDispatch>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT td.task_id, t.title, td.worker_id, w.name, td.updated_at,
                    EXISTS(SELECT 1 FROM worker_engagements e
                           WHERE e.worker_id = td.worker_id AND e.expires_at > ?1) AS engaged,
                    EXISTS(SELECT 1 FROM tasks other
                           WHERE other.assigned_worker_id = td.worker_id
                             AND other.state = 'active' AND other.removed_at IS NULL
                             AND other.id <> td.task_id) AS busy,
                    (SELECT earlier.title FROM tasks earlier
                      WHERE earlier.assigned_worker_id = td.worker_id
                        AND earlier.state = 'ready' AND earlier.removed_at IS NULL
                        AND (earlier.position < t.position
                             OR (earlier.position = t.position AND earlier.id < t.id))
                        AND NOT EXISTS (
                            SELECT 1 FROM task_dispatches earlier_brief
                            WHERE earlier_brief.task_id = earlier.id
                              AND earlier_brief.state IN ('delivered','uncertain')
                              AND earlier_brief.updated_at + ?2 <= ?1)
                      ORDER BY earlier.position, earlier.id LIMIT 1) AS blocked_by
             FROM task_dispatches td
             JOIN tasks t ON t.id = td.task_id
             JOIN worker_profiles w ON w.id = td.worker_id
             WHERE td.state = 'queued' AND t.removed_at IS NULL
             ORDER BY td.updated_at",
        )?;
        let rows = statement.query_map(params![now, ABANDONED_BRIEF_SECONDS], |row| {
            let engaged: bool = row.get(5)?;
            let busy: bool = row.get(6)?;
            let blocked_by: Option<String> = row.get(7)?;
            Ok(HeldTaskDispatch {
                task_id: row.get::<_, String>(0)?,
                title: row.get(1)?,
                worker_id: row.get::<_, String>(2)?,
                worker_name: row.get(3)?,
                queued_at: row.get(4)?,
                blocked_by: blocked_by.clone(),
                reason: if engaged {
                    DispatchHold::OperatorInTheTerminal
                } else if busy {
                    DispatchHold::WorkerAlreadyWorking
                } else {
                    DispatchHold::WaitingItsTurn
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Atomically claims a bounded batch of current assignments whose worker is quiet.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn claim_task_dispatches(&self, now: i64) -> Result<Vec<TaskDispatch>, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        repoint_assignments_left_on_a_dead_session(&transaction)?;
        Self::rebrief_ready_work_without_a_briefing(&transaction)?;
        let candidates = deliverable_briefings(&transaction, now)?;
        for delivery in &candidates {
            let updated = transaction.execute(
                "UPDATE task_dispatches SET state = 'dispatching', attempts = attempts + 1,
                     attempted_at = ?2, updated_at = ?2
                 WHERE assignment_id = ?1 AND state = 'queued' AND attempts < ?3",
                params![delivery.assignment_id, now, MAX_DISPATCH_ATTEMPTS],
            )?;
            if updated != 1 {
                return Err(TaskStoreError::IntegrityFailure(
                    "task dispatch claim lost atomic ownership".into(),
                ));
            }
        }
        if !candidates.is_empty() {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(candidates)
    }

    /// Records an acknowledged task briefing.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn complete_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        self.finish_task_dispatch(assignment_id, now, None)
    }

    /// Records a definitive retryable failure or an ambiguous outcome.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn fail_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
        failure: TaskDispatchFailure,
    ) -> Result<bool, TaskStoreError> {
        self.finish_task_dispatch(assignment_id, now, Some(failure))
    }

    /// Returns a claimed briefing to its durable queue without consuming an
    /// attempt when the provider is waiting for operator input.
    ///
    /// # Errors
    /// Returns a persistence or data-integrity error.
    pub fn defer_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_dispatches
             SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
             WHERE assignment_id = ?1 AND state = 'dispatching'",
            params![assignment_id, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn finish_task_dispatch(
        &self,
        assignment_id: &str,
        now: i64,
        failure: Option<TaskDispatchFailure>,
    ) -> Result<bool, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (state, delivered_at) = match failure {
            None => (TaskDispatchState::Delivered.to_string(), Some(now)),
            Some(TaskDispatchFailure::Uncertain) => {
                (TaskDispatchState::Uncertain.to_string(), None)
            }
            Some(TaskDispatchFailure::Retryable) => {
                let attempts: i64 = transaction.query_row(
                    "SELECT attempts FROM task_dispatches
                     WHERE assignment_id = ?1 AND state = 'dispatching'",
                    [assignment_id],
                    |row| row.get(0),
                )?;
                (
                    if attempts >= MAX_DISPATCH_ATTEMPTS {
                        "uncertain"
                    } else {
                        "queued"
                    }
                    .into(),
                    None,
                )
            }
        };
        let changed = transaction.execute(
            "UPDATE task_dispatches SET state = ?2, delivered_at = ?3, updated_at = ?4
             WHERE assignment_id = ?1 AND state = 'dispatching'",
            params![assignment_id, state, delivered_at, now],
        )? == 1;
        if changed {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Converts crash-interrupted task briefings to explicit, non-retrying uncertainty.
    ///
    /// # Errors
    /// Returns a persistence error.
    pub fn recover_inflight_task_dispatches(&self) -> Result<usize, TaskStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE task_dispatches SET state = 'uncertain', updated_at = unixepoch()
             WHERE state = 'dispatching'",
            [],
        )?;
        if changed > 0 {
            insert_control_room_event(&transaction, ControlRoomEventKind::TasksChanged)?;
        }
        transaction.commit()?;
        Ok(changed)
    }
}
/// Points work at the session its worker is actually running.
///
/// Delivery requires the assignment's own session to be live. A deliberate stop
/// releases assignments as it goes, but a crash, a reboot, or a killed host
/// does not: the rows survive naming a session that will never come back, and
/// every briefing behind one is undeliverable for good. The startup sweep ends
/// those sessions and clears their engagements, but has never touched their
/// assignments.
///
/// Measured after an unplanned reboot on 2026-08-24: seventeen of eighteen
/// queued briefings, across four workers that were all up and running again on
/// new sessions. From the board it looked like every worker had gone quiet.
///
/// Re-pointing rather than releasing, because the work is still that worker's
/// and the briefing is still owed — releasing would drop both and leave the
/// task assigned with nothing queued. The assignment names where work is being
/// sent, not a record of where it was once sent; `task_activity` is the log.
///
/// Run before every claim, so a Hive repairs itself on the next tick rather
/// than needing its workers restarted a second time.
fn repoint_assignments_left_on_a_dead_session(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<usize, TaskStoreError> {
    Ok(transaction.execute(
        "UPDATE task_assignments AS stranded
            SET worker_session_id = (
                SELECT live.session_id FROM worker_sessions live
                JOIN tasks task ON task.id = stranded.task_id
                WHERE live.worker_id = task.assigned_worker_id AND live.ended_at IS NULL
            )
          WHERE stranded.released_at IS NULL
            AND EXISTS (
                SELECT 1 FROM worker_sessions dead
                WHERE dead.session_id = stranded.worker_session_id
                  AND dead.ended_at IS NOT NULL
            )
            AND EXISTS (
                SELECT 1 FROM worker_sessions live
                JOIN tasks task ON task.id = stranded.task_id
                WHERE live.worker_id = task.assigned_worker_id AND live.ended_at IS NULL
            )",
        [],
    )?)
}

/// The briefings that may be delivered right now, in queue order.
///
/// Split out of `claim_task_dispatches` only because the conditions and the
/// history behind each one no longer fit in one function.
fn deliverable_briefings(
    transaction: &rusqlite::Transaction<'_>,
    now: i64,
) -> Result<Vec<TaskDispatch>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT td.assignment_id, td.task_id, td.worker_id, a.worker_session_id,
                    t.title, t.description, t.priority, t.workspace,
                    t.operator_instruction,
                    (SELECT COALESCE(NULLIF(link.sender_name, ''), link.sender_address)
                     FROM email_message_links link
                     WHERE link.task_id = t.id
                     ORDER BY link.received_at LIMIT 1)
             FROM task_dispatches td
             JOIN task_assignments a ON a.id = td.assignment_id AND a.released_at IS NULL
             JOIN tasks t ON t.id = td.task_id
             JOIN worker_sessions ws ON ws.session_id = a.worker_session_id
                 AND ws.worker_id = td.worker_id AND ws.ended_at IS NULL
             -- A briefing is owed for work that has already started, not
             -- only for work still waiting. Delivery runs on a timer, and a
             -- task can be started within a second of being assigned, so
             -- requiring 'ready' here stranded the briefing permanently:
             -- never claimed, never attempted, no error, and a woken worker
             -- sitting at a blank prompt.
             WHERE td.state = 'queued' AND t.removed_at IS NULL
               AND t.state IN ('ready', 'active')
               AND NOT EXISTS (
                   SELECT 1 FROM worker_engagements e
                   WHERE e.worker_id = td.worker_id AND e.expires_at > ?1
               )
               -- Other active work still holds a briefing back. The task
               -- being briefed does not count against itself.
               AND NOT EXISTS (
                   SELECT 1 FROM tasks active
                   WHERE active.assigned_worker_id = td.worker_id
                     AND active.state = 'active' AND active.removed_at IS NULL
                     AND active.id <> t.id
               )
               -- Queue order governs work that is waiting. Work already
               -- under way is not waiting its turn.
               -- A briefing already delivered holds the queue only while
               -- the worker could still be acting on it. Blocking on
               -- position with no time limit let one task that was briefed
               -- and never started hold up every later task for that
               -- worker indefinitely: measured at sixteen briefings queued
               -- behind five such tasks, none of them claimable, which
               -- read from outside as Queen refusing to route work.
               AND (t.state = 'active' OR NOT EXISTS (
                   SELECT 1 FROM tasks earlier
                   WHERE earlier.assigned_worker_id = td.worker_id
                     AND earlier.state = 'ready' AND earlier.removed_at IS NULL
                     AND (earlier.position < t.position
                          OR (earlier.position = t.position AND earlier.id < t.id))
                     AND NOT EXISTS (
                         SELECT 1 FROM task_dispatches earlier_brief
                         WHERE earlier_brief.task_id = earlier.id
                           AND earlier_brief.state IN ('delivered','uncertain')
                           AND earlier_brief.updated_at + ?3 <= ?1
                     )
               ))
             ORDER BY t.position, td.updated_at, td.assignment_id
             LIMIT ?2",
    )?;
    statement
        .query_map(
            params![now, MAX_DISPATCH_CLAIMS, ABANDONED_BRIEF_SECONDS],
            |row| {
                let priority = TaskPriority::from_str(&row.get::<_, String>(6)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(TaskDispatch {
                    assignment_id: row.get(0)?,
                    task_id: row
                        .get::<_, String>(1)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    worker_id: row
                        .get::<_, String>(2)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    session_id: row
                        .get::<_, String>(3)?
                        .parse()
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    title: row.get(4)?,
                    description: row.get(5)?,
                    priority,
                    workspace: row.get(7)?,
                    operator_instruction: row.get(8)?,
                    email_requester: row.get(9)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(TaskStoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{PresenceDeviceId, ProviderKind, TaskDispatchState};

    /// Queen's only non-completing exit from review used to be Active, and
    /// that transition enqueued nothing: the task changed column and the worker
    /// was never told, so it sat in Active looking like work nobody was doing.
    #[test]
    fn work_sent_back_from_review_is_briefed_again() {
        let (store, task_id, _session) = assigned_task();
        let first = store.claim_task_dispatches(100).unwrap();
        assert_eq!(first.len(), 1, "the original briefing");
        store
            .complete_task_dispatch(&first[0].assignment_id, 100)
            .unwrap();
        store
            .transition_task(task_id, swarm_domain::TaskState::Active)
            .unwrap();
        store
            .transition_task(task_id, swarm_domain::TaskState::Review)
            .unwrap();
        assert!(
            store.claim_task_dispatches(100).unwrap().is_empty(),
            "nothing is owed while the work sits in review"
        );

        store
            .transition_task(task_id, swarm_domain::TaskState::Active)
            .unwrap();

        let again = store.claim_task_dispatches(100).unwrap();
        assert_eq!(
            again.len(),
            1,
            "work handed back to a worker owes it a briefing again"
        );
        assert_eq!(again[0].task_id, task_id);
    }

    /// A worker starting its own Ready work is acting on the briefing it was
    /// just given. Re-arming there would replay it.
    #[test]
    fn a_worker_starting_its_own_ready_work_is_not_briefed_twice() {
        let (store, task_id, _session) = assigned_task();
        let first = store.claim_task_dispatches(100).unwrap();
        assert_eq!(first.len(), 1);
        store
            .complete_task_dispatch(&first[0].assignment_id, 100)
            .unwrap();

        store
            .transition_task(task_id, swarm_domain::TaskState::Active)
            .unwrap();

        assert!(
            store.claim_task_dispatches(100).unwrap().is_empty(),
            "starting the work is not a reason to repeat its briefing"
        );
    }

    fn assigned_task() -> (TaskStore, TaskId, WorkerSessionId) {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task_with_details(
                "Polish the empty state",
                "Keep the bee expressive at mobile size.",
                TaskPriority::High,
                "/workspace/petal",
            )
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store.assign_task(task.id, session).unwrap();
        (store, task.id, session)
    }

    #[test]
    fn assignment_waits_for_engagement_then_delivers_to_current_session() {
        let (store, task_id, session) = assigned_task();
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Queued)
        );
        store
            .renew_worker_engagement(session, Some(PresenceDeviceId::new()), 100, 300)
            .unwrap();
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());

        let dispatches = store.claim_task_dispatches(401).unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].task_id, task_id);
        assert_eq!(dispatches[0].session_id, session);
        assert_eq!(dispatches[0].priority, TaskPriority::High);
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Dispatching)
        );

        assert!(
            store
                .complete_task_dispatch(&dispatches[0].assignment_id, 402)
                .unwrap()
        );
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Delivered)
        );
    }

    #[test]
    fn draft_assignment_records_ownership_without_briefing() {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        let task = store
            .create_task("Shape the task", "/workspace/petal")
            .unwrap();

        let assigned = store.assign_task(task.id, session).unwrap();

        assert_eq!(assigned.state, swarm_domain::TaskState::Draft);
        assert_eq!(assigned.assigned_worker_id, Some(worker.id));
        assert_eq!(assigned.dispatch_state, None);
        assert!(store.claim_task_dispatches(100).unwrap().is_empty());

        let ready = store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        assert_eq!(ready.dispatch_state, Some(TaskDispatchState::Queued));
        assert_eq!(store.claim_task_dispatches(101).unwrap().len(), 1);
    }

    #[test]
    fn one_worker_receives_only_the_first_ready_brief_until_it_finishes() {
        let (store, first_id, session) = assigned_task();
        let second = store
            .create_task("Second queued task", "/workspace/petal")
            .unwrap();
        store
            .transition_task(second.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store.assign_task(second.id, session).unwrap();

        let first_dispatch = store.claim_task_dispatches(100).unwrap();
        assert_eq!(first_dispatch.len(), 1);
        assert_eq!(first_dispatch[0].task_id, first_id);
        let worker_id = first_dispatch[0].worker_id;
        store
            .complete_task_dispatch(&first_dispatch[0].assignment_id, 101)
            .unwrap();
        assert!(store.claim_task_dispatches(102).unwrap().is_empty());

        store
            .transition_task(first_id, swarm_domain::TaskState::Active)
            .unwrap();
        assert!(store.claim_task_dispatches(103).unwrap().is_empty());
        store
            .transition_task(first_id, swarm_domain::TaskState::Review)
            .unwrap();
        store
            .transition_task(first_id, swarm_domain::TaskState::Completed)
            .unwrap();

        let next_dispatch = store.claim_task_dispatches(104).unwrap();
        assert_eq!(next_dispatch.len(), 1);
        assert_eq!(next_dispatch[0].task_id, second.id);
        assert_eq!(next_dispatch[0].worker_id, worker_id);
    }

    #[test]
    fn an_unconfirmed_briefing_is_forgotten_once_its_work_moves_on() {
        // Observed 2026-08-20: six unconfirmed briefings, the oldest eight days
        // old, against tasks that had been completed, blocked, or sent to
        // review. One belonged to a worker with no open work at all. The mark
        // says "Swarm could not confirm this landed", which is a question about
        // work still waiting to be done — once the task has moved on it is
        // answered or moot, and nothing was clearing it.
        let (store, task_id, _session) = assigned_task();
        let claimed = store.claim_task_dispatches(100).unwrap();
        store
            .fail_task_dispatch(
                &claimed[0].assignment_id,
                101,
                TaskDispatchFailure::Uncertain,
            )
            .unwrap();
        assert_eq!(store.workers_with_unconfirmed_delivery().unwrap().len(), 1);

        // Still waiting to be done: the question stands.
        assert_eq!(store.forget_moot_unconfirmed_briefings().unwrap(), 0);
        assert_eq!(store.workers_with_unconfirmed_delivery().unwrap().len(), 1);

        for state in [
            swarm_domain::TaskState::Active,
            swarm_domain::TaskState::Review,
            swarm_domain::TaskState::Completed,
        ] {
            store.transition_task(task_id, state).unwrap();
        }

        assert_eq!(store.forget_moot_unconfirmed_briefings().unwrap(), 1);
        assert!(
            store
                .workers_with_unconfirmed_delivery()
                .unwrap()
                .is_empty(),
            "a briefing about finished work stops marking the worker"
        );
    }

    #[test]
    fn a_briefing_survives_its_task_being_started_before_delivery() {
        // Observed 2026-08-20: an imported email task was assigned at 00:32:29
        // and started at 00:32:50 — twenty-one seconds later, inside the
        // thirty-second delivery interval. Requiring 'ready' at claim time
        // meant the briefing was never claimed, never attempted, and reported
        // no error, while the woken worker sat at a blank prompt.
        let (store, task_id, _session) = assigned_task();

        store
            .transition_task(task_id, swarm_domain::TaskState::Active)
            .unwrap();

        let claimed = store.claim_task_dispatches(100).unwrap();

        assert_eq!(
            claimed.len(),
            1,
            "work already started is still owed its briefing"
        );
        assert_eq!(claimed[0].task_id, task_id);
    }

    #[test]
    fn other_active_work_still_holds_a_briefing_back() {
        // The task being briefed does not count against itself, but a
        // different active task does: that is the rule that stops a worker
        // being handed new work while it is busy.
        let (store, first_id, session) = assigned_task();
        let second = store
            .create_task("Second queued task", "/workspace/petal")
            .unwrap();
        store
            .transition_task(second.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store.assign_task(second.id, session).unwrap();
        let first = store.claim_task_dispatches(100).unwrap();
        store
            .complete_task_dispatch(&first[0].assignment_id, 101)
            .unwrap();

        store
            .transition_task(first_id, swarm_domain::TaskState::Active)
            .unwrap();

        assert!(
            store.claim_task_dispatches(102).unwrap().is_empty(),
            "the second task waits while the first is under way"
        );
    }

    #[test]
    fn reassignment_and_session_release_cancel_only_queued_briefings() {
        let (store, task_id, first_session) = assigned_task();
        let second = store
            .create_worker(
                "Violet",
                ProviderKind::ClaudeCode,
                "/workspace/violet",
                false,
                2,
            )
            .unwrap();
        let second_session = WorkerSessionId::new();
        store
            .bind_worker_session(second.id, second_session)
            .unwrap();

        store.assign_task(task_id, second_session).unwrap();
        let dispatches = store.claim_task_dispatches(100).unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].session_id, second_session);
        assert_ne!(dispatches[0].session_id, first_session);

        let second_task = store
            .create_task_with_details(
                "Verify mobile controls",
                "",
                TaskPriority::Normal,
                "/workspace/violet",
            )
            .unwrap();
        store.assign_task(second_task.id, second_session).unwrap();
        assert_eq!(
            store.release_session_assignments(second_session).unwrap(),
            2
        );
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());
        assert_eq!(store.get_task(second_task.id).unwrap().dispatch_state, None);
    }

    #[test]
    fn crash_ambiguity_never_replays_a_task_briefing() {
        let (store, task_id, _) = assigned_task();
        assert_eq!(store.claim_task_dispatches(100).unwrap().len(), 1);
        assert_eq!(store.recover_inflight_task_dispatches().unwrap(), 1);
        assert!(store.claim_task_dispatches(101).unwrap().is_empty());
        assert_eq!(
            store.get_task(task_id).unwrap().dispatch_state,
            Some(TaskDispatchState::Uncertain)
        );
    }

    /// Thirteen briefings sat six hours with `attempts` at zero and nothing
    /// anywhere saying why: a dispatch the claim skips is never attempted, so
    /// it never reaches the refusal ledger either. From the board it looked
    /// like Queen had assigned work and nothing had happened.
    #[test]
    fn a_briefing_that_is_not_moving_says_what_it_is_waiting_on() {
        use swarm_domain::{TaskActivityActor, TaskPriority, TaskState};
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        let task = store
            .create_task_with_details("Brief me", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(task.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        // Nothing in the way: it is simply next.
        let waiting = store.held_task_dispatches(1_000).unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].reason, DispatchHold::WaitingItsTurn);
        assert_eq!(waiting[0].worker_name, "Petal");

        // The operator opens that terminal. The briefing is now held, and says so.
        let device = swarm_domain::PresenceDeviceId::new();
        store
            .renew_worker_engagement(session, Some(device), 1_000, 300)
            .unwrap();

        let held = store.held_task_dispatches(1_100).unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].reason, DispatchHold::OperatorInTheTerminal);
        assert_eq!(held[0].task_id, task.id.to_string());
    }

    /// The other legitimate hold: one worker, one task at a time.
    #[test]
    fn a_worker_already_working_is_a_different_answer_from_an_open_terminal() {
        use swarm_domain::{TaskActivityActor, TaskPriority, TaskState};
        let store = TaskStore::in_memory().unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();

        let busy = store
            .create_task_with_details("Under way", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        store.transition_task(busy.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(busy.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();
        store.transition_task(busy.id, TaskState::Active).unwrap();

        let next = store
            .create_task_with_details("Waiting", "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        store.transition_task(next.id, TaskState::Ready).unwrap();
        store
            .assign_task_to_worker_as(next.id, worker.id, &TaskActivityActor::worker(queen.id))
            .unwrap();

        let held = store.held_task_dispatches(1_000).unwrap();
        let waiting = held
            .iter()
            .find(|entry| entry.task_id == next.id.to_string())
            .expect("the second briefing is queued");
        assert_eq!(waiting.reason, DispatchHold::WorkerAlreadyWorking);
    }

    /// An engaged worker with a live session and no work briefed yet.
    fn engaged_worker() -> (TaskStore, WorkerSessionId) {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker(
                "Petal",
                ProviderKind::ClaudeCode,
                "/workspace/petal",
                false,
                1,
            )
            .unwrap();
        let session = WorkerSessionId::new();
        store.bind_worker_session(worker.id, session).unwrap();
        store
            .renew_worker_engagement(session, Some(PresenceDeviceId::new()), 100, 300)
            .unwrap();
        (store, session)
    }

    fn ready_task_assigned_to(store: &TaskStore, session: WorkerSessionId, title: &str) -> TaskId {
        let task = store
            .create_task_with_details(title, "", TaskPriority::Normal, "/workspace/petal")
            .unwrap();
        store
            .transition_task(task.id, swarm_domain::TaskState::Ready)
            .unwrap();
        store.assign_task(task.id, session).unwrap();
        task.id
    }

    /// The shape behind "workers are idle and she hasn't pushed them to work".
    ///
    /// Ready work that reached Ready through neither path that creates a
    /// briefing has none, so it can never be delivered — and because queue
    /// order was decided on task position alone, it also held up every later
    /// task for that worker. Measured 2026-08-24: sixteen briefings queued and
    /// zero claimable.
    #[test]
    fn ready_work_that_was_never_briefed_is_briefed_rather_than_stranded() {
        let (store, session) = engaged_worker();
        let task_id = ready_task_assigned_to(&store, session, "Never briefed");
        // Reaching Ready by a path that leaves no dispatch row behind.
        store
            .connection()
            .unwrap()
            .execute("DELETE FROM task_dispatches", [])
            .unwrap();

        let claimed = store.claim_task_dispatches(401).unwrap();
        assert_eq!(
            claimed
                .iter()
                .filter(|dispatch| dispatch.task_id == task_id)
                .count(),
            1,
            "assigned Ready work with no briefing must be given one"
        );
    }

    /// Sculpt Studio's stall.
    ///
    /// A briefing that was delivered and never started leaves its task Ready
    /// forever. Holding the queue on position with no time limit let that one
    /// task block every later task for its worker indefinitely. Once the
    /// briefing is old enough that the worker plainly is not acting on it, the
    /// queue moves on; whether to chase the abandoned one is Queen's to judge,
    /// not the queue's.
    #[test]
    fn work_already_briefed_does_not_hold_up_the_rest_of_the_queue() {
        let (store, session) = engaged_worker();
        let stalled = ready_task_assigned_to(&store, session, "Briefed, never started");
        let delivered = store.claim_task_dispatches(401).unwrap();
        assert_eq!(delivered.len(), 1);
        store
            .complete_task_dispatch(&delivered[0].assignment_id, 402)
            .unwrap();

        let behind = ready_task_assigned_to(&store, session, "Behind it");

        // Still being acted on: the queue holds, which is the rule that keeps a
        // worker from being handed two tasks at once.
        assert!(
            store.claim_task_dispatches(403).unwrap().is_empty(),
            "a briefing delivered seconds ago must still hold the queue"
        );

        // An hour later it plainly is not being acted on. Written out rather
        // than derived from the constant, so the contract is asserted and not
        // just restated.
        let an_hour_later = 402 + 60 * 60;
        let claimed = store.claim_task_dispatches(an_hour_later).unwrap();
        assert!(
            claimed.iter().any(|dispatch| dispatch.task_id == behind),
            "an abandoned briefing must not block the queue forever"
        );
        assert!(
            !claimed.iter().any(|dispatch| dispatch.task_id == stalled),
            "and it must not be briefed a second time on every tick"
        );
    }
}
