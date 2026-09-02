//! A governed channel between Queen and a worker, recorded on the task.
//!
//! Queen was already asking workers questions before this existed — through
//! Claude Code's own session channel, which Swarm did not build, cannot see and
//! cannot record. The useful half of that is worth keeping: she can ask a
//! worker something without resetting its conversation. The rest was a hole in
//! the premise that what is not on the board did not happen.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use swarm_domain::{TaskId, WorkerId};
use uuid::Uuid;

use super::{TaskStore, TaskStoreError};

/// Who an end of an exchange is.
///
/// Not a worker id on its own, because the RULE is about roles: Queen may talk
/// to any worker, a worker may talk to Queen, and no worker may talk to another
/// worker. Expressing that with ids alone would make the rule a comparison
/// somebody has to remember to write.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageParty {
    Queen,
    Worker,
    /// The operator, who may reach anyone and is never a recipient here — they
    /// read the board rather than an inbox.
    Operator,
}

impl MessageParty {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Queen => "queen",
            Self::Worker => "worker",
            Self::Operator => "operator",
        }
    }
}

/// One end of an exchange: the role, and which worker when the role is a worker.
///
/// Kept together because they are one fact. Passing them as loose arguments let
/// a caller name a worker recipient without a worker id, which is a message
/// with nowhere to arrive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageEnd {
    pub party: MessageParty,
    pub worker_id: Option<WorkerId>,
}

impl MessageEnd {
    #[must_use]
    pub const fn queen() -> Self {
        Self {
            party: MessageParty::Queen,
            worker_id: None,
        }
    }

    #[must_use]
    pub const fn operator() -> Self {
        Self {
            party: MessageParty::Operator,
            worker_id: None,
        }
    }

    #[must_use]
    pub const fn worker(worker_id: WorkerId) -> Self {
        Self {
            party: MessageParty::Worker,
            worker_id: Some(worker_id),
        }
    }

    /// The worker this end names, or `None` when it is the Queen office.
    #[must_use]
    pub const fn worker_id(self) -> Option<WorkerId> {
        self.worker_id
    }
}

/// One message in a task's exchange.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskMessage {
    pub id: String,
    pub task_id: TaskId,
    pub sender: MessageParty,
    pub recipient: MessageParty,
    pub sender_worker_id: Option<String>,
    pub recipient_worker_id: Option<String>,
    pub body: String,
    pub created_at: i64,
    /// When it reached the recipient's terminal, or `None` while it waits.
    ///
    /// It waits on purpose: delivery holds until the session is resting, which
    /// is what stops a question arriving mid-turn and taking the thread with
    /// it.
    pub delivered_at: Option<i64>,
}

/// The largest message the channel accepts.
///
/// A question, not a second description. Something longer is a task amendment
/// or a new task, and letting it through here would make the channel a way to
/// redirect work with no record of the work changing.
pub const MAX_TASK_MESSAGE_BYTES: usize = 4_000;

impl TaskStore {
    /// Records a message from Queen to a worker, or from a worker to Queen.
    ///
    /// # Errors
    /// Refuses a worker-to-worker send, an empty or oversized body, a message
    /// to a worker that does not name one, and an unknown task.
    pub fn send_task_message(
        &self,
        task_id: TaskId,
        from: MessageEnd,
        to: MessageEnd,
        body: &str,
        now: i64,
    ) -> Result<TaskMessage, TaskStoreError> {
        let (sender, sender_worker_id) = (from.party, from.worker_id);
        let (recipient, recipient_worker_id) = (to.party, to.worker_id);
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_TASK_MESSAGE_BYTES {
            return Err(TaskStoreError::InvalidTaskMessage {
                max: MAX_TASK_MESSAGE_BYTES,
            });
        }
        // REFUSED HERE AS WELL AS IN THE SCHEMA. The CHECK is the guarantee;
        // this is the explanation. A caller that hits the constraint gets "SQL
        // failed", which tells them nothing about why the product will not do
        // this.
        if sender == MessageParty::Worker && recipient == MessageParty::Worker {
            return Err(TaskStoreError::WorkerToWorkerMessageRefused);
        }
        // A message to "a worker" that names none has no inbox to arrive in,
        // and silence is the one failure this channel must not have.
        if recipient == MessageParty::Worker && recipient_worker_id.is_none() {
            return Err(TaskStoreError::InvalidTaskMessage {
                max: MAX_TASK_MESSAGE_BYTES,
            });
        }
        let connection = self.connection()?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND removed_at IS NULL)",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(TaskStoreError::NotFound);
        }
        let id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO task_messages
                 (id, task_id, sender, recipient, sender_worker_id, recipient_worker_id,
                  body, created_at, delivered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
            params![
                id,
                task_id.to_string(),
                sender.as_str(),
                recipient.as_str(),
                sender_worker_id.map(|id| id.to_string()),
                recipient_worker_id.map(|id| id.to_string()),
                body,
                now,
            ],
        )?;
        Ok(TaskMessage {
            id,
            task_id,
            sender,
            recipient,
            sender_worker_id: sender_worker_id.map(|id| id.to_string()),
            recipient_worker_id: recipient_worker_id.map(|id| id.to_string()),
            body: body.to_owned(),
            created_at: now,
            delivered_at: None,
        })
    }

    /// Reads a task's exchange, oldest first.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn task_messages(&self, task_id: TaskId) -> Result<Vec<TaskMessage>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, task_id, sender, recipient, sender_worker_id, recipient_worker_id,
                    body, created_at, delivered_at
             FROM task_messages WHERE task_id = ?1
             ORDER BY created_at, id",
        )?;
        let messages = statement
            .query_map([task_id.to_string()], message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Messages waiting to reach one worker's terminal.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn undelivered_task_messages(
        &self,
        worker_id: WorkerId,
    ) -> Result<Vec<TaskMessage>, TaskStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, task_id, sender, recipient, sender_worker_id, recipient_worker_id,
                    body, created_at, delivered_at
             FROM task_messages
             WHERE recipient_worker_id = ?1 AND delivered_at IS NULL
             ORDER BY created_at, id",
        )?;
        let messages = statement
            .query_map([worker_id.to_string()], message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Marks one message as having reached its recipient's terminal.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn mark_task_message_delivered(&self, id: &str, now: i64) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_messages SET delivered_at = ?2 WHERE id = ?1 AND delivered_at IS NULL",
            params![id, now],
        )?;
        Ok(())
    }

    /// Whether a worker has an unanswered question from Queen on a task.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn worker_owes_an_answer(&self, task_id: TaskId) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        let last: Option<String> = connection
            .query_row(
                "SELECT sender FROM task_messages WHERE task_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(last.as_deref() == Some("queen"))
    }
}

/// A message with a live terminal to write it into.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskMessageDispatch {
    pub message_id: String,
    pub task_id: TaskId,
    pub task_title: String,
    pub session_id: swarm_domain::WorkerSessionId,
    pub sender: MessageParty,
    pub sender_name: String,
    pub body: String,
}

impl TaskStore {
    /// Undelivered messages whose recipient has a session that is still open.
    ///
    /// A message for a worker that is asleep stays queued rather than being
    /// dropped or counted as delivered — it arrives when that worker is next
    /// running, which is the difference between a channel and a broadcast.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn pending_task_message_dispatches(
        &self,
    ) -> Result<Vec<TaskMessageDispatch>, TaskStoreError> {
        use std::str::FromStr;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            // BOTH DIRECTIONS. This filtered `m.recipient = 'worker'`, so every
            // worker-to-Queen message was recorded and never delivered — the
            // channel was one-way while its tool told the worker "Queen sees it
            // on her next run". Silence is the single failure this channel
            // exists to remove, and it had it in the reply direction.
            //
            // Queen is resolved by ROLE rather than by an id on the row,
            // because a message to Queen is addressed to the office: the
            // recipient_worker_id is null and whoever holds the role reads it.
            "SELECT m.id, m.task_id, task.title, session.session_id, m.sender,
                    COALESCE(sender.name, 'Queen'), m.body
             FROM task_messages m
             JOIN tasks task ON task.id = m.task_id AND task.removed_at IS NULL
             JOIN worker_profiles recipient
                  ON (m.recipient = 'worker' AND recipient.id = m.recipient_worker_id)
                  OR (m.recipient = 'queen' AND recipient.role = 'queen')
             JOIN worker_sessions session ON session.worker_id = recipient.id
                  AND session.ended_at IS NULL
             LEFT JOIN worker_profiles sender ON sender.id = m.sender_worker_id
             WHERE m.delivered_at IS NULL
             ORDER BY m.created_at, m.id",
        )?;
        let dispatches = statement
            .query_map([], |row| {
                let task_id: String = row.get(1)?;
                let session_id: String = row.get(3)?;
                let sender: String = row.get(4)?;
                Ok(TaskMessageDispatch {
                    message_id: row.get(0)?,
                    task_id: TaskId::from_str(&task_id)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    task_title: row.get(2)?,
                    session_id: swarm_domain::WorkerSessionId::from_str(&session_id)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    sender: match sender.as_str() {
                        "queen" => MessageParty::Queen,
                        "operator" => MessageParty::Operator,
                        _ => MessageParty::Worker,
                    },
                    sender_name: row.get(5)?,
                    body: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(dispatches)
    }

    /// Whether anything could deliver a message to this recipient right now.
    ///
    /// `pending_task_message_dispatches` joins `worker_sessions` on
    /// `ended_at IS NULL`, so a message to a recipient with no open session is
    /// not merely slow — it is excluded from the dispatch query outright and
    /// waits for a session that may never start. Nothing surfaces that: the
    /// message sits with `delivered_at` null, which looks identical to one
    /// queued behind a busy terminal.
    ///
    /// So the sender is told at the moment of sending, which is the only point
    /// where they can still choose another route.
    ///
    /// # Errors
    ///
    /// Returns [`TaskStoreError`] if the database cannot be read.
    pub fn recipient_has_open_session(
        &self,
        recipient: MessageEnd,
    ) -> Result<bool, TaskStoreError> {
        let connection = self.connection()?;
        // Queen is matched by ROLE for the same reason the dispatch query does
        // it: a message to Queen is addressed to the office, not to an id.
        let open: i64 = match recipient.worker_id() {
            Some(worker) => connection.query_row(
                "SELECT COUNT(*) FROM worker_sessions
                 WHERE worker_id = ?1 AND ended_at IS NULL",
                [worker.to_string()],
                |row| row.get(0),
            )?,
            None => connection.query_row(
                "SELECT COUNT(*) FROM worker_sessions session
                 JOIN worker_profiles worker ON worker.id = session.worker_id
                 WHERE worker.role = 'queen' AND session.ended_at IS NULL",
                [],
                |row| row.get(0),
            )?,
        };
        Ok(open > 0)
    }
}

fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskMessage> {
    use std::str::FromStr;
    let task_id: String = row.get(1)?;
    let sender: String = row.get(2)?;
    let recipient: String = row.get(3)?;
    let party = |value: &str| match value {
        "queen" => MessageParty::Queen,
        "operator" => MessageParty::Operator,
        _ => MessageParty::Worker,
    };
    Ok(TaskMessage {
        id: row.get(0)?,
        task_id: TaskId::from_str(&task_id).map_err(|_| rusqlite::Error::InvalidQuery)?,
        sender: party(&sender),
        recipient: party(&recipient),
        sender_worker_id: row.get(4)?,
        recipient_worker_id: row.get(5)?,
        body: row.get(6)?,
        created_at: row.get(7)?,
        delivered_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::{ProviderKind, TaskState, WorkerSessionId};

    fn hive() -> (TaskStore, TaskId, WorkerId) {
        let store = TaskStore::in_memory().unwrap();
        store.ensure_queen("/workspace/queen").unwrap();
        let worker = store
            .create_worker("Platform", ProviderKind::ClaudeCode, "/workspace", false, 1)
            .unwrap();
        let task = store.create_task("Some work", "/workspace").unwrap();
        store.transition_task(task.id, TaskState::Ready).unwrap();
        (store, task.id, worker.id)
    }

    /// The operator's requirement, and the one that must not be a convention:
    /// "No worker to worker communication, but queen<->worker communication is
    /// fine in both directions."
    #[test]
    fn a_worker_cannot_message_another_worker() {
        let (store, task, worker) = hive();
        let other = store
            .create_worker("Hub", ProviderKind::ClaudeCode, "/workspace/hub", false, 2)
            .unwrap();

        let refused = store.send_task_message(
            task,
            MessageEnd::worker(worker),
            MessageEnd::worker(other.id),
            "Queen said you should stop.",
            1_000,
        );

        assert!(
            matches!(refused, Err(TaskStoreError::WorkerToWorkerMessageRefused)),
            "a peer channel would let a fabricated ruling travel with no board record"
        );
        assert!(
            store.task_messages(task).unwrap().is_empty(),
            "and nothing is recorded, so a refusal cannot be read later as an exchange"
        );
    }

    /// THE SENDER'S STATUS MUST NOT BE A CONSTANT.
    ///
    /// `swarm_message_worker` used to answer with a hardcoded `delivered:
    /// false`. It reads as a live status and was the same value for every
    /// message ever sent, including ones delivered a second later. Queen relied
    /// on it and reported three messages as having sat undelivered for 20-45
    /// minutes; two had been delivered within one second, twenty minutes before
    /// she filed the report.
    ///
    /// The distinction the sender actually needs is not delivered-or-not — the
    /// answer to that is always "not" at send time — but whether anything CAN
    /// deliver it. A recipient with no open session is excluded from
    /// `pending_task_message_dispatches` outright, so its message does not
    /// arrive late, it does not arrive at all.
    #[test]
    fn a_recipient_with_no_session_is_distinguishable_from_a_busy_one() {
        let (store, _task, worker) = hive();

        assert!(
            !store
                .recipient_has_open_session(MessageEnd::worker(worker))
                .unwrap(),
            "a worker with no session cannot be delivered to, and saying `queued` would be a lie"
        );

        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();

        assert!(
            store
                .recipient_has_open_session(MessageEnd::worker(worker))
                .unwrap(),
            "and once a session exists the same call must say so, or it is a constant again"
        );
    }

    /// Queen is reachable by ROLE, not by an id, exactly as delivery resolves
    /// her — otherwise the reply direction would report unreachable forever.
    #[test]
    fn queen_is_reachable_through_whoever_holds_the_role() {
        let (store, _task, _worker) = hive();
        let queen = store.ensure_queen("/workspace/queen").unwrap();

        assert!(
            !store
                .recipient_has_open_session(MessageEnd::queen())
                .unwrap(),
            "no queen session means a worker's reply cannot be delivered"
        );

        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();

        assert!(
            store
                .recipient_has_open_session(MessageEnd::queen())
                .unwrap(),
            "a message to Queen is addressed to the office, so any queen session makes it reachable"
        );
    }

    /// A BROADCAST MUST SAY WHO IT DID NOT REACH.
    ///
    /// The dispatch join requires a live session, so a worker without one is
    /// excluded from delivery rather than queued for it. Measured on the
    /// operator's Hive when this was built: 13 of 45 workers had a session. A
    /// broadcast that answered "sent" would let them believe 45 people were
    /// told, which is worse than telling 13 by hand — that way they would know
    /// they had stopped.
    #[test]
    fn a_broadcast_reports_the_workers_it_could_not_reach() {
        let (store, _task, worker) = hive();
        let asleep = store
            .create_worker(
                "Asleep",
                ProviderKind::ClaudeCode,
                "/workspace/two",
                false,
                2,
            )
            .unwrap();
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();

        let broadcast = store
            .broadcast_to_workers("reloading the engine in five minutes", 1_000)
            .unwrap();

        assert_eq!(broadcast.reached, 1, "only the worker with a live session");
        assert!(
            broadcast.skipped >= 1,
            "and the one with no session is reported, not silently dropped: {broadcast:?}"
        );
        let _ = asleep;

        let pending = store.pending_operator_broadcast_dispatches().unwrap();
        assert_eq!(pending.len(), 1, "only reachable workers are queued");
        assert_eq!(pending[0].body, "reloading the engine in five minutes");
    }

    /// A delivered broadcast is not delivered twice.
    #[test]
    fn a_delivered_broadcast_leaves_the_queue() {
        let (store, _task, worker) = hive();
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();
        let broadcast = store.broadcast_to_workers("heads up", 1_000).unwrap();
        let pending = store.pending_operator_broadcast_dispatches().unwrap();
        assert_eq!(pending.len(), 1);

        store
            .mark_operator_broadcast_delivered(&broadcast.id, &worker.to_string(), 2_000)
            .unwrap();

        assert!(
            store
                .pending_operator_broadcast_dispatches()
                .unwrap()
                .is_empty(),
            "a delivered broadcast that stays queued arrives again every pass"
        );
    }

    /// Both directions between Queen and a worker, recorded on the task.
    #[test]
    fn queen_and_a_worker_can_both_start_an_exchange() {
        let (store, task, worker) = hive();

        store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "Which SHA did this ship as?",
                1_000,
            )
            .unwrap();
        assert!(
            store.worker_owes_an_answer(task).unwrap(),
            "an unanswered question is a debt the board can see"
        );

        store
            .send_task_message(
                task,
                MessageEnd::worker(worker),
                MessageEnd::queen(),
                "9ddfdd7, and CI was green on it.",
                2_000,
            )
            .unwrap();
        assert!(
            !store.worker_owes_an_answer(task).unwrap(),
            "answering discharges it"
        );

        let exchange = store.task_messages(task).unwrap();
        assert_eq!(exchange.len(), 2, "the whole exchange lives on the task");
        assert_eq!(exchange[0].sender, MessageParty::Queen);
        assert_eq!(exchange[1].sender, MessageParty::Worker);
    }

    /// A message waits for the terminal rather than being lost or forced in.
    ///
    /// Delivery is separate on purpose: it holds until the session is resting,
    /// which is what stops a question arriving mid-turn and taking the thread
    /// with it. Until then it must still be findable.
    #[test]
    fn an_undelivered_message_is_queued_rather_than_dropped() {
        let (store, task, worker) = hive();
        store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "Are you still on this?",
                1_000,
            )
            .unwrap();

        let waiting = store.undelivered_task_messages(worker).unwrap();
        assert_eq!(waiting.len(), 1);

        store
            .mark_task_message_delivered(&waiting[0].id, 2_000)
            .unwrap();
        assert!(
            store.undelivered_task_messages(worker).unwrap().is_empty(),
            "delivered once, not on every pass"
        );
    }

    /// A REPLY MUST BE DELIVERABLE TOO, and it was not.
    ///
    /// The dispatch query filtered `recipient = 'worker'`, so every
    /// worker-to-Queen message was recorded and never delivered while the tool
    /// that sent it said "Queen sees it on her next run". Two sat unread for
    /// nearly an hour. Silence is the one failure this channel exists to
    /// remove and it had it in the reply direction, which is the direction
    /// nobody thinks to check.
    ///
    /// Queen is resolved by ROLE: a message to her is addressed to the office,
    /// so the row carries no recipient worker id and whoever holds the role
    /// reads it.
    #[test]
    fn a_message_to_queen_is_dispatched_as_readily_as_one_to_a_worker() {
        let (store, task, worker) = hive();
        // BOTH ends need a live terminal, because a message waits for one
        // rather than being dropped. Binding only Queen's would have proved
        // half of what this test claims.
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();
        let queen = store.ensure_queen("/workspace/queen").unwrap();
        store
            .bind_worker_session(queen.id, WorkerSessionId::new())
            .unwrap();

        store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "Which SHA did this ship as?",
                1_000,
            )
            .unwrap();
        store
            .send_task_message(
                task,
                MessageEnd::worker(worker),
                MessageEnd::queen(),
                "9ddfdd7, and CI was green on it.",
                2_000,
            )
            .unwrap();

        let pending = store.pending_task_message_dispatches().unwrap();
        assert_eq!(
            pending.len(),
            2,
            "both directions must reach a terminal, not just the outbound one"
        );
        assert!(
            pending.iter().any(|d| d.sender == MessageParty::Worker),
            "the REPLY is the one that was silently dropped: {pending:?}"
        );
    }

    /// A question with nowhere to arrive is refused rather than sent nowhere.
    ///
    /// Silence is the one failure this channel must not have: an undelivered
    /// instruction is indistinguishable from none.
    #[test]
    fn a_message_to_no_particular_worker_is_refused() {
        let (store, task, _worker) = hive();
        let refused = store.send_task_message(
            task,
            MessageEnd::queen(),
            MessageEnd {
                party: MessageParty::Worker,
                worker_id: None,
            },
            "Somebody look at this.",
            1_000,
        );
        assert!(matches!(
            refused,
            Err(TaskStoreError::InvalidTaskMessage { .. })
        ));
    }
}

/// One operator broadcast and, crucially, who it could actually reach.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperatorBroadcast {
    pub id: String,
    pub body: String,
    /// Workers with a live session, which is the only place a message can land.
    pub reached: usize,
    /// Workers with NO live session. Not slow — excluded.
    pub skipped: usize,
}

/// One broadcast waiting to reach one terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorBroadcastDispatch {
    pub broadcast_id: String,
    pub worker_id: String,
    pub session_id: swarm_domain::WorkerSessionId,
    pub body: String,
}

impl TaskStore {
    /// Records a broadcast and queues it for every worker with a live session.
    ///
    /// THE COUNTS ARE THE FEATURE. Measured when this was built: 13 of 45
    /// workers had an open session. A broadcast that reports success without
    /// saying so lets the operator believe 45 people were told, which is worse
    /// than telling 13 by hand — they would at least know they had stopped.
    ///
    /// # Errors
    /// Refuses an empty or oversized body; returns an error when persistence
    /// is unavailable.
    pub fn broadcast_to_workers(
        &self,
        body: &str,
        now: i64,
    ) -> Result<OperatorBroadcast, TaskStoreError> {
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_TASK_MESSAGE_BYTES {
            return Err(TaskStoreError::InvalidTaskMessage {
                max: MAX_TASK_MESSAGE_BYTES,
            });
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT INTO operator_broadcasts (id, body, created_at) VALUES (?1, ?2, ?3)",
            params![id, body, now],
        )?;
        // A live session is what the dispatch join requires, so this is the
        // same reachability the delivery pass will see rather than a second
        // opinion about it.
        let reached = transaction.execute(
            "INSERT INTO operator_broadcast_deliveries (broadcast_id, worker_id, session_id)
             SELECT ?1, worker.id, session.session_id
             FROM worker_profiles worker
             JOIN worker_sessions session
               ON session.worker_id = worker.id AND session.ended_at IS NULL",
            params![id],
        )?;
        let total: i64 =
            transaction.query_row("SELECT COUNT(*) FROM worker_profiles", [], |row| row.get(0))?;
        transaction.commit()?;
        Ok(OperatorBroadcast {
            id,
            body: body.to_owned(),
            reached,
            skipped: usize::try_from(total).unwrap_or(0).saturating_sub(reached),
        })
    }

    /// Broadcasts still waiting for a terminal to be resting.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn pending_operator_broadcast_dispatches(
        &self,
    ) -> Result<Vec<OperatorBroadcastDispatch>, TaskStoreError> {
        use std::str::FromStr;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT delivery.broadcast_id, delivery.worker_id, delivery.session_id, broadcast.body
             FROM operator_broadcast_deliveries delivery
             JOIN operator_broadcasts broadcast ON broadcast.id = delivery.broadcast_id
             -- The session must still be the live one. A worker restarted since
             -- the broadcast was written has a different session, and writing
             -- into the old one reaches a terminal nobody is watching.
             JOIN worker_sessions session
               ON session.session_id = delivery.session_id AND session.ended_at IS NULL
             WHERE delivery.delivered_at IS NULL
             ORDER BY broadcast.created_at, delivery.worker_id",
        )?;
        let dispatches = statement
            .query_map([], |row| {
                let session_id: String = row.get(2)?;
                Ok(OperatorBroadcastDispatch {
                    broadcast_id: row.get(0)?,
                    worker_id: row.get(1)?,
                    session_id: swarm_domain::WorkerSessionId::from_str(&session_id)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    body: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(dispatches)
    }

    /// Marks one worker's copy of a broadcast as delivered.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn mark_operator_broadcast_delivered(
        &self,
        broadcast_id: &str,
        worker_id: &str,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE operator_broadcast_deliveries SET delivered_at = ?3
             WHERE broadcast_id = ?1 AND worker_id = ?2 AND delivered_at IS NULL",
            params![broadcast_id, worker_id, now],
        )?;
        Ok(())
    }
}
