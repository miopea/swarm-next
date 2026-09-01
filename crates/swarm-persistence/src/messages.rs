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
    use swarm_domain::{ProviderKind, TaskState};

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
