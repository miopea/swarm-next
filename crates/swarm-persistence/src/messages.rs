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
    /// WHICH SESSION took it, alongside when.
    ///
    /// `delivered_at` on its own made a message written into a session that
    /// then exited indistinguishable from one the running worker read and
    /// acted on. Both read as delivered, so the sender stopped chasing the one
    /// nobody living had been told about.
    ///
    /// `None` on an undelivered message means it has not gone anywhere. `None`
    /// on a DELIVERED one means it predates schema 121 — not that it went
    /// nowhere. Nothing in the record says which session took those, and
    /// inventing one would read exactly like a measured answer.
    pub delivered_session_id: Option<swarm_domain::WorkerSessionId>,
    /// Whether the session it was written into IS STILL OPEN.
    ///
    /// This is the question Queen was answering by hand, comparing a delivery
    /// timestamp against the session id from `swarm_list_workers`. False on a
    /// delivered message means it was written into a terminal that no longer
    /// exists: the bytes were typed, and nothing running was ever told.
    ///
    /// A worker has at most one open session at a time — that is the property
    /// `LIVE_RECIPIENT_SESSION_JOIN` already relies on — so "still open" and
    /// "the one running now" are the same session here. Stated as still-open
    /// because that is what is measured.
    ///
    /// False for a message that has not been delivered at all, and false for
    /// one delivered before schema 121, because in neither case is there a
    /// live session it demonstrably reached. Read it with `delivered_at`, not
    /// instead of it.
    pub reached_the_current_session: bool,
}

/// The largest message the channel accepts.
///
/// A question, not a second description. Something longer is a task amendment
/// or a new task, and letting it through here would make the channel a way to
/// redirect work with no record of the work changing.
pub const MAX_TASK_MESSAGE_BYTES: usize = 4_000;

/// The join that finds a recipient's LIVE terminal, owned in one place.
///
/// This is the only thing the two dispatch queries share, and it is the thing
/// they diverged on. Task messages joined by WORKER; the broadcast query, written
/// beside it months later and 590 lines below, joined by SESSION — so a worker
/// restart left every queued broadcast matching nothing. 14 queued, 0 delivered,
/// silent in both directions.
///
/// The rest of the two queries differs legitimately — one resolves a Queen
/// recipient by role and carries a task and a sender name, the other carries a
/// body and a delivery window — so consolidating them WOULD BE WRONG. Only the
/// shared idea is shared, and the `{recipient}` a caller substitutes is the
/// column naming whose terminal to find.
///
/// A fragment is a weak abstraction and cannot stop a third query being written
/// without it. That is what the per-path restart tests are for.
const LIVE_RECIPIENT_SESSION_JOIN: &str = "JOIN worker_sessions session
       ON session.worker_id = {recipient} AND session.ended_at IS NULL";

/// The columns every read of a message must carry, so no reader can be handed
/// a delivery record that says when without saying where.
///
/// Written as one string for the same reason as `LIVE_RECIPIENT_SESSION_JOIN`:
/// there are three queries reading this table and they had already drifted
/// once. `message_from_row` reads these by position, so a query that selects a
/// different list fails loudly rather than quietly returning the wrong column.
const MESSAGE_COLUMNS: &str = "m.id, m.task_id, m.sender, m.recipient, m.sender_worker_id,
            m.recipient_worker_id, m.body, m.created_at, m.delivered_at,
            m.delivered_session_id,
            EXISTS(SELECT 1 FROM worker_sessions live
                   WHERE live.session_id = m.delivered_session_id
                     AND live.ended_at IS NULL)";

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
        let connection = self.connection()?;
        Self::insert_task_message(&connection, task_id, from, to, body, now)
    }

    /// Shared insertion boundary for messages that accompany a domain change.
    pub(super) fn insert_task_message(
        connection: &rusqlite::Connection,
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
            delivered_session_id: None,
            reached_the_current_session: false,
        })
    }

    /// Reads a task's exchange, oldest first.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable or stored data is corrupt.
    pub fn task_messages(&self, task_id: TaskId) -> Result<Vec<TaskMessage>, TaskStoreError> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM task_messages m WHERE m.task_id = ?1
             ORDER BY m.created_at, m.id"
        );
        let mut statement = connection.prepare(&sql)?;
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
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM task_messages m
             WHERE m.recipient_worker_id = ?1 AND m.delivered_at IS NULL
             ORDER BY m.created_at, m.id"
        );
        let mut statement = connection.prepare(&sql)?;
        let messages = statement
            .query_map([worker_id.to_string()], message_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// Marks one message as having reached a terminal, AND NAMES WHICH ONE.
    ///
    /// The session is not optional. A delivery that records only a timestamp
    /// cannot be told apart from one written into a session that has since
    /// exited, and that is the whole defect this parameter exists to close —
    /// the broadcast path has recorded its session since schema 119.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn mark_task_message_delivered(
        &self,
        id: &str,
        session_id: swarm_domain::WorkerSessionId,
        now: i64,
    ) -> Result<(), TaskStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE task_messages SET delivered_at = ?2, delivered_session_id = ?3
             WHERE id = ?1 AND delivered_at IS NULL",
            params![id, now, session_id.to_string()],
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
        // BOTH DIRECTIONS. This filtered `m.recipient = 'worker'`, so every
        // worker-to-Queen message was recorded and never delivered — the
        // channel was one-way while its tool told the worker "Queen sees it on
        // her next run". Silence is the single failure this channel exists to
        // remove, and it had it in the reply direction.
        //
        // Queen is resolved by ROLE rather than by an id on the row, because a
        // message to Queen is addressed to the office: the recipient_worker_id
        // is null and whoever holds the role reads it.
        //
        // The live-session join is substituted from its one owner, so this
        // query and the broadcast one cannot drift apart on it again.
        let sql = format!(
            "SELECT m.id, m.task_id, task.title, session.session_id, m.sender,
                    COALESCE(sender.name, 'Queen'), m.body
             FROM task_messages m
             JOIN tasks task ON task.id = m.task_id AND task.removed_at IS NULL
             JOIN worker_profiles recipient
                  ON (m.recipient = 'worker' AND recipient.id = m.recipient_worker_id)
                  OR (m.recipient = 'queen' AND recipient.role = 'queen')
             {live_session}
             LEFT JOIN worker_profiles sender ON sender.id = m.sender_worker_id
             WHERE m.delivered_at IS NULL
             ORDER BY m.created_at, m.id",
            live_session = LIVE_RECIPIENT_SESSION_JOIN.replace("{recipient}", "recipient.id"),
        );
        let mut statement = connection.prepare(&sql)?;
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
        delivered_session_id: row
            .get::<_, Option<String>>(9)?
            .as_deref()
            .map(swarm_domain::WorkerSessionId::from_str)
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        reached_the_current_session: row.get(10)?,
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

    /// QUEEN'S FALSIFIER, RUN DETERMINISTICALLY RATHER THAN ON A LIVE WORKER.
    ///
    /// She wrote it as: send a message, exit the session, restart, and see
    /// whether anything says the two are different. Doing that live means
    /// killing a real worker's session, so it is done here instead — the
    /// sequence is the same and it is repeatable.
    ///
    /// TWO MESSAGES, IDENTICAL RECORDS, DIFFERENT TRUTHS. One is delivered into
    /// a session that then exits; the other into the session that is running
    /// now. Both come back with a `delivered_at` and nothing else, so the sender
    /// sees "delivered" for a message no living session was ever told about and
    /// stops chasing it.
    ///
    /// Queen only caught the case that prompted this by reading a delivery
    /// timestamp against the session id from `swarm_list_workers` BY HAND. That
    /// hand comparison is the thing this must remove.
    ///
    /// THE BROADCAST PATH IN THIS SAME FILE ALREADY RECORDS THE SESSION.
    /// `operator_broadcast_deliveries` carries `session_id`; `task_messages` does
    /// not. Nobody decided that — it is divergence by authorship, the same
    /// shape as the two dispatch queries that disagreed about following a
    /// worker or a session.
    #[test]
    fn a_message_taken_by_a_session_that_exited_is_told_apart_from_one_the_live_session_took() {
        let (store, task, worker) = hive();

        let departed = WorkerSessionId::new();
        store.bind_worker_session(worker, departed).unwrap();
        let stranded = store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "which half shipped?",
                1_000,
            )
            .unwrap();
        store
            .mark_task_message_delivered(&stranded.id, departed, 1_001)
            .unwrap();
        store.release_worker_session(departed).unwrap();

        let current = WorkerSessionId::new();
        store.bind_worker_session(worker, current).unwrap();
        let read = store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "and the second half?",
                2_000,
            )
            .unwrap();
        store
            .mark_task_message_delivered(&read.id, current, 2_001)
            .unwrap();

        let messages = store.task_messages(task).unwrap();
        let stranded = messages
            .iter()
            .find(|message| message.id == stranded.id)
            .expect("the stranded message is on the task");
        let read = messages
            .iter()
            .find(|message| message.id == read.id)
            .expect("the read message is on the task");

        assert!(
            stranded.delivered_at.is_some() && read.delivered_at.is_some(),
            "both were written into a terminal, which is all delivered_at has ever meant"
        );
        assert_eq!(
            stranded.delivered_session_id,
            Some(departed),
            "a delivery records WHERE it went, not only when"
        );
        assert_eq!(read.delivered_session_id, Some(current));
        assert!(
            !stranded.reached_the_current_session,
            "this one was written into a session that no longer exists, and the sender \
             must be able to see that without comparing timestamps to swarm_list_workers"
        );
        assert!(
            read.reached_the_current_session,
            "and this one was not, so the two must not read alike"
        );
    }

    /// DECIDED, NOT INHERITED: a delivered message is NOT re-delivered when the
    /// session that took it exits. This test is the decision.
    ///
    /// 01a06340 required this to be answered explicitly, on the grounds that
    /// re-delivery is not obviously right. It is not, and the answer is no.
    ///
    /// THE BYTES WERE WRITTEN INTO A TERMINAL. A session that received a
    /// question may well have read it and acted on it — which is exactly what
    /// happened in the case that prompted the ticket. Queen concluded a request
    /// had been lost to an exited session; it had not, the worker had acted,
    /// and both halves of what it asked for shipped. Re-delivering would have
    /// presented finished work as a new request, and for anything with a side
    /// effect that is a worse failure than a visible gap.
    ///
    /// IT IS ALSO THE SAME RULE BROADCASTS ALREADY FOLLOW, not a different one.
    /// 01a062f4 answered "follows the worker or expires" for a broadcast that
    /// was never written anywhere — an UNDELIVERED one. Undelivered task
    /// messages already re-aim the same way, which
    /// `a_worker_restart_re_aims_a_task_message_too` pins. Neither path
    /// re-delivers something already typed into a terminal. The rule is about
    /// undelivered work in both cases, and this makes that explicit rather than
    /// letting the two look like they disagree.
    ///
    /// THE GAP IS CLOSED BY MAKING IT VISIBLE, NOT BY RESENDING. The sender can
    /// see `reached_the_current_session` is false and re-send deliberately.
    /// That turns a silent loss into somebody's decision, which is the whole
    /// difference this ticket was filed about.
    #[test]
    fn a_message_already_written_into_a_terminal_is_not_delivered_again_after_a_restart() {
        let (store, task, worker) = hive();
        let departed = WorkerSessionId::new();
        store.bind_worker_session(worker, departed).unwrap();
        let message = store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "which half shipped?",
                1_000,
            )
            .unwrap();
        store
            .mark_task_message_delivered(&message.id, departed, 1_001)
            .unwrap();
        store.release_worker_session(departed).unwrap();

        let current = WorkerSessionId::new();
        store.bind_worker_session(worker, current).unwrap();

        assert!(
            store.pending_task_message_dispatches().unwrap().is_empty(),
            "a message already typed into a terminal is not typed again — the previous \
             session may have acted on it, and re-sending would present finished work as new"
        );
        let seen = store.task_messages(task).unwrap();
        let seen = seen.first().expect("the message is still on the task");
        assert!(
            seen.delivered_at.is_some() && !seen.reached_the_current_session,
            "and the gap is closed by being VISIBLE: the sender can see nothing running was \
             told, and re-send on purpose"
        );
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

        let pending = store.pending_operator_broadcast_dispatches(1_000).unwrap();
        assert_eq!(pending.len(), 1, "only reachable workers are queued");
        assert_eq!(pending[0].body, "reloading the engine in five minutes");
    }

    /// THE SAME PROPERTY, ASSERTED FOR THE OTHER PATH.
    ///
    /// Task messages already survived a worker restart — that is why Queen's
    /// messages were delivered while every broadcast stranded. But nothing
    /// TESTED it: the property held because of how the query happened to be
    /// written, and the broadcast query was written differently right beside it.
    ///
    /// A shared SQL fragment stops these two drifting. It cannot stop a THIRD
    /// dispatch query being written without it, so each path asserts the
    /// behaviour itself. This is the assertion that did not exist.
    #[test]
    fn a_worker_restart_re_aims_a_task_message_too() {
        let (store, task, worker) = hive();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker, first).unwrap();
        store
            .send_task_message(
                task,
                MessageEnd::queen(),
                MessageEnd::worker(worker),
                "which half shipped?",
                1_000,
            )
            .unwrap();

        store.release_worker_session(first).unwrap();
        assert!(
            store.pending_task_message_dispatches().unwrap().is_empty(),
            "with no live terminal there is nothing to write into"
        );

        let second = WorkerSessionId::new();
        store.bind_worker_session(worker, second).unwrap();
        let pending = store.pending_task_message_dispatches().unwrap();
        assert_eq!(
            pending.len(),
            1,
            "the message follows the worker, not one session"
        );
        assert_eq!(
            pending[0].session_id, second,
            "and is aimed at the terminal that exists now"
        );
    }

    /// A RESTART RE-AIMS A BROADCAST INSTEAD OF ORPHANING IT.
    ///
    /// This is the defect that shipped. Deliveries were pinned to the session
    /// that existed when the broadcast was written, so when the operator
    /// pressed Force worker reload three minutes after broadcasting, all 14
    /// deliveries pointed at dead sessions: never delivered, never retried,
    /// never expired, never reported. Measured 2026-09-02, 14 queued and 0
    /// delivered.
    #[test]
    fn a_worker_restart_re_aims_a_broadcast_rather_than_stranding_it() {
        let (store, _task, worker) = hive();
        let first = WorkerSessionId::new();
        store.bind_worker_session(worker, first).unwrap();
        let broadcast = store.broadcast_to_workers("pause please", 1_000).unwrap();
        assert_eq!(broadcast.reached, 1);

        // The session the broadcast was queued against ends, exactly as a force
        // worker reload ends it.
        store.release_worker_session(first).unwrap();
        assert!(
            store
                .pending_operator_broadcast_dispatches(1_000)
                .unwrap()
                .is_empty(),
            "with no live terminal there is nothing to write into"
        );

        // The worker comes back. The delivery must find it.
        let second = WorkerSessionId::new();
        store.bind_worker_session(worker, second).unwrap();
        let pending = store.pending_operator_broadcast_dispatches(1_060).unwrap();
        assert_eq!(
            pending.len(),
            1,
            "the broadcast follows the worker, not one session"
        );
        assert_eq!(
            pending[0].session_id, second,
            "and it is aimed at the terminal that exists now, not the one that is gone"
        );
    }

    /// AND IT DOES NOT ARRIVE LATE. The operator ruled the window: a broadcast
    /// describes now, and "pause work so I can reload" delivered after the
    /// reload is worse than never delivered.
    #[test]
    fn a_broadcast_past_its_window_expires_with_a_reason_rather_than_waiting() {
        let (store, _task, worker) = hive();
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();
        let broadcast = store.broadcast_to_workers("pause please", 1_000).unwrap();

        let past = 1_000 + BROADCAST_DELIVERY_WINDOW_SECONDS + 1;
        assert!(
            store
                .pending_operator_broadcast_dispatches(past)
                .unwrap()
                .is_empty(),
            "a stale broadcast is not delivered"
        );
        assert_eq!(
            store.expire_stale_broadcasts(past).unwrap(),
            1,
            "and it is closed rather than left pending forever"
        );

        let (delivered, expired, waiting) =
            store.operator_broadcast_outcome(&broadcast.id).unwrap();
        assert_eq!((delivered, expired, waiting), (0, 1, 0));
    }

    /// A delivered broadcast is not delivered twice.
    #[test]
    fn a_delivered_broadcast_leaves_the_queue() {
        let (store, _task, worker) = hive();
        store
            .bind_worker_session(worker, WorkerSessionId::new())
            .unwrap();
        let broadcast = store.broadcast_to_workers("heads up", 1_000).unwrap();
        let pending = store.pending_operator_broadcast_dispatches(1_000).unwrap();
        assert_eq!(pending.len(), 1);

        store
            .mark_operator_broadcast_delivered(&broadcast.id, &worker.to_string(), 2_000)
            .unwrap();

        assert!(
            store
                .pending_operator_broadcast_dispatches(1_000)
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
            .mark_task_message_delivered(&waiting[0].id, WorkerSessionId::new(), 2_000)
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

/// How long a broadcast is worth delivering after it was written.
///
/// A broadcast describes NOW — the first real one was "Please pause work so I
/// can reload" — so arriving late is worse than not arriving. Ten minutes is
/// long enough for a worker restarted by that very reload to come back and
/// receive it, and short enough that nothing wakes tomorrow to an instruction
/// about yesterday.
pub const BROADCAST_DELIVERY_WINDOW_SECONDS: i64 = 600;

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
        now: i64,
    ) -> Result<Vec<OperatorBroadcastDispatch>, TaskStoreError> {
        use std::str::FromStr;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            // AIMED AT THE WORKER, NOT AT ONE SESSION OF IT.
            //
            // This joined on delivery.session_id, so a delivery was pinned to
            // the session that existed when the broadcast was written. Not
            // writing into a dead terminal was right; STRANDING the message
            // when that session ended was not. Measured 2026-09-02 on the first
            // real broadcast: 14 queued, 0 delivered, and after a force worker
            // reload all 14 pointed at dead sessions — never delivered, never
            // retried, never expired, never reported.
            //
            // So it now finds the worker's CURRENT live session, and a restart
            // re-aims the delivery instead of orphaning it.
            &format!("SELECT delivery.broadcast_id, delivery.worker_id, session.session_id, broadcast.body
             FROM operator_broadcast_deliveries delivery
             JOIN operator_broadcasts broadcast ON broadcast.id = delivery.broadcast_id
             {live_session}
             WHERE delivery.delivered_at IS NULL
               AND delivery.expired_at IS NULL
               -- TIME-BOXED, because a broadcast describes NOW. The operator's
               -- own case was \"pause work so I can reload\": delivering that
               -- after the reload is worse than not delivering it. A worker
               -- back within the window still gets it; one returning tomorrow
               -- does not, and is expired with a reason rather than silently.
               AND broadcast.created_at > ?1 - ?2
             ORDER BY broadcast.created_at, delivery.worker_id",
            live_session = LIVE_RECIPIENT_SESSION_JOIN.replace("{recipient}", "delivery.worker_id"),
            ),
        )?;
        let dispatches = statement
            .query_map(params![now, BROADCAST_DELIVERY_WINDOW_SECONDS], |row| {
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

    /// Expires deliveries that ran out of their window, with a reason.
    ///
    /// THE ONE OUTCOME THAT MUST NOT REMAIN IS SILENCE. Before this, a delivery
    /// whose session ended simply stopped matching anything: not delivered, not
    /// retried, not expired, not reported. Returns how many it closed so a
    /// caller can say so rather than discover it later.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn expire_stale_broadcasts(&self, now: i64) -> Result<usize, TaskStoreError> {
        let connection = self.connection()?;
        let expired = connection.execute(
            "UPDATE operator_broadcast_deliveries
             SET expired_at = ?1,
                 expiry_reason = 'the worker did not have a live terminal within the delivery window'
             WHERE delivered_at IS NULL AND expired_at IS NULL
               AND broadcast_id IN (
                   SELECT id FROM operator_broadcasts WHERE created_at <= ?1 - ?2
               )",
            params![now, BROADCAST_DELIVERY_WINDOW_SECONDS],
        )?;
        Ok(expired)
    }

    /// What actually became of one broadcast.
    ///
    /// The send response could only report who it QUEUED for, which read as
    /// reach and was not: the first real broadcast reported 14 and delivered 0.
    ///
    /// # Errors
    /// Returns an error when persistence is unavailable.
    pub fn operator_broadcast_outcome(
        &self,
        broadcast_id: &str,
    ) -> Result<(usize, usize, usize), TaskStoreError> {
        let connection = self.connection()?;
        let row = connection.query_row(
            "SELECT
                 SUM(delivered_at IS NOT NULL),
                 SUM(expired_at IS NOT NULL),
                 SUM(delivered_at IS NULL AND expired_at IS NULL)
             FROM operator_broadcast_deliveries WHERE broadcast_id = ?1",
            [broadcast_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                ))
            },
        )?;
        Ok((
            usize::try_from(row.0).unwrap_or(0),
            usize::try_from(row.1).unwrap_or(0),
            usize::try_from(row.2).unwrap_or(0),
        ))
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
